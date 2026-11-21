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

    fn record_late_return_cleanup_panic(
        &mut self,
        ticket: StepTicket,
        metadata: Option<ActivationMetadata>,
    ) {
        let request = metadata.as_ref().and_then(|metadata| metadata.request);
        let mut context = crate::FailureContext {
            ticket: Some(ticket),
            phase: Some(CallbackPhase::Cleanup),
            ..Default::default()
        };
        if let Some(metadata) = metadata {
            context.session = Some(metadata.session);
            context.generation = Some(metadata.generation);
            context.request = metadata.request;
            context.process = Some(metadata.process);
            context.path = Some(metadata.path);
            context.output = metadata.output;
        }
        let mut secondary = RuntimeError::DestructorPanicked {
            stage: CleanupStage::LateReturn,
        }
        .failure()
        .with_context(context);
        if let Some(lease) = &self.lease {
            secondary = secondary.retain_leases(std::slice::from_ref(lease));
        }
        self.append_failure_cleanup(request, secondary);
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
            let failure = RuntimeError::DeferredTimeout {
                process: index,
                path: self.compiled.processes()[index].path().into(),
                request,
                ticket: job.ticket,
                timeout: self.deferred_timeout.expect("supervised timeout"),
            }
            .failure()
            .with_context(crate::FailureContext {
                ticket: Some(job.ticket),
                ..Default::default()
            });
            self.fail_process(index, failure);
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
    /// Translate one owned emission into a prepared transport transaction.
    /// False means validation has already recorded a process failure and
    /// the completion must retain its historical early-return behavior.
    fn stage_completed_emission(
        &mut self,
        index: usize,
        ticket: StepTicket,
        emission: crate::process::ProcessEmission,
    ) -> bool {
        let process = &self.compiled.processes()[index];
        let actual = emission.value.identity();
        let error = match process.interface().output(&emission.output) {
            None => Some(
                RuntimeError::UnknownOutput {
                    process: index,
                    path: process.path().into(),
                    output: emission.output.clone(),
                }
                .failure(),
            ),
            Some(port) if port.value != actual => Some(
                RuntimeError::EmissionType {
                    process: index,
                    path: process.path().into(),
                    output: emission.output.clone(),
                    expected: port.value.clone(),
                    actual,
                }
                .failure(),
            ),
            Some(_) => None,
        };
        if let Some(error) = error {
            self.fail_emission(index, ticket, emission, error);
            return false;
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
            let selection = (|| -> Result<Vec<usize>, Failure> {
                let budget = outgoing
                    .iter()
                    .filter(|q| {
                        matches!(self.endpoints[**q].1, QueueEndpoint::Process { .. })
                    })
                    .count();
                // Reject oversized process-produced lists before allocating
                // routing maps, copying targets, or preparing any payload.
                if destinations.len() > budget {
                    return Err(RuntimeError::SelectionBudget {
                        budget,
                        actual: destinations.len(),
                    }
                    .failure());
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
                        RuntimeError::UnknownSelection { position }.failure()
                    })?;
                    if resolved.is_empty() {
                        return Err(RuntimeError::EmptySelection { position }.failure());
                    }
                    for (target, input) in resolved {
                        let queue =
                            connected.get(&(*target, input.as_str())).ok_or_else(|| {
                                RuntimeError::UnconnectedSelection {
                                    path: self.compiled.processes()[*target].path().into(),
                                    input: input.clone(),
                                }
                                .failure()
                            })?;
                        if !selected.insert(*queue) {
                            return Err(RuntimeError::DuplicateSelection {
                                path: self.compiled.processes()[*target].path().into(),
                                input: input.clone(),
                            }
                            .failure());
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
                    self.fail_emission(index, ticket, emission, error);
                    return false;
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
            let error = RuntimeError::UnconnectedOutput {
                path: self.compiled.processes()[index].path().into(),
                output: emission.output.clone(),
            }
            .failure();
            self.fail_emission(index, ticket, emission, error);
            return false;
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
            output: emission.output.clone(),
            activation: ticket.activation(),
            request: self.processes[index].request,
            capture: None,
        }));
        let prepared = self.transport.prepare(&packet, &targets).map_err(|error| {
            error
                .into_failure()
                .retain_leases(packet.value().known_leases())
        });
        let cleanup = packet.into_value().dispose_with_cleanup(|| {
            self.enrich_failure(
                Some(index),
                RuntimeError::DestructorPanicked {
                    stage: CleanupStage::Failure,
                }
                .failure()
                .with_context(crate::FailureContext {
                    ticket: Some(ticket),
                    output: Some(emission.output.clone()),
                    phase: Some(CallbackPhase::Cleanup),
                    ..Default::default()
                }),
            )
        });
        match prepared {
            Ok(mut transaction) => {
                for queue in &indices {
                    if let QueueEndpoint::Capture(name) = &self.endpoints[*queue].1 {
                        transaction.stamp_capture(*queue, name);
                    }
                }
                self.processes[index].pending = Some(transaction);
                self.processes[index].pending_targets = indices;
                let mut cleanup = cleanup.into_iter();
                if let Some(failure) = cleanup.next() {
                    self.fail_process(index, cleanup.fold(failure, Failure::with_cleanup));
                }
            }
            Err(mut failure) => {
                let mut context = failure.context().clone();
                context.ticket = Some(ticket);
                context.output = Some(emission.output);
                failure = failure.with_context(context);
                for secondary in cleanup {
                    failure = failure.with_cleanup(secondary);
                }
                self.fail_process(index, failure);
            }
        }
        true
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
                if !self.stage_completed_emission(index, ticket, emission) {
                    return Ok(());
                }
            }
            Ok(Step::WaitInput(name)) => {
                if !self.processes[index].inputs.iter().any(|p| p.name == name) {
                    self.fail_process(
                        index,
                        RuntimeError::UnknownWaitInput { port: name }.failure(),
                    );
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
            Err(error) => self.fail_process(index, error),
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
        let cleanup = p.handoff.reject(job)?;
        self.dispatched_at.remove(&index);
        if let Err(failure) = cleanup {
            self.fail_process(index, failure);
        }
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
            Err(error) => self.fail_process(
                index,
                Failure::external("process publication failed", error)
                    .with_kind(FailureKind::Framework),
            ),
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
                    let failure =
                        crate::process::callback_panicked(CallbackPhase::Dispatch, payload);
                    if self.processes[index].handoff.outstanding() == Some(ticket) {
                        self.fail_process(index, failure);
                    }
                    break;
                }
            };
            match dispatched {
                DispatchResult::Completed(completion) => {
                    if let Err((error, completion)) = self.complete(index, completion) {
                        let ticket = completion.ticket();
                        if self.timeout_invalidated(ticket) || self.terminal.is_some() {
                            let metadata = completion.metadata().cloned();
                            if completion.dispose_catching() {
                                self.record_late_return_cleanup_panic(ticket, metadata);
                            }
                            break;
                        }
                        self.dispatch_return =
                            Some(DispatchReturn::Completed { error, completion });
                        self.fail(
                            self.enrich_failure(
                                Some(index),
                                Failure::external(
                                    "process dispatcher returned an invalid completion",
                                    error,
                                )
                                .with_kind(FailureKind::Framework),
                            ),
                        );
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
                            let metadata = job.metadata().cloned();
                            if job.dispose_catching() {
                                self.record_late_return_cleanup_panic(ticket, metadata);
                            }
                            break;
                        }
                        self.dispatch_return = Some(DispatchReturn::Rejected { error, job });
                        self.fail(
                            self.enrich_failure(
                                Some(index),
                                Failure::external(
                                    "process dispatcher returned an invalid rejected job",
                                    error,
                                )
                                .with_kind(FailureKind::Framework),
                            ),
                        );
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

    fn fail_emission(
        &mut self,
        index: usize,
        ticket: StepTicket,
        emission: crate::ProcessEmission,
        failure: Failure,
    ) {
        let leases = emission.value.known_leases().to_vec();
        let mut context = failure.context().clone();
        context.output = Some(emission.output);
        context.ticket = Some(ticket);
        let mut failure = failure.with_context(context).retain_leases(&leases);
        if emission.value.dispose_catching() {
            let secondary = self.enrich_failure(
                Some(index),
                RuntimeError::DestructorPanicked {
                    stage: CleanupStage::Failure,
                }
                .failure()
                .with_context(crate::FailureContext {
                    ticket: Some(ticket),
                    output: failure.context().output.clone(),
                    phase: Some(CallbackPhase::Cleanup),
                    ..Default::default()
                })
                .retain_leases(&leases),
            );
            failure = failure.with_cleanup(secondary);
        }
        self.fail_process(index, failure);
    }
}
