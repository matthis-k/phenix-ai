//! Owned jobs, supervision, retained publication and cooperative dispatch.
// The wall-clock deadline-report helper stays here with the driver that calls it;
// observation only inspects deadlines against a caller-supplied instant.
use super::*;

impl NetworkSession {
    fn next_deferred_timeout(&self) -> Option<Duration> {
        let timeout = self.deferred_timeout?;
        let now = Instant::now();
        self.dispatched_at
            .iter()
            .filter_map(|(index, since)| {
                self.processes[*index]
                    .handoff
                    .outstanding()
                    .map(|_| timeout.saturating_sub(now.saturating_duration_since(*since)))
            })
            .min()
    }

    fn timeout_invalidated(&self, ticket: StepTicket) -> bool {
        self.requests
            .values()
            .any(|scope| scope.timed_out && scope.cancelled_jobs.contains(&ticket))
    }

    fn record_late_return_cleanup_panic(&mut self, request: Option<RequestId>) {
        let failure = request
            .and_then(|request| self.requests.get_mut(&request))
            .and_then(|scope| scope.failure.as_mut())
            .or(self.failure.as_mut());
        if let Some(failure) = failure {
            failure.push_str("; destructor panicked during late dispatcher return cleanup");
        }
    }

    pub fn take_dispatch_return(&mut self) -> Option<DispatchReturn> {
        self.dispatch_return.take()
    }

    pub(super) fn supervise_configured_deferred(&mut self) {
        if self.deferred_timeout.is_some() {
            self.supervise_deferred(Instant::now());
        }
    }

    /// Host-driven watchdog. Failing a request or session invalidates outstanding
    /// tickets but cannot revoke worker ownership or undo external effects.
    pub fn supervise_deferred(&mut self, now: Instant) -> Vec<OverdueJob> {
        let overdue = self.overdue_jobs(now);
        for job in &overdue {
            let index = job.process;
            if self.processes[index].handoff.outstanding() != Some(job.ticket) {
                continue;
            }
            self.dispatched_at.remove(&index);
            let request = self.processes[index].request;
            if let Some(request) = request {
                self.requests.get_mut(&request).unwrap().timed_out = true;
            }
            self.fail_process(index, format!(
                "deferred job timeout for process {} ({}, request={request:?}) at activation {} step {}",
                index, self.compiled.processes()[index].path(),
                job.ticket.activation(), job.ticket.sequence()
            ));
        }
        overdue
    }

    /// Completion authority stays in the handoff; the runner owns the job.
    /// A configured deadline starts when this method returns the job.
    /// Publication retries happen here, never by re-running a completed step.
    pub fn next_job(&mut self) -> Option<(usize, ActivationJob)> {
        self.supervise_configured_deferred();
        let (index, job) = self.next_job_inner()?;
        if self.deferred_timeout.is_some() {
            self.dispatched_at.insert(index, Instant::now());
        }
        Some((index, job))
    }

    fn next_job_inner(&mut self) -> Option<(usize, ActivationJob)> {
        self.refresh();
        while let Some(index) = self.ready.pop_front() {
            self.processes[index].queued = false;
            if self.processes[index].pending.is_some() {
                self.publish(index);
                self.refresh();
                if self.terminal.is_some() {
                    return None;
                }
                continue;
            }
            let mut job = self.processes[index].handoff.next_job().unwrap();
            let ticket = job.ticket();
            job = job.with_metadata(ActivationMetadata {
                session: self.session_id,
                generation: self.generation,
                request: self.processes[index].request,
                process: index,
                path: self.compiled.processes()[index].path().into(),
                activation: ticket.activation(),
                step: ticket.sequence(),
                output: None,
                origin: self.compiled.processes()[index].origin().cloned(),
            });
            for guard in &self.execution_guards {
                job = job.with_guard(guard.clone());
            }
            return Some((index, job));
        }
        None
    }
    pub fn complete(
        &mut self,
        index: usize,
        completion: CompletedStep,
    ) -> Result<(), (HandoffError, Box<CompletedStep>)> {
        self.supervise_configured_deferred();
        let ticket = completion.ticket();
        let Some(p) = self.processes.get_mut(index) else {
            return Err((HandoffError::ForeignCompletion, Box::new(completion)));
        };
        let result = p.handoff.complete(completion)?;
        self.dispatched_at.remove(&index);
        self.last_step = Some(ActivationMetadata {
            session: self.session_id,
            generation: self.generation,
            request: self.processes[index].request,
            process: index,
            path: self.compiled.processes()[index].path().into(),
            activation: ticket.activation(),
            step: ticket.sequence(),
            output: match &result {
                Ok(Step::Emit(emission)) => Some(emission.output.clone()),
                _ => None,
            },
            origin: self.compiled.processes()[index].origin().cloned(),
        });
        match result {
            Ok(Step::Emit(emission)) => {
                let process = &self.compiled.processes()[index];
                let actual = emission.value.identity();
                let error = match process.interface().output(&emission.output) {
                    None => Some(format!(
                        "invalid emission from process {index} ({}) output {:?}: unknown output",
                        process.path(),
                        emission.output
                    )),
                    Some(port) if port.value != actual => Some(format!(
                        "invalid emission from process {index} ({}) output {:?}: incompatible value identity (expected {:?}, actual {actual:?})",
                        process.path(),
                        emission.output,
                        port.value
                    )),
                    Some(_) => None,
                };
                if let Some(error) = error {
                    self.fail_process(index, error);
                    return Ok(());
                }
                let outgoing = match (self.processes[index].request, self.request_queue_policy) {
                    (Some(request), RequestQueuePolicy::BoundedLanes) => {
                        let output_key = (index, emission.output.clone());
                        let lane = self.requests[&request].lane;
                        self.request_lanes
                            .get(&lane)
                            .and_then(|lane| lane.outputs.get(&output_key))
                            .map(Vec::as_slice)
                            .unwrap_or(&[])
                    }
                    _ => self.processes[index].outputs[&emission.output].as_slice(),
                };
                let indices = if let Some(destinations) = &emission.destinations {
                    let selection = (|| -> Result<Vec<usize>, String> {
                        let budget = outgoing
                            .iter()
                            .filter(|q| {
                                matches!(self.endpoints[**q].1, QueueEndpoint::Process { .. })
                            })
                            .count();
                        // Reject oversized process-produced lists before allocating
                        // routing maps, copying targets, or preparing any payload.
                        if destinations.len() > budget {
                            return Err(format!(
                                "selection exceeds compiled outgoing connection budget ({budget})"
                            ));
                        }
                        let connected: BTreeMap<_, _> = outgoing
                            .iter()
                            .filter_map(|q| match &self.endpoints[*q].1 {
                                QueueEndpoint::Process { index, port } => {
                                    Some(((*index, port.as_str()), *q))
                                }
                                _ => None,
                            })
                            .collect();
                        let mut selected = BTreeSet::new();
                        for (position, path) in destinations.iter().enumerate() {
                            let resolved = self.compiled.resolve_input(path).ok_or_else(|| {
                                format!("unknown selected input at destination {position}")
                            })?;
                            if resolved.is_empty() {
                                return Err("selected input has no physical destinations".into());
                            }
                            for (target, input) in resolved {
                                let queue =
                                    connected.get(&(*target, input.as_str())).ok_or_else(|| {
                                        format!(
                                            "selected input {}.{} is not connected to this output",
                                            self.compiled.processes()[*target].path(),
                                            input
                                        )
                                    })?;
                                if !selected.insert(*queue) {
                                    return Err(format!(
                                        "duplicate selected input after alias resolution: {}.{}",
                                        self.compiled.processes()[*target].path(),
                                        input
                                    ));
                                }
                            }
                        }
                        Ok(outgoing
                            .iter()
                            .copied()
                            .filter(|q| {
                                selected.contains(q)
                                    || matches!(self.endpoints[*q].1, QueueEndpoint::Capture(_))
                            })
                            .collect())
                    })();
                    match selection {
                        Ok(indices) => indices,
                        Err(error) => {
                            self.fail_process(index, error);
                            return Ok(());
                        }
                    }
                } else {
                    outgoing.to_vec()
                };
                if indices.is_empty()
                    && emission.destinations.is_none()
                    && !self.discard_outputs.contains(&(
                        self.compiled.processes()[index].path().to_owned(),
                        emission.output.clone(),
                    ))
                {
                    self.fail_process(
                        index,
                        format!(
                            "unconnected output {}.{} requires explicit discard_outputs",
                            self.compiled.processes()[index].path(),
                            emission.output
                        ),
                    );
                    return Ok(());
                }
                let targets: Vec<_> = indices.iter().map(|q| FanoutTarget::identity(*q)).collect();
                let mut packet = Packet::new(emission.value);
                packet.origin = self.compiled.processes()[index].origin().cloned();
                if let Some(lease) = &self.lease {
                    packet = packet.with_lease(lease.clone());
                }
                packet.metadata = Some(Arc::new(CaptureMetadata {
                    session: self.session_id,
                    generation: self.generation,
                    process: Some(index),
                    output: emission.output,
                    activation: ticket.activation(),
                    request: self.processes[index].request,
                    capture: None,
                }));
                match self.transport.prepare(&packet, &targets) {
                    Ok(mut transaction) => {
                        for queue in &indices {
                            if let QueueEndpoint::Capture(name) = &self.endpoints[*queue].1 {
                                transaction.stamp_capture(*queue, name);
                            }
                        }
                        self.processes[index].pending = Some(transaction);
                        self.processes[index].pending_targets = indices;
                    }
                    Err(error) => self.fail_process(index, error.to_string()),
                }
            }
            Ok(Step::WaitInput(name)) => {
                if !self.processes[index].inputs.iter().any(|p| p.name == name) {
                    self.fail_process(index, format!("unknown WaitInput port: {name}"));
                } else {
                    // A staged input may later become request-locally closed
                    // when an upstream activation ends without a queue event.
                    self.always_poll.insert(index);
                    self.processes[index].waiting = Some(name);
                }
            }
            Ok(Step::WaitInputs(policy)) => {
                // Custom or group-closure readiness may progress without a
                // physical packet; keep this process polled on every drive.
                self.always_poll.insert(index);
                self.processes[index].waiting_policy = Some(policy);
            }
            Ok(Step::WaitWake) => {}
            Ok(Step::Finished) => self.finish(index),
            Ok(Step::Complete) => {
                self.processes[index].request = None;
            }
            Ok(Step::Yield) => {}
            Err(error) => self.fail_process(index, format!("process {index}: {error}")),
        }
        self.dirty_process(index);
        self.refresh();
        Ok(())
    }
    pub fn reject(
        &mut self,
        index: usize,
        job: ActivationJob,
    ) -> Result<(), (HandoffError, ActivationJob)> {
        self.supervise_configured_deferred();
        let Some(p) = self.processes.get_mut(index) else {
            return Err((HandoffError::ForeignCompletion, job));
        };
        p.handoff.reject(job)?;
        self.dispatched_at.remove(&index);
        self.dirty_process(index);
        self.refresh();
        Ok(())
    }
    pub fn complete_job(
        &mut self,
        completion: CompletedStep,
    ) -> Result<(), (HandoffError, Box<CompletedStep>)> {
        self.supervise_configured_deferred();
        let Some(index) = self
            .processes
            .iter()
            .position(|p| p.handoff.owns(completion.ticket()))
        else {
            return Err((HandoffError::ForeignCompletion, Box::new(completion)));
        };
        self.complete(index, completion)
    }
    fn publish(&mut self, index: usize) {
        let transaction = self.processes[index].pending.as_mut().unwrap();
        match self.transport.commit(transaction) {
            Ok(()) => {
                // The atomic commit can make consumers runnable and can
                // resume this publisher's continuation.
                let touched = self.processes[index].pending_targets.clone();
                for queue in touched {
                    self.dirty_queue(queue);
                }
                self.dirty_process(index);
                if self.processes[index].pending_targets.is_empty() {
                    self.discarded_emissions = self.discarded_emissions.saturating_add(1);
                }
                self.processes[index].pending = None;
                self.processes[index].pending_targets.clear();
                self.processes[index]
                    .handoff
                    .publication_committed()
                    .unwrap();
                self.publications = self.publications.saturating_add(1);
            }
            Err(ConnectionError::Full) => {}
            Err(error) => self.fail_process(index, format!("process {index} publication: {error}")),
        }
    }
    /// The budget counts process steps; publication retries do not execute code
    /// and are bounded by the number of retained per-process transactions.
    /// Both drivers stop at quiescence or external backpressure. Neither spins
    /// waiting for an ingress, capture consumer, worker completion, or wake.
    pub fn run_until_idle(&mut self, budget: usize) -> RunReport {
        self.supervise_configured_deferred();
        let before = self.publications;
        let mut steps = 0;
        while steps < budget {
            self.supervise_configured_deferred();
            let Some((index, job)) = self.next_job_inner() else {
                break;
            };
            let ticket = job.ticket();
            steps += 1;
            let dispatched = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.dispatcher.dispatch(job)
            }));
            self.supervise_configured_deferred();
            let dispatched = match dispatched {
                Ok(dispatched) => dispatched,
                Err(payload) => {
                    crate::discard_panic_payload(payload);
                    if self.processes[index].handoff.outstanding() == Some(ticket) {
                        self.fail_process(index, "process dispatcher panicked".into());
                    }
                    break;
                }
            };
            match dispatched {
                DispatchResult::Completed(completion) => {
                    if let Err((error, completion)) = self.complete(index, completion) {
                        let ticket = completion.ticket();
                        if self.timeout_invalidated(ticket) || self.terminal.is_some() {
                            let request =
                                completion.metadata().and_then(|metadata| metadata.request);
                            if completion.dispose_catching() {
                                self.record_late_return_cleanup_panic(request);
                            }
                            break;
                        }
                        self.dispatch_return =
                            Some(DispatchReturn::Completed { error, completion });
                        self.fail(format!("process dispatcher returned an invalid completion for process {index}: {error}"));
                        break;
                    }
                }
                DispatchResult::Deferred => {
                    if self.deferred_timeout.is_some()
                        && self.processes[index].handoff.outstanding() == Some(ticket)
                    {
                        // Only externally owned jobs are supervised. The
                        // synchronous dispatch above has already returned.
                        self.dispatched_at.insert(index, Instant::now());
                    }
                }
                DispatchResult::Rejected(job) => {
                    if let Err((error, job)) = self.reject(index, job) {
                        let ticket = job.ticket();
                        if self.timeout_invalidated(ticket) || self.terminal.is_some() {
                            let request = job.metadata().and_then(|metadata| metadata.request);
                            if job.dispose_catching() {
                                self.record_late_return_cleanup_panic(request);
                            }
                            break;
                        }
                        self.dispatch_return = Some(DispatchReturn::Rejected { error, job });
                        self.fail(format!("process dispatcher returned an invalid rejected job for process {index}: {error}"));
                    }
                    break;
                }
            }
        }
        self.supervise_configured_deferred();
        let state = self.state();
        RunReport {
            state,
            steps,
            publications: self.publications - before,
            disposed_packets: self.disposed_packets,
            discarded_emissions: self.discarded_emissions,
            budget_exhausted: steps == budget && state == SessionState::Runnable,
            next_deferred_timeout: self.next_deferred_timeout(),
        }
    }
}
