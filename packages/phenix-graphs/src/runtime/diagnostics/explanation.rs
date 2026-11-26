use super::*;

#[derive(Clone, Debug)]
pub struct SessionObservation {
    pub session: u64,
    pub generation: u64,
    pub state: SessionState,
    pub terminal: Option<SessionState>,
    pub failure: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyExplanation {
    Any,
    All,
    CorrelatedAll,
    AtLeast(usize),
    Optional {
        required: Vec<String>,
        optional: Vec<String>,
    },
    Custom,
}
impl From<&Readiness> for PolicyExplanation {
    fn from(policy: &Readiness) -> Self {
        match &policy.declaration().rule {
            Some(SelectionRule::Any) => Self::Any,
            Some(SelectionRule::All) if policy.correlated() => Self::CorrelatedAll,
            Some(SelectionRule::All) => Self::All,
            Some(SelectionRule::AtLeast(n)) => Self::AtLeast(n.get()),
            Some(SelectionRule::Optional { required, optional }) => Self::Optional {
                required: required.clone(),
                optional: optional.clone(),
            },
            None => Self::Custom,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadinessDeclaration {
    Initial,
    WaitInput(String),
    WaitInputs,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedInput {
    pub port: usize,
    /// None means a closure notification, not a physical packet.
    pub packet: Option<(usize, usize)>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadinessBlock {
    NoCandidate,
    NoAvailableInput,
    MissingRequired {
        port: usize,
    },
    AwaitingOptionalInputOrClosure {
        port: usize,
    },
    Threshold {
        available_ports: usize,
        required_ports: usize,
    },
    NoFreshClosure,
}
/// Readiness only: Available does not mean admitted, scheduled or executable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadyAssessment {
    Available {
        request: Option<RequestId>,
        selected: Vec<SelectedInput>,
    },
    Blocked {
        reasons: Vec<ReadinessBlock>,
    },
    UnknownCustom,
    Invalid {
        reason: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueueHeadExplanation {
    Empty,
    Unscoped,
    /// live_lane is looked up by authenticated ID, never by physical queue lane.
    Request {
        id: RequestId,
        live_lane: Option<usize>,
    },
}
#[derive(Clone, Debug)]
pub struct QueueExplanation {
    pub queue: usize,
    pub lane: Option<usize>,
    pub producer: QueueEndpoint,
    pub consumer: QueueEndpoint,
    pub occupancy: usize,
    pub capacity: usize,
    pub writer_closed: bool,
    pub reader_closed: bool,
    pub head: QueueHeadExplanation,
}
#[derive(Clone, Debug)]
pub struct InputExplanation {
    pub port: usize,
    pub name: String,
    pub cursor: usize,
    pub queues: Vec<QueueExplanation>,
}
#[derive(Clone, Debug)]
pub struct CandidateInputExplanation {
    pub port: usize,
    pub selectable: Option<(usize, usize)>,
    pub closure: ClosureProof,
    pub fresh: bool,
}
#[derive(Clone, Debug)]
pub struct CandidateExplanation {
    pub request: Option<RequestId>,
    pub live_lane: Option<usize>,
    /// False entries are evidence only and cannot win scheduler selection.
    pub scheduler_candidate: bool,
    pub inputs: Vec<CandidateInputExplanation>,
    pub assessment: ReadyAssessment,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WakeExplanation {
    pub active: bool,
    pub pending: bool,
    pub resume_eligible: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobExplanation {
    pub ticket: StepTicket,
    pub dispatched_at: Option<Instant>,
    pub timeout: Option<Duration>,
    pub overdue: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicationAssessment {
    ClosedTargetError {
        queue: usize,
        error: ConnectionError,
    },
    Full,
    CommitEligible,
}
#[derive(Clone, Debug)]
pub struct PublicationExplanation {
    pub retained_packets: usize,
    pub targets: Vec<QueueExplanation>,
    pub assessment: PublicationAssessment,
}
#[derive(Clone, Debug)]
pub struct ProcessExplanation {
    pub process: usize,
    pub path: String,
    pub session: SessionObservation,
    pub handoff: HandoffState,
    pub scheduled: bool,
    pub queued_flag: bool,
    pub done: bool,
    pub source: bool,
    pub owner: Option<RequestId>,
    pub live_lane: Option<usize>,
    pub wake: WakeExplanation,
    pub queue_policy: RequestQueuePolicy,
    pub job: Option<JobExplanation>,
    pub publication: Option<PublicationExplanation>,
    pub declaration: ReadinessDeclaration,
    pub policy: PolicyExplanation,
    pub notification_driven: bool,
    pub cursor: usize,
    pub input_order: Vec<usize>,
    pub inputs: Vec<InputExplanation>,
    pub candidates: Vec<CandidateExplanation>,
    pub readiness: ReadyAssessment,
    pub idle_inputs_cannot_progress: bool,
    /// Only meaningful after a known-empty selection in Idle/WaitingInput.
    pub idle_completion_after_empty_selection: Option<bool>,
    pub stranded_after_empty_selection: Option<bool>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalOrigin {
    Local,
    InheritedSession,
}
#[derive(Clone, Debug)]
pub struct RequestOwnerExplanation {
    pub process: usize,
    pub handoff: HandoffState,
    pub scheduled: bool,
    pub done: bool,
    pub job: Option<JobExplanation>,
    pub publication: Option<PublicationExplanation>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsumerOwnership {
    Unowned,
    ThisRequest,
    OtherRequest(RequestId),
}
#[derive(Clone, Debug)]
pub struct RequestQueueExplanation {
    pub queue: QueueExplanation,
    pub matching_packets: usize,
    pub consumer_ownership: ConsumerOwnership,
}
#[derive(Clone, Debug)]
pub struct ClosureObligationExplanation {
    pub process: usize,
    pub unobserved_ports: Vec<usize>,
}
#[derive(Clone, Debug)]
pub struct RequestExplanation {
    pub request: RequestId,
    pub session: SessionObservation,
    /// Authentication succeeded even when capture validation/count failed.
    pub status: Result<RequestStatus, RequestError>,
    pub sealed: bool,
    pub expected_capture: Option<String>,
    /// Captures consumed/observed by the host, not merely queued captures.
    pub observed_captures: usize,
    pub invalid_capture: bool,
    pub queue_policy: RequestQueuePolicy,
    pub lane: usize,
    pub terminal_origin: Option<TerminalOrigin>,
    pub failure: Option<String>,
    pub work: RequestWork,
    pub disposal: RequestDisposal,
    pub invalidated_tickets: Vec<StepTicket>,
    /// Exactly the owner, indexed queue and obligation terms of request_live.
    pub owners: Vec<RequestOwnerExplanation>,
    pub queues: Vec<RequestQueueExplanation>,
    pub closure_obligations: Vec<ClosureObligationExplanation>,
}

impl NetworkSession {
    fn explanation_session(&self) -> SessionObservation {
        SessionObservation {
            session: self.session_id,
            generation: self.generation,
            state: self.state(),
            terminal: self.terminal,
            failure: self
                .failure
                .as_ref()
                .map(|failure| failure.summary().to_owned()),
        }
    }
    fn explanation_queue(&self, index: usize) -> QueueExplanation {
        let queue = &self.transport.queues()[index];
        let head = match queue.head() {
            None => QueueHeadExplanation::Empty,
            Some(packet) => match packet.metadata().and_then(|m| m.request()) {
                None => QueueHeadExplanation::Unscoped,
                Some(id) => QueueHeadExplanation::Request {
                    id,
                    live_lane: self.requests.get(&id).map(|s| s.lane),
                },
            },
        };
        QueueExplanation {
            queue: index,
            lane: self.queue_lanes.get(index).copied().flatten(),
            producer: self.endpoints[index].0.clone(),
            consumer: self.endpoints[index].1.clone(),
            occupancy: queue.len(),
            capacity: queue.capacity(),
            writer_closed: queue.writer_closed(),
            reader_closed: queue.reader_closed(),
            head,
        }
    }
    fn explanation_job(&self, index: usize, now: Instant) -> Option<JobExplanation> {
        let ticket = self.processes[index].handoff.outstanding()?;
        let dispatched_at = self.dispatched_at.get(&index).copied();
        Some(JobExplanation {
            ticket,
            dispatched_at,
            timeout: self.deferred_timeout,
            overdue: self.terminal.is_none()
                && dispatched_at
                    .zip(self.deferred_timeout)
                    .is_some_and(|(since, timeout)| {
                        now.saturating_duration_since(since) >= timeout
                    }),
        })
    }
    fn explanation_publication(&self, index: usize) -> Option<PublicationExplanation> {
        let p = &self.processes[index];
        let pending = p.pending.as_ref()?;
        let targets: Vec<_> = p
            .pending_targets
            .iter()
            .map(|q| self.explanation_queue(*q))
            .collect();
        let assessment = targets
            .iter()
            .find_map(|q| {
                let error = if q.reader_closed {
                    ConnectionError::ReaderClosed
                } else if q.writer_closed {
                    ConnectionError::WriterClosed
                } else {
                    return None;
                };
                Some(PublicationAssessment::ClosedTargetError {
                    queue: q.queue,
                    error,
                })
            })
            .unwrap_or_else(|| {
                if targets.iter().any(|q| q.occupancy == q.capacity) {
                    PublicationAssessment::Full
                } else {
                    PublicationAssessment::CommitEligible
                }
            });
        Some(PublicationExplanation {
            retained_packets: pending.retained_packets(),
            targets,
            assessment,
        })
    }
    fn explanation_selection(&self, selected: readiness::Reservation) -> ReadyAssessment {
        if selected.ports.is_empty() {
            ReadyAssessment::Blocked {
                reasons: vec![ReadinessBlock::NoFreshClosure],
            }
        } else {
            ReadyAssessment::Available {
                request: selected.request,
                selected: selected
                    .ports
                    .into_iter()
                    .map(|(port, packet)| SelectedInput { port, packet })
                    .collect(),
            }
        }
    }
    fn explanation_candidate(
        &self,
        index: usize,
        order: &[usize],
        request: Option<RequestId>,
    ) -> ReadyAssessment {
        let p = &self.processes[index];
        if p.waiting.is_some() {
            return match self.exact_wait_selection(index) {
                Ok(selected) if selected.ports.is_empty() => ReadyAssessment::Blocked {
                    reasons: vec![ReadinessBlock::MissingRequired {
                        port: p
                            .inputs
                            .iter()
                            .position(|port| Some(&port.name) == p.waiting.as_ref())
                            .unwrap(),
                    }],
                },
                Ok(selected) => self.explanation_selection(selected),
                Err(reason) => ReadyAssessment::Invalid {
                    reason: reason.summary().to_owned(),
                },
            };
        }
        let snapshot = self.selection_snapshot(index, order, request);
        if let Some(reason) = self.staged_selection_error(index, &snapshot) {
            return ReadyAssessment::Invalid {
                reason: reason.summary().to_owned(),
            };
        }
        let Some(names) = self.declared_selection_names(index, &snapshot) else {
            return ReadyAssessment::UnknownCustom;
        };
        if !names.is_empty() {
            return match self.selected_ports(index, &snapshot, names) {
                Ok(selected) => self.explanation_selection(selected),
                Err(reason) => ReadyAssessment::Invalid {
                    reason: reason.summary().to_owned(),
                },
            };
        }
        let policy = p.waiting_policy.as_ref().unwrap_or(&p.policy);
        let available = snapshot.heads.iter().filter(|h| h.available).count();
        let mut reasons = vec![];
        for port in order {
            let head = snapshot
                .heads
                .iter()
                .find(|h| h.port == p.inputs[*port].name)
                .unwrap();
            if head.available {
                continue;
            }
            match &policy.declaration().rule {
                Some(SelectionRule::All) => {
                    reasons.push(ReadinessBlock::MissingRequired { port: *port })
                }
                Some(SelectionRule::Optional { required, optional }) => {
                    if required.contains(&head.port) {
                        reasons.push(ReadinessBlock::MissingRequired { port: *port });
                    }
                    if optional.contains(&head.port) && !head.closed {
                        reasons
                            .push(ReadinessBlock::AwaitingOptionalInputOrClosure { port: *port });
                    }
                }
                _ => {}
            }
        }
        if let Some(SelectionRule::AtLeast(n)) = &policy.declaration().rule {
            reasons.push(ReadinessBlock::Threshold {
                available_ports: available,
                required_ports: n.get(),
            });
        }
        if reasons.is_empty() {
            reasons.push(ReadinessBlock::NoAvailableInput);
        }
        ReadyAssessment::Blocked { reasons }
    }

    /// Passive, bounded current-state metadata. Never evaluates application code,
    /// reserves input, consumes wakes, supervises deadlines, or reads a clock.
    /// Wake atomics are Acquire observations and can race, not a global snapshot.
    /// None means an out-of-range compiled process index. Startup failure remains
    /// visible through session cause and the actual cancelled/idle handoff.
    pub fn explain_process(&self, index: usize, now: Instant) -> Option<ProcessExplanation> {
        let compiled = self.compiled.processes().get(index)?;
        let p = &self.processes[index];
        let policy = p.waiting_policy.as_ref().unwrap_or(&p.policy);
        let order = self.selection_order(index);
        let mut requests = if p.waiting.is_some() {
            vec![p.request]
        } else {
            self.selection_candidates(index, &order)
        };
        let candidate_count = requests.len();
        // Noncandidate scopes explain request absence without changing selection.
        if p.waiting.is_none() {
            if !requests.contains(&None) {
                requests.push(None);
            }
            for request in self.requests.keys() {
                if !requests.contains(&Some(*request)) {
                    requests.push(Some(*request));
                }
            }
        }
        let candidates: Vec<_> = requests
            .into_iter()
            .enumerate()
            .map(|(position, request)| CandidateExplanation {
                request,
                live_lane: request.and_then(|id| self.requests.get(&id).map(|s| s.lane)),
                scheduler_candidate: position < candidate_count,
                inputs: order
                    .iter()
                    .map(|port| CandidateInputExplanation {
                        port: *port,
                        selectable: self.packet_for(
                            index,
                            *port,
                            request,
                            p.waiting.is_none() && policy.correlated(),
                        ),
                        closure: self.input_closure_proof(index, *port, request),
                        fresh: self.closure_fresh(index, *port, request),
                    })
                    .collect(),
                assessment: self.explanation_candidate(index, &order, request),
            })
            .collect();
        let blocked = || {
            candidates
                .iter()
                .find(|c| c.scheduler_candidate)
                .map(|c| c.assessment.clone())
                .unwrap_or(ReadyAssessment::Blocked {
                    reasons: vec![ReadinessBlock::NoCandidate],
                })
        };
        let readiness = if let Err(reason) = self.validate_policy(index, policy) {
            ReadyAssessment::Invalid {
                reason: reason.summary().to_owned(),
            }
        } else if let Some(candidate) = candidates
            .iter()
            .filter(|c| c.scheduler_candidate)
            .find(|c| !matches!(c.assessment, ReadyAssessment::Blocked { .. }))
        {
            candidate.assessment.clone()
        } else if p.handoff.state() != HandoffState::WaitingInput {
            let fallback = self.global_eos_selection(index);
            if !fallback.ports.is_empty() {
                self.explanation_selection(fallback)
            } else {
                blocked()
            }
        } else {
            blocked()
        };
        let empty = matches!(readiness, ReadyAssessment::Blocked { .. })
            && matches!(
                p.handoff.state(),
                HandoffState::Idle | HandoffState::WaitingInput
            )
            && !self.idle_inputs_cannot_progress(index);
        let active = p.wake.active.load(Ordering::Acquire);
        let pending = p.wake.pending.load(Ordering::Acquire);
        Some(ProcessExplanation {
            process: index,
            path: compiled.path().to_owned(),
            session: self.explanation_session(),
            handoff: p.handoff.state(),
            scheduled: self.ready.contains(&index),
            queued_flag: p.queued,
            done: p.done,
            source: p.source,
            owner: p.request,
            live_lane: p
                .request
                .and_then(|id| self.requests.get(&id).map(|s| s.lane)),
            wake: WakeExplanation {
                active,
                pending,
                resume_eligible: p.handoff.state() == HandoffState::WaitingWake
                    || (p.source && p.handoff.state() == HandoffState::Idle),
            },
            queue_policy: self.request_queue_policy,
            job: self.explanation_job(index, now),
            publication: self.explanation_publication(index),
            declaration: if let Some(waiting) = &p.waiting {
                ReadinessDeclaration::WaitInput(waiting.clone())
            } else if p.waiting_policy.is_some() {
                ReadinessDeclaration::WaitInputs
            } else {
                ReadinessDeclaration::Initial
            },
            policy: policy.into(),
            notification_driven: p.notification_driven,
            cursor: p.cursor,
            input_order: order,
            inputs: p
                .inputs
                .iter()
                .enumerate()
                .map(|(port, input)| InputExplanation {
                    port,
                    name: input.name.clone(),
                    cursor: input.cursor,
                    queues: input
                        .queues
                        .iter()
                        .map(|q| self.explanation_queue(*q))
                        .collect(),
                })
                .collect(),
            candidates,
            readiness,
            idle_inputs_cannot_progress: self.idle_inputs_cannot_progress(index),
            idle_completion_after_empty_selection: empty
                .then(|| self.idle_completion_after_empty_selection(index)),
            stranded_after_empty_selection: empty.then(|| self.stranded_at_eos(index)),
        })
    }

    /// Authenticate first, then explain all current request_live terms. Reachable
    /// processes alone are not blockers. No recursive upstream explanation runs.
    pub fn explain_request(
        &self,
        id: RequestId,
        now: Instant,
    ) -> Result<RequestExplanation, RequestError> {
        let scope = self.check_request(id)?;
        let owners = self
            .processes
            .iter()
            .enumerate()
            .filter(|(_, p)| p.request == Some(id))
            .map(|(process, p)| RequestOwnerExplanation {
                process,
                handoff: p.handoff.state(),
                scheduled: self.ready.contains(&process),
                done: p.done,
                job: self.explanation_job(process, now),
                publication: self.explanation_publication(process),
            })
            .collect();
        let queues = self
            .transport
            .queues()
            .iter()
            .enumerate()
            .filter_map(|(index, queue)| {
                let matching_packets = queue.request_count(id);
                if matching_packets == 0 {
                    return None;
                }
                let owner = match &self.endpoints[index].1 {
                    QueueEndpoint::Process { index, .. } => self.processes[*index].request,
                    _ => None,
                };
                Some(RequestQueueExplanation {
                    queue: self.explanation_queue(index),
                    matching_packets,
                    consumer_ownership: match owner {
                        None => ConsumerOwnership::Unowned,
                        Some(owner) if owner == id => ConsumerOwnership::ThisRequest,
                        Some(owner) => ConsumerOwnership::OtherRequest(owner),
                    },
                })
            })
            .collect();
        let closure_obligations = scope
            .reachable
            .iter()
            .copied()
            .filter(|index| self.closure_obligation(*index, id))
            .map(|process| {
                let p = &self.processes[process];
                let names = scope
                    .closure_ports
                    .get(&process)
                    .map(Vec::as_slice)
                    .unwrap_or_else(|| p.policy.declared_closure_ports());
                ClosureObligationExplanation {
                    process,
                    unobserved_ports: p
                        .inputs
                        .iter()
                        .enumerate()
                        .filter(|(port, input)| {
                            names.contains(&input.name)
                                && !scope.observed_inputs.contains(&(process, *port))
                        })
                        .map(|(port, _)| port)
                        .collect(),
                }
            })
            .collect();
        Ok(RequestExplanation {
            request: id,
            session: self.explanation_session(),
            status: self.request_status(id),
            sealed: scope.sealed,
            expected_capture: scope.expected.clone(),
            observed_captures: scope.captures,
            invalid_capture: scope.invalid,
            queue_policy: self.request_queue_policy,
            lane: scope.lane,
            terminal_origin: if scope.terminal.is_some() {
                Some(if scope.inherited_terminal {
                    TerminalOrigin::InheritedSession
                } else {
                    TerminalOrigin::Local
                })
            } else {
                self.inherited_request_terminal()
                    .map(|_| TerminalOrigin::InheritedSession)
            },
            failure: self
                .request_failure(id)?
                .map(|failure| failure.summary().to_owned()),
            work: self.request_work(id)?,
            disposal: scope.disposal,
            invalidated_tickets: scope.cancelled_jobs.clone(),
            owners,
            queues,
            closure_obligations,
        })
    }
}
