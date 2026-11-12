use super::*;

#[derive(Clone, Default)]
pub enum Readiness {
    #[default]
    Any,
    All,
    /// Opt-in request-scoped join. Selects the earliest packet of one request
    /// from each bounded input queue, even behind another request's head.
    /// Preserves order within each request, not global cross-request FIFO.
    /// Unscoped packets remain strictly head-only.
    CorrelatedAll,
    AtLeast(NonZeroUsize),
    /// Omit optional inputs only at actual EOS or scheduler-proven request-local
    /// upstream closure. Sealing a request alone is not closure evidence.
    Optional {
        required: Vec<String>,
        optional: Vec<String>,
    },
    /// Pure head-only selection. Processes stage subsequent reservations using
    /// WaitInput; a policy cannot skip packets or own a second payload buffer.
    Custom(Arc<dyn ReadinessPolicy>),
}
impl std::fmt::Debug for Readiness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Any => f.write_str("Any"),
            Self::All => f.write_str("All"),
            Self::CorrelatedAll => f.write_str("CorrelatedAll"),
            Self::AtLeast(n) => f.debug_tuple("AtLeast").field(n).finish(),
            Self::Optional { required, optional } => f
                .debug_struct("Optional")
                .field("required", required)
                .field("optional", optional)
                .finish(),
            Self::Custom(_) => f.write_str("Custom(..)"),
        }
    }
}
#[derive(Clone, Debug)]
pub struct InputHead {
    pub port: String,
    pub available: bool,
    pub closed: bool,
}
#[derive(Clone, Debug)]
pub struct ReadinessSnapshot {
    pub request: Option<RequestId>,
    pub heads: Vec<InputHead>,
}
pub trait ReadinessPolicy: Send + Sync {
    /// Return distinct port names whose heads should be atomically reserved.
    /// Every selected port must have a matching head or proven closure.
    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String>;
}
pub(super) struct Reservation {
    pub ports: Vec<(usize, Option<(usize, usize)>)>,
    request: Option<RequestId>,
}

/// Validate only declarations. Never inspect application code or spawn a factory.
pub(super) fn validate_policy_ports<'a>(
    ports: impl Iterator<Item = &'a str>,
    policy: &Readiness,
) -> Result<(), String> {
    let names: BTreeSet<&str> = ports.collect();
    match policy {
        Readiness::AtLeast(n) if n.get() > names.len() => {
            return Err("readiness threshold exceeds input port count".into());
        }
        Readiness::Optional { required, optional } => {
            let mut seen = BTreeSet::new();
            for name in required.iter().chain(optional) {
                if !names.contains(name.as_str()) || !seen.insert(name) {
                    return Err(format!("invalid optional readiness port {name}"));
                }
            }
            if seen.is_empty() {
                return Err("empty optional readiness policy".into());
            }
        }
        _ => {}
    }
    Ok(())
}

impl NetworkSession {
    pub(super) fn validate_policy(&self, index: usize, policy: &Readiness) -> Result<(), String> {
        validate_policy_ports(
            self.processes[index].inputs.iter().map(|p| p.name.as_str()),
            policy,
        )
    }
    fn packet_for(
        &self,
        index: usize,
        port: usize,
        request: Option<RequestId>,
        correlated: bool,
    ) -> Option<(usize, usize)> {
        let input = &self.processes[index].inputs[port];
        for offset in 0..input.queues.len() {
            let queue = input.queues[(input.cursor + offset) % input.queues.len()];
            let packets = &self.transport.queues()[queue];
            let position = if correlated && request.is_some() {
                packets
                    .packets()
                    .position(|packet| packet.metadata().and_then(|m| m.request()) == request)
            } else {
                packets.head().and_then(|packet| {
                    (packet.metadata().and_then(|m| m.request()) == request).then_some(0)
                })
            };
            if let Some(position) = position {
                return Some((queue, position));
            }
        }
        None
    }
    pub(super) fn selection(&self, index: usize) -> Result<Reservation, String> {
        let p = &self.processes[index];
        let count = p.inputs.len();
        self.validate_policy(index, p.waiting_policy.as_ref().unwrap_or(&p.policy))?;
        if let Some(waiting) = &p.waiting {
            let port = p
                .inputs
                .iter()
                .position(|port| &port.name == waiting)
                .unwrap();
            let queue = self.packet_for(index, port, p.request, false);
            let closed = self.input_closed_for(index, port, p.request);
            if queue.is_none() && closed && !self.closure_fresh(index, port, p.request) {
                return Err(format!(
                    "WaitInput requested already observed closure: {waiting}"
                ));
            }
            return Ok(Reservation {
                ports: if queue.is_some() || closed {
                    vec![(port, queue)]
                } else {
                    vec![]
                },
                request: p.request,
            });
        }
        let order: Vec<_> = (0..count)
            .map(|offset| (p.cursor + offset) % count)
            .collect();
        let policy = p.waiting_policy.as_ref().unwrap_or(&p.policy);
        let correlated = matches!(policy, Readiness::CorrelatedAll);
        let mut candidates = vec![];
        for port in &order {
            let input = &p.inputs[*port];
            for offset in 0..input.queues.len() {
                let queue = input.queues[(input.cursor + offset) % input.queues.len()];
                for (position, packet) in self.transport.queues()[queue].packets().enumerate() {
                    if position > 0 && !correlated {
                        break;
                    }
                    let request = packet.metadata().and_then(|m| m.request());
                    // A global (unscoped) packet is never dequeued out of FIFO
                    // order, even under this explicit request correlation policy.
                    if position == 0 || request.is_some() {
                        if !candidates.contains(&request) {
                            candidates.push(request);
                        }
                    }
                }
            }
        }
        if p.handoff.state() == HandoffState::WaitingInput {
            candidates = vec![p.request];
        } else {
            for (request, scope) in &self.requests {
                if scope.sealed
                    && self.closure_obligation(index, *request)
                    && !candidates.contains(&Some(*request))
                {
                    candidates.push(Some(*request));
                }
            }
        }
        for request in candidates {
            let snapshot = ReadinessSnapshot {
                request,
                heads: order
                    .iter()
                    .map(|port| InputHead {
                        port: p.inputs[*port].name.clone(),
                        available: self.packet_for(index, *port, request, correlated).is_some(),
                        closed: self.input_closed_for(index, *port, request),
                    })
                    .collect(),
            };
            let available: Vec<_> = snapshot
                .heads
                .iter()
                .filter(|h| h.available)
                .map(|h| h.port.clone())
                .collect();
            let policy = p.waiting_policy.as_ref().unwrap_or(&p.policy);
            if p.handoff.state() == HandoffState::WaitingInput {
                let missing = |name: &String| {
                    snapshot
                        .heads
                        .iter()
                        .find(|h| &h.port == name)
                        .is_some_and(|h| !h.available && h.closed)
                };
                let impossible = match policy {
                    Readiness::Optional { required, .. } => {
                        required.iter().find(|name| missing(name)).map(|name| {
                            format!("required input {name} closed before its staged reservation")
                        })
                    }
                    Readiness::All | Readiness::CorrelatedAll => snapshot
                        .heads
                        .iter()
                        .find(|h| !h.available && h.closed)
                        .map(|h| {
                            format!(
                                "required input {} closed before its staged reservation",
                                h.port
                            )
                        }),
                    Readiness::Any if snapshot.heads.iter().all(|h| !h.available && h.closed) => {
                        Some("staged Any group exhausted without input".into())
                    }
                    Readiness::AtLeast(n)
                        if snapshot
                            .heads
                            .iter()
                            .filter(|h| h.available || !h.closed)
                            .count()
                            < n.get() =>
                    {
                        Some(format!(
                            "staged input group cannot supply {} required inputs",
                            n.get()
                        ))
                    }
                    _ => None,
                };
                if let Some(error) = impossible {
                    return Err(error);
                }
            }
            let names = match policy {
                Readiness::Any => available.into_iter().take(1).collect(),
                Readiness::All | Readiness::CorrelatedAll if available.len() == count => available,
                Readiness::AtLeast(n) if available.len() >= n.get() => {
                    available.into_iter().take(n.get()).collect()
                }
                Readiness::Optional { required, optional } => {
                    let found = |name: &String| snapshot.heads.iter().find(|h| &h.port == name);
                    if required
                        .iter()
                        .all(|n| found(n).is_some_and(|h| h.available))
                        && optional
                            .iter()
                            .all(|n| found(n).is_some_and(|h| h.available || h.closed))
                    {
                        required.iter().chain(optional).cloned().collect()
                    } else {
                        vec![]
                    }
                }
                Readiness::Custom(policy) => {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        policy.select(&snapshot)
                    }))
                    .map_err(|_| "readiness policy panicked".to_owned())?
                }
                _ => vec![],
            };
            if names.is_empty() {
                if p.handoff.state() == HandoffState::WaitingInput
                    && matches!(policy, Readiness::Custom(_))
                    && snapshot.heads.iter().all(|h| h.available || h.closed)
                {
                    return Err(
                        "staged custom required group exhausted without a legal selection".into(),
                    );
                }
                continue;
            }
            let mut ports = vec![];
            for name in names {
                let port = p
                    .inputs
                    .iter()
                    .position(|port| port.name == name)
                    .ok_or_else(|| format!("unknown readiness port {name}"))?;
                let queue = self.packet_for(index, port, request, correlated);
                if ports.iter().any(|(selected, _)| *selected == port)
                    || (queue.is_none() && !self.input_closed_for(index, port, request))
                {
                    return Err(format!("invalid readiness selection for {name}"));
                }
                ports.push((port, queue));
            }
            if !ports
                .iter()
                .any(|(port, queue)| queue.is_some() || self.closure_fresh(index, *port, request))
            {
                continue;
            }
            return Ok(Reservation { ports, request });
        }
        if p.handoff.state() == HandoffState::WaitingInput {
            // A staged policy must select its own group. Closure on an unrelated
            // or optional port cannot satisfy missing required input.
            return Ok(Reservation {
                ports: vec![],
                request: p.request,
            });
        }
        let request = None;
        // Idle global EOS notifications remain uncorrelated. Exact WaitInput
        // closure is handled above; WaitInputs has no implicit EOS fallback.
        let ports = (0..count)
            .filter(|port| {
                self.closure_fresh(index, *port, request)
                    && self.input_closed_for(index, *port, request)
            })
            .map(|port| (port, None))
            .collect();
        Ok(Reservation { ports, request })
    }
    pub(super) fn reserve(&mut self, index: usize, selected: Reservation) -> ProcessContext {
        if self.processes[index].handoff.state() == HandoffState::Idle {
            self.processes[index].closed_in_activation.clear();
        }
        self.processes[index].request = selected.request;
        let mut inputs = BTreeMap::new();
        let mut closed = vec![];
        let mut request_closed = vec![];
        for (port, queue) in selected.ports {
            let global_eos = self.closed(index, port);
            let p = &mut self.processes[index].inputs[port];
            if let Some(queue) = queue {
                let packet = self.transport.pop_at(queue.0, queue.1).unwrap();
                inputs.insert(p.name.clone(), packet.into_value());
                p.cursor = (p.queues.iter().position(|q| *q == queue.0).unwrap() + 1)
                    % p.queues.len();
            } else {
                if global_eos {
                    p.eos_delivered = true;
                    closed.push(p.name.clone());
                } else {
                    request_closed.push(p.name.clone());
                }
                self.processes[index].closed_in_activation.insert(port);
            }
            // Data already resolves this optional input for the scope; it must
            // not replay later as an EOS-only missing branch.
            if let Some(request) = selected.request {
                self.requests
                    .get_mut(&request)
                    .unwrap()
                    .observed_inputs
                    .insert((index, port));
            }
            self.processes[index].cursor = (port + 1) % self.processes[index].inputs.len();
        }
        ProcessContext::new(inputs, closed)
            .with_request_closed(request_closed)
            .with_resources(self.processes[index].resources.clone())
    }
}
