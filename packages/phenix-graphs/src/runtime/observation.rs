//! Passive snapshots of committed session state and supplied-time job deadlines.
use super::*;

impl NetworkSession {
    pub fn output_closed(&self, name: &str) -> Result<bool, ConnectionError> {
        let index = *self
            .captures
            .get(name)
            .ok_or(ConnectionError::InvalidDestination)?;
        Ok(std::iter::once(index)
            .chain(
                self.request_lanes
                    .values()
                    .filter_map(|lane| lane.captures.get(name).copied()),
            )
            .all(|index| {
                self.transport.queues()[index].is_eos()
                    || self.transport.queues()[index].reader_closed()
            }))
    }
    pub fn process_origin(&self, index: usize) -> Option<&GraphOrigin> {
        self.compiled.processes().get(index)?.origin()
    }
    pub fn failure(&self) -> Option<&Failure> {
        self.failure.as_ref()
    }
    pub fn disposed_packets(&self) -> usize {
        self.disposed_packets
    }
    pub fn discarded_emissions(&self) -> usize {
        self.discarded_emissions
    }
    /// Bounded snapshot of the actual input queues at the first input failure.
    pub fn stranded_inputs(&self) -> &[QueueWait] {
        &self.input_failure
    }
    /// Only the last accepted step is retained, not an execution history.
    pub fn last_step(&self) -> Option<&ActivationMetadata> {
        self.last_step.as_ref()
    }

    /// Passive inspection of outstanding jobs whose configured age limit
    /// elapsed. Never mutates a job, invokes a worker or reads wall-clock time.
    pub fn overdue_jobs(&self, now: Instant) -> Vec<OverdueJob> {
        let Some(timeout) = self.deferred_timeout else {
            return Vec::new();
        };
        if self.terminal.is_some() {
            return Vec::new();
        }
        self.dispatched_at
            .iter()
            .filter(|(_, admitted)| now.saturating_duration_since(**admitted) >= timeout)
            .filter_map(|(index, _)| {
                self.processes[*index]
                    .handoff
                    .outstanding()
                    .map(|ticket| OverdueJob {
                        process: *index,
                        ticket,
                    })
            })
            .collect()
    }

    pub(super) fn is_completed(&self) -> bool {
        self.processes.iter().all(|p| p.done)
            && self
                .transport
                .queues()
                .iter()
                .all(ConnectionQueue::is_empty)
            && self
                .ingress
                .values()
                .flatten()
                .all(|q| self.transport.queues()[*q].writer_closed())
    }

    /// Passive snapshot of already committed scheduler state.
    /// A newly delivered wake is scheduled by the next driver operation,
    /// never by a status query; no user readiness policy runs here.
    pub fn state(&self) -> SessionState {
        if let Some(state) = self.terminal {
            return state;
        }
        if !self.ready.is_empty()
            || self.processes.iter().any(|process| {
                process.wake.pending.load(Ordering::Acquire)
                    && (process.handoff.state() == HandoffState::WaitingWake
                        || (process.source && process.handoff.state() == HandoffState::Idle))
            })
        {
            return SessionState::Runnable;
        }
        let drained = self.transport.queues().iter().all(|q| q.is_empty());
        if self.is_completed() {
            return SessionState::Completed;
        }
        if !drained
            || self.requests.iter().any(|(request, scope)| {
                scope.sealed
                    && scope
                        .reachable
                        .iter()
                        .any(|index| self.closure_obligation(*index, *request))
            })
            || self.processes.iter().any(|p| {
                p.pending.is_some()
                    || p.inputs
                        .iter()
                        .flat_map(|p| &p.queues)
                        .any(|q| !self.transport.queues()[*q].is_empty())
                    || matches!(
                        p.handoff.state(),
                        HandoffState::Dispatched
                            | HandoffState::WaitingInput
                            | HandoffState::WaitingWake
                    )
            })
        {
            return SessionState::Waiting;
        }
        SessionState::Quiescent
    }

    /// Actual full-queue publication dependencies, including external captures.
    /// These are diagnostics, not a proof that a cycle can make progress.
    pub fn blocked_queues(&self) -> Vec<QueueWait> {
        self.blocked_queues_matching(None)
    }

    pub(super) fn blocked_queues_matching(&self, request: Option<RequestId>) -> Vec<QueueWait> {
        self.processes
            .iter()
            .filter(|p| {
                p.pending.is_some() && request.is_none_or(|request| p.request == Some(request))
            })
            .flat_map(|p| &p.pending_targets)
            .filter(|q| self.transport.queues()[**q].is_full())
            .map(|q| {
                let queue = &self.transport.queues()[*q];
                QueueWait {
                    queue: *q,
                    producer: self.endpoints[*q].0.clone(),
                    consumer: self.endpoints[*q].1.clone(),
                    occupancy: queue.len(),
                    capacity: queue.capacity(),
                }
            })
            .collect()
    }

    /// Empty upstream queues relevant to idle/read-waiting processes. An open
    /// ingress is an external dependency, not a deadlock or synthetic EOS.
    pub fn input_waits(&self) -> Vec<QueueWait> {
        let live_lanes: BTreeSet<_> =
            if self.request_queue_policy == RequestQueuePolicy::BoundedLanes {
                self.requests.values().map(|scope| scope.lane).collect()
            } else {
                BTreeSet::new()
            };
        self.processes
            .iter()
            .filter(|p| {
                !p.done
                    && matches!(
                        p.handoff.state(),
                        HandoffState::Idle | HandoffState::WaitingInput
                    )
            })
            .flat_map(|p| {
                p.inputs
                    .iter()
                    .filter(|port| p.waiting.as_ref().is_none_or(|name| name == &port.name))
                    .flat_map(|port| &port.queues)
                    .filter(|queue| {
                        if self.request_queue_policy == RequestQueuePolicy::SharedFifo {
                            return true;
                        }
                        let lane = self.queue_lanes[**queue];
                        if p.handoff.state() == HandoffState::WaitingInput {
                            match p.request {
                                Some(request) => lane == Some(self.requests[&request].lane),
                                None => lane.is_none(),
                            }
                        } else {
                            lane.is_none_or(|lane| live_lanes.contains(&lane))
                        }
                    })
            })
            .filter(|q| {
                self.transport.queues()[**q].is_empty() && !self.transport.queues()[**q].is_eos()
            })
            .map(|q| QueueWait {
                queue: *q,
                producer: self.endpoints[*q].0.clone(),
                consumer: self.endpoints[*q].1.clone(),
                occupancy: 0,
                capacity: self.transport.queues()[*q].capacity(),
            })
            .collect()
    }
}
