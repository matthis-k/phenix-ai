use super::*;

impl NetworkSession {
    /// Dispose only this scope's work. With active processes the default policy
    /// fails the session; RestartAffectedProcess explicitly opts into fresh state.
    /// Returned tickets remain runner inventory: cancellation cannot undo effects
    /// or preempt a handed job. Invalidated jobs retain their own code leases.
    pub fn cancel_request(&mut self, id: RequestId) -> Result<Vec<StepTicket>, RequestError> {
        // Explicit cancellation wins if it is serialized before timeout supervision.
        self.terminate_request(id, RequestStatus::Cancelled, None)
    }
    /// Explicit host failure uses the same disposal/recovery policy as step
    /// errors. The first terminal transition wins; no external effects undo.
    pub fn fail_request(
        &mut self,
        id: RequestId,
        diagnostic: Failure,
    ) -> Result<Vec<StepTicket>, RequestError> {
        self.supervise_configured_deferred();
        self.fail_request_inner(id, diagnostic)
    }

    fn fail_request_inner(
        &mut self,
        id: RequestId,
        diagnostic: Failure,
    ) -> Result<Vec<StepTicket>, RequestError> {
        self.terminate_request(id, RequestStatus::Failed, Some(diagnostic))
    }

    pub fn request_failure(&self, id: RequestId) -> Result<Option<&Failure>, RequestError> {
        let scope = self.check_request(id)?;
        Ok(scope.failure.as_ref().or_else(|| {
            if self.terminal == Some(SessionState::Failed)
                && (scope.terminal.is_none() || scope.inherited_terminal)
            {
                self.failure()
            } else {
                None
            }
        }))
    }

    pub fn request_disposal(&self, id: RequestId) -> Result<RequestDisposal, RequestError> {
        Ok(self.check_request(id)?.disposal)
    }

    /// Also exposes inventory from automatic request-local failure recovery.
    /// Snapshot this before release_request removes the bounded scope record.
    pub fn request_cancelled_jobs(&self, id: RequestId) -> Result<&[StepTicket], RequestError> {
        Ok(&self.check_request(id)?.cancelled_jobs)
    }

    pub(super) fn enrich_failure(&self, index: Option<usize>, error: Failure) -> Failure {
        let mut context = error.context().clone();
        context.session = Some(self.session_id);
        context.generation = Some(self.generation);
        if let Some(index) = index {
            context.process = Some(index);
            context.path = Some(self.compiled.processes()[index].path().into());
            context.request = self.processes[index].request;
        }
        let mut error = error.with_context(context);
        if let Some(lease) = &self.lease {
            error = error.retain_leases(std::slice::from_ref(lease));
        }
        error
    }

    pub(super) fn fail_process(&mut self, index: usize, error: Failure) {
        let error = self.enrich_failure(Some(index), error);
        if let Some(request) = self.processes[index].request {
            // Recovery failure has already made the whole session terminal.
            let _ = self.fail_request_inner(request, error);
        } else {
            self.fail(error);
        }
    }

    pub(super) fn append_failure_cleanup(
        &mut self,
        request: Option<RequestId>,
        secondary: Failure,
    ) -> bool {
        let primary = request
            .and_then(|id| self.requests.get(&id))
            .and_then(|scope| scope.failure.as_ref())
            .or(self.failure.as_ref())
            .cloned();
        let Some(primary) = primary else {
            return false;
        };
        if let Some(failure) = request
            .and_then(|id| self.requests.get_mut(&id))
            .and_then(|scope| scope.failure.as_mut())
        {
            *failure = failure.clone().with_cleanup(secondary.clone());
        }
        if let Some(failure) = self.failure.as_mut().filter(|failure| {
            failure.same_origin(&primary) && failure.context().request == primary.context().request
        }) {
            *failure = failure.clone().with_cleanup(secondary);
            // Inherited cleanup can arrive through a peer request after escalation.
            if let Some(request_failure) = failure
                .context()
                .request
                .and_then(|id| self.requests.get_mut(&id))
                .and_then(|scope| scope.failure.as_mut())
                .filter(|request_failure| request_failure.same_origin(failure))
            {
                *request_failure = failure.clone();
            }
        }
        true
    }

    fn terminate_request(
        &mut self,
        id: RequestId,
        status: RequestStatus,
        error: Option<Failure>,
    ) -> Result<Vec<StepTicket>, RequestError> {
        let scope = self.check_request(id)?;
        if scope.terminal.is_some() {
            return Ok(scope.cancelled_jobs.clone());
        }
        // Inherited termination fixes the outcome, not ownership disposal. A
        // late caller may release its own retained work but cannot replace cause.
        let inherited = self.inherited_request_terminal();
        let status = inherited.unwrap_or(status);
        let error = if inherited.is_some() { None } else { error };
        // Request termination clears reachability below; detach only watches
        // that were registered by an earlier close_request before clearing it.
        if scope.sealed {
            let reachable: Vec<_> = scope.reachable.iter().copied().collect();
            self.unwatch_scoped_readiness(&reachable);
        }
        let error = error.map(|error| {
            let mut context = error.context().clone();
            context.request = Some(id);
            self.enrich_failure(None, error.with_context(context))
        });
        let scope = self.requests.get_mut(&id).unwrap();
        scope.sealed = true;
        scope.terminal = Some(status);
        scope.inherited_terminal = inherited.is_some();
        scope.failure = error;
        scope.reachable.clear();
        scope.observed_inputs.clear();

        let affected: BTreeSet<_> = self
            .processes
            .iter()
            .enumerate()
            .filter(|(_, p)| p.request == Some(id))
            .map(|(index, _)| index)
            .collect();
        let disposed_packets = self.transport.detach_request(id);
        let disposed = disposed_packets.len();
        self.disposed_packets = self.disposed_packets.saturating_add(disposed);
        let scope = self.requests.get_mut(&id).unwrap();
        scope.disposal.queued_packets = scope.disposal.queued_packets.saturating_add(disposed);
        self.ready.retain(|index| !affected.contains(index));
        let mut cancelled_processes = Vec::with_capacity(affected.len());
        let mut pending_disposals = Vec::new();
        let mut recovery_tickets = BTreeMap::new();
        for index in &affected {
            self.dispatched_at.remove(index);
            let retired_wake = {
                let p = &mut self.processes[*index];
                let scope = self.requests.get_mut(&id).unwrap();
                if let Some(ticket) = p.handoff.outstanding().or_else(|| {
                    scope
                        .failure
                        .as_ref()
                        .filter(|failure| failure.context().process == Some(*index))
                        .and_then(|failure| failure.context().ticket)
                }) {
                    recovery_tickets.insert(*index, ticket);
                }
                scope.disposal.activations += 1;
                if let Some(prepared) = p.pending.take() {
                    scope.disposal.prepared_packets += prepared.retained_packets();
                    pending_disposals.push(prepared);
                }
                let cancelled = p.handoff.cancel_detach();
                if let Some(ticket) = cancelled.ticket() {
                    scope.cancelled_jobs.push(ticket);
                    scope.disposal.handed_jobs += 1;
                }
                cancelled_processes.push(cancelled);
                p.pending_targets.clear();
                p.request = None;
                p.waiting = None;
                p.waiting_policy = None;
                p.closed_in_activation.clear();
                p.queued = false;
                p.wake.clone()
            };
            self.sync_poll(*index);
            // A notifier for the old request may race with recovery. Retire
            // under the same lock it uses to publish a process target, before
            // a replacement slot can be installed for this compiled index.
            self.signal.retire_target(*index, &retired_wake);
        }
        let mut process_destructor_panicked = false;
        for cancelled in cancelled_processes {
            process_destructor_panicked |= cancelled.dispose().is_err();
        }
        let pending_destructor_panicked = drop_prepared_catching(pending_disposals);
        let packet_destructor_panicked = drop_packets_catching(disposed_packets);
        // Attach local cleanup before escalation; escaped clones stay immutable.
        let mut cleanup = Vec::new();
        for (panicked, error) in [
            (
                process_destructor_panicked,
                RequestError::ProcessDestructorPanicked,
            ),
            (
                pending_destructor_panicked || packet_destructor_panicked,
                RequestError::PacketDestructorPanicked,
            ),
        ] {
            if panicked {
                let secondary = self.enrich_failure(
                    None,
                    Failure::external("destructor panicked during request cleanup", error)
                        .with_kind(FailureKind::Cleanup)
                        .with_context(crate::FailureContext {
                            request: Some(id),
                            phase: Some(CallbackPhase::Cleanup),
                            ..Default::default()
                        }),
                );
                if !self.append_failure_cleanup(Some(id), secondary.clone()) {
                    cleanup.push(secondary);
                }
            }
        }
        let mut recovery_failed = false;
        if !affected.is_empty() && self.terminal.is_none() {
            if self.request_failure_policy == RequestFailurePolicy::FailSession {
                let request_failed = self.requests[&id].failure.is_some();
                let diagnostic = self.requests[&id].failure.clone().unwrap_or_else(|| {
                    let mut failure = RuntimeError::ActiveRequestCancelled.failure();
                    for secondary in cleanup {
                        failure = failure.with_cleanup(secondary);
                    }
                    failure
                });
                self.fail(diagnostic);
                if request_failed {
                    // Session stop may append peer cleanup to this same primary.
                    self.requests.get_mut(&id).unwrap().failure = self.failure.clone();
                }
            } else {
                for index in affected {
                    let spawned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        self.compiled.processes()[index].factory().spawn()
                    }));
                    let process = match spawned {
                        Ok(process) => process,
                        Err(payload) => {
                            let diagnostic = self.enrich_failure(
                                Some(index),
                                crate::process::callback_panicked(
                                    CallbackPhase::RecoveryConstruct,
                                    payload,
                                ),
                            );
                            let mut context = diagnostic.context().clone();
                            context.request = Some(id);
                            context.ticket = recovery_tickets.get(&index).copied();
                            let mut diagnostic = diagnostic.with_context(context);
                            for secondary in cleanup {
                                diagnostic = diagnostic.with_cleanup(secondary);
                            }
                            let scope = self.requests.get_mut(&id).unwrap();
                            let failure = match scope.failure.take() {
                                Some(primary) => primary.with_cleanup(diagnostic),
                                None => diagnostic,
                            };
                            self.fail(failure);
                            self.requests.get_mut(&id).unwrap().failure = self.failure.clone();
                            recovery_failed = true;
                            break;
                        }
                    };
                    let mut handoff = self.processes[index].handoff.replacement(process);
                    if let Some(lease) = &self.lease {
                        handoff = handoff.with_lease(lease.clone());
                    }
                    let p = &mut self.processes[index];
                    p.handoff = handoff;
                    p.wake = Arc::new(WakeSlot::default());
                    p.done = false;
                    p.cursor = 0;
                    self.request_cursors.remove(&index);
                    for port in &mut p.inputs {
                        port.cursor = 0;
                        port.eos_delivered = false;
                    }
                }
            }
        }
        let tickets = self.requests[&id].cancelled_jobs.clone();
        // Queue disposal and process replacement can affect arbitrary
        // predecessors, so never rely on the packet-only dirty set here.
        self.force_full_refresh = true;
        self.refresh();
        self.notifier().notify();
        if recovery_failed {
            return Err(RequestError::RecoveryFailed);
        }
        if process_destructor_panicked {
            return Err(RequestError::ProcessDestructorPanicked);
        }
        if pending_destructor_panicked || packet_destructor_panicked {
            return Err(RequestError::PacketDestructorPanicked);
        }
        Ok(tickets)
    }
}

impl NetworkSession {
    pub(super) fn finish(&mut self, index: usize) {
        if self.processes[index]
            .inputs
            .iter()
            .flat_map(|p| &p.queues)
            .any(|q| !self.transport.queues()[*q].is_empty())
        {
            self.fail_input(index, InputFailureReason::Abandoned);
            return;
        }
        let mut destructor_panicked = false;
        let (writers, readers, retired_wake) = {
            let p = &mut self.processes[index];
            if p.handoff.state() == HandoffState::Idle {
                destructor_panicked = p.handoff.finish_idle().is_err();
            }
            p.done = true;
            p.request = None;
            (
                p.outputs.values().flatten().copied().collect::<Vec<_>>(),
                p.inputs
                    .iter()
                    .flat_map(|port| port.queues.iter().copied())
                    .collect::<Vec<_>>(),
                p.wake.clone(),
            )
        };
        self.sync_poll(index);
        // Retire before transport closure can dispatch unrelated successors.
        // The notifier check and target removal share the WakeSignal lock.
        self.signal.retire_target(index, &retired_wake);
        // The queue itself remains authoritative for EOS and retained
        // packets. Both endpoints change scheduling conditions on writer
        // closure or reader disposal, including upstream pending fanout.
        for queue in writers {
            self.transport.close_writer(queue).unwrap();
            self.dirty_queue(queue);
        }
        let lane_writers: Vec<_> = self
            .request_lanes
            .values()
            .flat_map(|lane| lane.outputs.iter())
            .filter(|((producer, _), _)| *producer == index)
            .flat_map(|(_, queues)| queues.iter().copied())
            .collect();
        for queue in lane_writers {
            self.transport.close_writer(queue).unwrap();
            self.dirty_queue(queue);
        }
        for queue in readers {
            let packets = self.transport.close_reader(queue).unwrap();
            debug_assert!(packets.is_empty());
            self.dirty_queue(queue);
        }
        if destructor_panicked {
            self.fail_process(
                index,
                RuntimeError::DestructorPanicked {
                    stage: CleanupStage::Finish,
                }
                .failure(),
            );
        }
    }
    pub(super) fn fail(&mut self, error: Failure) {
        if self.terminal.is_some() {
            return;
        }
        let error = self.enrich_failure(None, error);
        // Failure is not rollback: retain committed captures for host draining.
        let (cancelled, pending) = self.stop_processes();
        let mut disposed = Vec::new();
        for index in 0..self.transport.queues().len() {
            self.transport.close_writer(index).unwrap();
            if !matches!(self.endpoints[index].1, QueueEndpoint::Capture(_)) {
                for packet in self.transport.queues()[index].packets() {
                    if let Some(request) = packet.metadata().and_then(|m| m.request())
                        && let Some(scope) = self.requests.get_mut(&request)
                    {
                        scope.disposal.queued_packets =
                            scope.disposal.queued_packets.saturating_add(1);
                    }
                }
                let packets = self.transport.close_reader(index).unwrap();
                self.disposed_packets = self.disposed_packets.saturating_add(packets.len());
                disposed.extend(packets);
            }
        }
        self.failure = Some(error);
        self.terminal = Some(SessionState::Failed);
        let mut destructor_panicked = false;
        for process in cancelled {
            destructor_panicked |= process.dispose().is_err();
        }
        destructor_panicked |= drop_prepared_catching(pending);
        destructor_panicked |= drop_packets_catching(disposed);
        if destructor_panicked {
            let secondary = self.enrich_failure(
                None,
                RuntimeError::DestructorPanicked {
                    stage: CleanupStage::Failure,
                }
                .failure(),
            );
            self.failure = self
                .failure
                .take()
                .map(|failure| failure.with_cleanup(secondary));
        }
    }
    fn stop_processes(&mut self) -> (Vec<CancelledProcess>, Vec<PreparedFanout>) {
        self.dispatched_at.clear();
        let mut cancelled = Vec::new();
        let mut pending = Vec::new();
        for p in &mut self.processes {
            let request = p.request;
            let retired = p.handoff.cancel_detach();
            let ticket = retired.ticket();
            if let Some(request) = p.request
                && let Some(scope) = self.requests.get_mut(&request)
            {
                scope.disposal.activations += 1;
                if let Some(ticket) = ticket {
                    scope.cancelled_jobs.push(ticket);
                    scope.disposal.handed_jobs += 1;
                }
            }
            if let Some(prepared) = p.pending.take() {
                if let Some(request) = request
                    && let Some(scope) = self.requests.get_mut(&request)
                {
                    scope.disposal.prepared_packets += prepared.retained_packets();
                }
                pending.push(prepared);
            }
            cancelled.push(retired);
            p.pending_targets.clear();
            p.wake.active.store(false, Ordering::Release);
            p.request = None;
        }
        self.ready.clear();
        self.signal.invalidate();
        (cancelled, pending)
    }
    pub fn cancel(&mut self) {
        // A terminal result is final. In particular, do not turn a failure
        // into cancellation or dispose captures intentionally retained by fail.
        if self.terminal.is_some() || self.is_completed() {
            return;
        }
        let (cancelled, pending) = self.stop_processes();
        for queue in self.transport.queues() {
            for packet in queue.packets() {
                if let Some(request) = packet.metadata().and_then(|m| m.request())
                    && let Some(scope) = self.requests.get_mut(&request)
                {
                    scope.disposal.queued_packets = scope.disposal.queued_packets.saturating_add(1);
                }
            }
        }
        let packets = self.transport.cancel();
        self.disposed_packets = self.disposed_packets.saturating_add(packets.len());
        self.terminal = Some(SessionState::Cancelled);
        let mut destructor_panicked = false;
        for process in cancelled {
            destructor_panicked |= process.dispose().is_err();
        }
        destructor_panicked |= drop_prepared_catching(pending);
        destructor_panicked |= drop_packets_catching(packets);
        if destructor_panicked {
            self.failure = Some(
                self.enrich_failure(
                    None,
                    RuntimeError::DestructorPanicked {
                        stage: CleanupStage::Cancellation,
                    }
                    .failure(),
                ),
            );
        }
    }
}
