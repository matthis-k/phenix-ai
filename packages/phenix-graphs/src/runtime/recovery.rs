use super::*;

impl NetworkSession {
    /// Dispose only this scope's work. With active processes the default policy
    /// fails the session; RestartAffectedProcess explicitly opts into fresh state.
    /// Returned tickets remain runner inventory: cancellation cannot undo effects
    /// or preempt a handed job. Invalidated jobs retain their own code leases.
    pub fn cancel_request(&mut self, id: RequestId) -> Result<Vec<StepTicket>, RequestError> {
        self.terminate_request(id, RequestStatus::Cancelled, None)
    }
    /// Explicit host failure uses the same disposal/recovery policy as step
    /// errors. The first terminal transition wins; no external effects undo.
    pub fn fail_request(
        &mut self,
        id: RequestId,
        diagnostic: impl Into<String>,
    ) -> Result<Vec<StepTicket>, RequestError> {
        self.terminate_request(id, RequestStatus::Failed, Some(diagnostic.into()))
    }

    pub fn request_failure(&self, id: RequestId) -> Result<Option<&str>, RequestError> {
        let scope = self.check_request(id)?;
        Ok(scope.failure.as_deref().or_else(|| {
            if self.terminal == Some(SessionState::Failed) {
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

    pub(super) fn fail_process(&mut self, index: usize, error: String) {
        if let Some(request) = self.processes[index].request {
            // Recovery failure has already made the whole session terminal.
            let _ = self.fail_request(request, error);
        } else {
            self.fail(error);
        }
    }

    fn terminate_request(
        &mut self,
        id: RequestId,
        status: RequestStatus,
        error: Option<String>,
    ) -> Result<Vec<StepTicket>, RequestError> {
        let scope = self.check_request(id)?;
        if scope.terminal.is_some() {
            return Ok(scope.cancelled_jobs.clone());
        }
        // Request termination clears reachability below; detach only watches
        // that were registered by an earlier close_request before clearing it.
        if scope.sealed {
            let reachable: Vec<_> = scope.reachable.iter().copied().collect();
            self.unwatch_scoped_readiness(&reachable);
        }
        let scope = self.requests.get_mut(&id).unwrap();
        scope.sealed = true;
        scope.terminal = Some(status);
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
        let disposed = self.transport.discard_request(id);
        self.disposed_packets = self.disposed_packets.saturating_add(disposed);
        let scope = self.requests.get_mut(&id).unwrap();
        scope.disposal.queued_packets = scope.disposal.queued_packets.saturating_add(disposed);
        self.ready.retain(|index| !affected.contains(index));
        for index in &affected {
            let p = &mut self.processes[*index];
            let scope = self.requests.get_mut(&id).unwrap();
            scope.disposal.activations += 1;
            if let Some(pending) = p.pending.take() {
                scope.disposal.prepared_packets += pending.retained_packets();
            }
            if let Some(ticket) = p.handoff.cancel() {
                scope.cancelled_jobs.push(ticket);
                scope.disposal.handed_jobs += 1;
            }
            p.pending_targets.clear();
            p.wake.active.store(false, Ordering::Release);
            p.wake.pending.store(false, Ordering::Release);
            p.request = None;
            p.waiting = None;
            p.waiting_policy = None;
            p.closed_in_activation.clear();
            p.queued = false;
        }
        if !affected.is_empty() && self.terminal.is_none() {
            if self.request_failure_policy == RequestFailurePolicy::FailSession {
                let diagnostic = self.requests[&id].failure.clone().unwrap_or_else(|| {
                    "active request cancelled without RestartAffectedProcess policy".into()
                });
                self.fail(diagnostic);
            } else {
                for index in affected {
                    let spawned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        self.compiled.processes()[index].factory().spawn()
                    }));
                    let process = match spawned {
                        Ok(process) => process,
                        Err(_) => {
                            let diagnostic = format!(
                                "process factory panicked during request recovery: {}",
                                self.compiled.processes()[index].path()
                            );
                            self.requests.get_mut(&id).unwrap().failure = Some(diagnostic.clone());
                            self.fail(diagnostic);
                            return Err(RequestError::RecoveryFailed);
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
        Ok(tickets)
    }
}
