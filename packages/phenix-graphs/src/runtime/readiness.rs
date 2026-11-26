use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadinessError {
    Threshold { required: usize, ports: usize },
    InvalidOptional { port: String },
    EmptyOptional,
    RepeatedClosure { port: String },
    RequiredClosed { port: String },
    AnyExhausted,
    ThresholdExhausted { required: usize },
    UnknownPort { port: String },
    InvalidSelection { port: String },
    CustomExhausted,
}
impl std::fmt::Display for ReadinessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Threshold { .. } => f.write_str("readiness threshold exceeds input port count"),
            Self::InvalidOptional { port } => write!(f, "invalid optional readiness port {port}"),
            Self::EmptyOptional => f.write_str("empty optional readiness policy"),
            Self::RepeatedClosure { port } => {
                write!(f, "WaitInput requested already observed closure: {port}")
            }
            Self::RequiredClosed { port } => write!(
                f,
                "required input {port} closed before its staged reservation"
            ),
            Self::AnyExhausted => f.write_str("staged Any group exhausted without input"),
            Self::ThresholdExhausted { required } => write!(
                f,
                "staged input group cannot supply {required} required inputs"
            ),
            Self::UnknownPort { port } => write!(f, "unknown readiness port {port}"),
            Self::InvalidSelection { port } => write!(f, "invalid readiness selection for {port}"),
            Self::CustomExhausted => {
                f.write_str("staged custom required group exhausted without a legal selection")
            }
        }
    }
}
impl std::error::Error for ReadinessError {}
impl ReadinessError {
    pub(crate) fn failure(self) -> Failure {
        Failure::external(format!("{self}"), self).with_kind(FailureKind::Framework)
    }
}

#[derive(Clone, Default)]
pub enum Readiness {
    #[default]
    Any,
    All,
    /// Opt-in request-scoped join. Selects the earliest packet of one request
    /// from each bounded input queue, even behind another request's head.
    /// Preserves order within each request, not global cross-request FIFO.
    /// An exact WaitInput continuation owned by this policy may also skip
    /// foreign heads for its next authenticated request packet. It does not
    /// change head-only staged waits under other readiness policies.
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
    pub correlation_lane: Option<usize>,
    pub heads: Vec<InputHead>,
}
pub trait ReadinessPolicy: Send + Sync {
    /// Request-local closure obligations in this process's input namespace. Called
    /// once before a request scope is admitted; a panic rejects that admission.
    fn closure_ports(&self, _lane: usize) -> Vec<String> {
        vec![]
    }
    /// Return distinct port names whose heads should be atomically reserved.
    /// Every selected port must have a matching head or proven closure.
    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String>;

    /// Opt in to event-driven readiness. False (the default) retains polling.
    ///
    /// Returning true promises that every external state change affecting
    /// select() is followed by a ProcessNotifier::notify() for this process.
    /// Packet, global EOS and sealed request changes remain kernel events.
    /// This declaration is read once when the session starts. Violating the
    /// contract can strand work; the kernel cannot observe arbitrary state.
    fn notification_driven(&self) -> bool {
        false
    }
}
pub(super) struct Reservation {
    pub ports: Vec<(usize, Option<(usize, usize)>)>,
    pub(super) request: Option<RequestId>,
}

/// Validate only declarations. Never inspect application code or spawn a factory.
pub(super) fn validate_policy_ports<'a>(
    ports: impl Iterator<Item = &'a str>,
    policy: &Readiness,
) -> Result<(), ReadinessError> {
    let names: BTreeSet<&str> = ports.collect();
    match policy {
        Readiness::AtLeast(n) if n.get() > names.len() => {
            return Err(ReadinessError::Threshold {
                required: n.get(),
                ports: names.len(),
            });
        }
        Readiness::Optional { required, optional } => {
            let mut seen = BTreeSet::new();
            for name in required.iter().chain(optional) {
                if !names.contains(name.as_str()) || !seen.insert(name) {
                    return Err(ReadinessError::InvalidOptional { port: name.clone() });
                }
            }
            if seen.is_empty() {
                return Err(ReadinessError::EmptyOptional);
            }
        }
        _ => {}
    }
    Ok(())
}

impl Readiness {
    pub(super) fn correlated(&self) -> bool {
        matches!(self, Self::CorrelatedAll)
    }

    pub(super) fn opaque(&self) -> bool {
        matches!(self, Self::Custom(_))
    }

    pub(super) fn watches_scoped_closure(&self) -> bool {
        matches!(self, Self::Optional { .. })
    }

    pub(super) fn has_zero_packet_closure(&self) -> bool {
        matches!(self, Self::Optional { required, .. } if required.is_empty())
    }

    pub(super) fn declared_closure_ports(&self) -> &[String] {
        match self {
            Self::Optional { required, optional } if required.is_empty() => optional,
            _ => &[],
        }
    }

    /// Pure staged exhaustion assessment, independent of queues or scheduler.
    fn staged_error(&self, snapshot: &ReadinessSnapshot) -> Option<ReadinessError> {
        let missing = |name: &String| {
            snapshot
                .heads
                .iter()
                .find(|h| &h.port == name)
                .is_some_and(|h| !h.available && h.closed)
        };
        match self {
            Self::Optional { required, .. } => required
                .iter()
                .find(|name| missing(name))
                .map(|name| ReadinessError::RequiredClosed { port: name.clone() }),
            Self::All | Self::CorrelatedAll => snapshot
                .heads
                .iter()
                .find(|h| !h.available && h.closed)
                .map(|h| ReadinessError::RequiredClosed {
                    port: h.port.clone(),
                }),
            Self::Any if snapshot.heads.iter().all(|h| !h.available && h.closed) => {
                Some(ReadinessError::AnyExhausted)
            }
            Self::AtLeast(n)
                if snapshot
                    .heads
                    .iter()
                    .filter(|h| h.available || !h.closed)
                    .count()
                    < n.get() =>
            {
                Some(ReadinessError::ThresholdExhausted { required: n.get() })
            }
            _ => None,
        }
    }

    /// Passive selection for declarative policies; opaque callbacks are never evaluated here.
    fn passive_selection(&self, snapshot: &ReadinessSnapshot) -> Option<Vec<String>> {
        (!self.opaque()).then(|| self.select(snapshot))
    }
}

// The standard selectors implement the same selection contract as externally
// authored policies. The remaining enum is the declaration/ownership carrier.
struct Any;
impl ReadinessPolicy for Any {
    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        snapshot
            .heads
            .iter()
            .filter(|head| head.available)
            .take(1)
            .map(|head| head.port.clone())
            .collect()
    }
}

struct All;
impl ReadinessPolicy for All {
    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        if snapshot.heads.iter().all(|head| head.available) {
            snapshot.heads.iter().map(|head| head.port.clone()).collect()
        } else {
            vec![]
        }
    }
}

struct AtLeast(NonZeroUsize);
impl ReadinessPolicy for AtLeast {
    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        let selected: Vec<_> = snapshot
            .heads
            .iter()
            .filter(|head| head.available)
            .take(self.0.get())
            .map(|head| head.port.clone())
            .collect();
        if selected.len() == self.0.get() {
            selected
        } else {
            vec![]
        }
    }
}

struct Optional<'a> {
    required: &'a [String],
    optional: &'a [String],
}
impl ReadinessPolicy for Optional<'_> {
    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        let found = |name: &String| snapshot.heads.iter().find(|h| &h.port == name);
        if self
            .required
            .iter()
            .all(|name| found(name).is_some_and(|head| head.available))
            && self
                .optional
                .iter()
                .all(|name| found(name).is_some_and(|head| head.available || head.closed))
        {
            self.required.iter().chain(self.optional).cloned().collect()
        } else {
            vec![]
        }
    }
}

impl ReadinessPolicy for Readiness {
    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        match self {
            Self::Any => Any.select(snapshot),
            Self::All | Self::CorrelatedAll => All.select(snapshot),
            Self::AtLeast(n) => AtLeast(*n).select(snapshot),
            Self::Optional { required, optional } => {
                Optional { required, optional }.select(snapshot)
            }
            Self::Custom(policy) => policy.select(snapshot),
        }
    }

    fn closure_ports(&self, lane: usize) -> Vec<String> {
        match self {
            Self::Custom(policy) => policy.closure_ports(lane),
            _ => self.declared_closure_ports().to_vec(),
        }
    }

    fn notification_driven(&self) -> bool {
        match self {
            Self::Custom(policy) => policy.notification_driven(),
            _ => true,
        }
    }
}

impl NetworkSession {
    pub(super) fn validate_policy(&self, index: usize, policy: &Readiness) -> Result<(), Failure> {
        validate_policy_ports(
            self.processes[index].inputs.iter().map(|p| p.name.as_str()),
            policy,
        )
        .map_err(ReadinessError::failure)
    }
    pub(super) fn packet_for(
        &self,
        index: usize,
        port: usize,
        request: Option<RequestId>,
        correlated: bool,
    ) -> Option<(usize, usize)> {
        let input = &self.processes[index].inputs[port];
        let queues = match (self.request_queue_policy, request) {
            (RequestQueuePolicy::BoundedLanes, Some(request)) => {
                let lane = self.requests[&request].lane;
                self.request_lanes[&lane]
                    .input_ports
                    .get(&(index, port))
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
            }
            (RequestQueuePolicy::BoundedLanes, None) => {
                let canonical_count = input
                    .queues
                    .partition_point(|queue| self.queue_lanes[*queue].is_none());
                &input.queues[..canonical_count]
            }
            (RequestQueuePolicy::SharedFifo, _) => &input.queues,
        };
        for offset in 0..queues.len() {
            let queue = queues[(input.cursor + offset) % queues.len()];
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
    pub(super) fn exact_wait_selection(&self, index: usize) -> Result<Reservation, Failure> {
        let p = &self.processes[index];
        if let Some(waiting) = &p.waiting {
            let port = p
                .inputs
                .iter()
                .position(|port| &port.name == waiting)
                .unwrap();
            // A continuation owned by CorrelatedAll may take the first
            // packet of its authenticated request behind foreign FIFO heads.
            // Without this, a crossed peer head can indefinitely strand an
            // already-owned WaitInput even when its own next packet is queued.
            // All other policies retain strict head-only staged reservations.
            let correlated = p.waiting_policy.as_ref().unwrap_or(&p.policy).correlated();
            let queue = self.packet_for(index, port, p.request, correlated);
            let closed = self.input_closed_for(index, port, p.request);
            if queue.is_none() && closed && !self.closure_fresh(index, port, p.request) {
                return Err(ReadinessError::RepeatedClosure {
                    port: waiting.clone(),
                }
                .failure());
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
        unreachable!("exact wait requires a port");
    }
    pub(super) fn selection_order(&self, index: usize) -> Vec<usize> {
        let p = &self.processes[index];
        let count = p.inputs.len();
        (0..count)
            .map(|offset| (p.cursor + offset) % count)
            .collect()
    }
    pub(super) fn selection_candidates(
        &self,
        index: usize,
        order: &[usize],
    ) -> Vec<Option<RequestId>> {
        let p = &self.processes[index];
        let policy = p.waiting_policy.as_ref().unwrap_or(&p.policy);
        let correlated = policy.correlated();
        let mut candidates = vec![];
        for port in order {
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
                    if (position == 0 || request.is_some()) && !candidates.contains(&request) {
                        candidates.push(request);
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
        // CorrelatedAll opts in to cross-request head skipping, so an always
        // ready older request must not permanently win on input queue order.
        // Reuse the existing bounded active-lane cursor rather than adding a
        // second request queue or disturbing ordinary SharedFifo policies.
        if self.request_queue_policy == RequestQueuePolicy::BoundedLanes || correlated {
            candidates.sort_by_key(|request| {
                request.map_or(self.max_requests, |id| self.requests[&id].lane)
            });
            let next = candidates.partition_point(|request| {
                request.map_or(self.max_requests, |id| self.requests[&id].lane)
                    < self.request_cursors.get(&index).copied().unwrap_or(0)
            });
            let candidate_count = candidates.len();
            if candidate_count != 0 {
                candidates.rotate_left(next % candidate_count);
            }
        }
        candidates
    }
    pub(super) fn selection_snapshot(
        &self,
        index: usize,
        order: &[usize],
        request: Option<RequestId>,
    ) -> ReadinessSnapshot {
        let p = &self.processes[index];
        let correlated = p.waiting_policy.as_ref().unwrap_or(&p.policy).correlated();
        ReadinessSnapshot {
            request,
            correlation_lane: None,
            heads: order
                .iter()
                .map(|port| InputHead {
                    port: p.inputs[*port].name.clone(),
                    available: self.packet_for(index, *port, request, correlated).is_some(),
                    closed: self.input_closed_for(index, *port, request),
                })
                .collect(),
        }
    }
    pub(super) fn staged_selection_error(
        &self,
        index: usize,
        snapshot: &ReadinessSnapshot,
    ) -> Option<Failure> {
        let p = &self.processes[index];
        if p.handoff.state() != HandoffState::WaitingInput {
            return None;
        }
        p.waiting_policy
            .as_ref()
            .unwrap_or(&p.policy)
            .staged_error(snapshot)
            .map(ReadinessError::failure)
    }

    pub(super) fn builtin_selection_names(
        &self,
        index: usize,
        snapshot: &ReadinessSnapshot,
    ) -> Option<Vec<String>> {
        let p = &self.processes[index];
        p.waiting_policy
            .as_ref()
            .unwrap_or(&p.policy)
            .passive_selection(snapshot)
    }

    pub(super) fn selected_ports(
        &self,
        index: usize,
        snapshot: &ReadinessSnapshot,
        names: Vec<String>,
    ) -> Result<Reservation, Failure> {
        let p = &self.processes[index];
        let request = snapshot.request;
        let correlated = p.waiting_policy.as_ref().unwrap_or(&p.policy).correlated();
        let mut ports = vec![];
        for name in names {
            let port = p
                .inputs
                .iter()
                .position(|port| port.name == name)
                .ok_or_else(|| ReadinessError::UnknownPort { port: name.clone() }.failure())?;
            let queue = self.packet_for(index, port, request, correlated);
            let closed = snapshot
                .heads
                .iter()
                .find(|head| head.port == name)
                .is_some_and(|head| head.closed);
            if ports.iter().any(|(selected, _)| *selected == port) || (queue.is_none() && !closed) {
                return Err(ReadinessError::InvalidSelection { port: name }.failure());
            }
            ports.push((port, queue));
        }
        if !ports
            .iter()
            .any(|(port, queue)| queue.is_some() || self.closure_fresh(index, *port, request))
        {
            ports.clear();
        }
        Ok(Reservation { ports, request })
    }
    pub(super) fn global_eos_selection(&self, index: usize) -> Reservation {
        let count = self.processes[index].inputs.len();
        let request = None;
        let ports = (0..count)
            .filter(|port| {
                self.closure_fresh(index, *port, request)
                    && self.input_closed_for(index, *port, request)
            })
            .map(|port| (port, None))
            .collect();
        Reservation { ports, request }
    }
    pub(super) fn selection(&self, index: usize) -> Result<Reservation, Failure> {
        let p = &self.processes[index];
        self.validate_policy(index, p.waiting_policy.as_ref().unwrap_or(&p.policy))?;
        if p.waiting.is_some() {
            return self.exact_wait_selection(index);
        }
        let order = self.selection_order(index);
        for request in self.selection_candidates(index, &order) {
            let mut snapshot = self.selection_snapshot(index, &order, request);
            if let Some(error) = self.staged_selection_error(index, &snapshot) {
                return Err(error);
            }
            let policy = p.waiting_policy.as_ref().unwrap_or(&p.policy);
            snapshot.correlation_lane = request.map(|id| self.requests[&id].lane);
            let names = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                policy.select(&snapshot)
            }))
            .map_err(|payload| {
                crate::process::callback_panicked(CallbackPhase::Readiness, payload)
            })?;
            if names.is_empty() {
                if p.handoff.state() == HandoffState::WaitingInput
                    && policy.opaque()
                    && snapshot.heads.iter().all(|h| h.available || h.closed)
                {
                    return Err(ReadinessError::CustomExhausted.failure());
                }
                continue;
            }
            let selected = self.selected_ports(index, &snapshot, names)?;
            if selected.ports.is_empty() {
                continue;
            }
            return Ok(selected);
        }
        if p.handoff.state() == HandoffState::WaitingInput {
            // A staged policy must select its own group. Closure on an unrelated
            // or optional port cannot satisfy missing required input.
            return Ok(Reservation {
                ports: vec![],
                request: p.request,
            });
        }
        // Idle global EOS notifications remain uncorrelated. Exact WaitInput
        // closure is handled above; WaitInputs has no implicit EOS fallback.
        Ok(self.global_eos_selection(index))
    }
    pub(super) fn reserve(&mut self, index: usize, selected: Reservation) -> ProcessContext {
        if self.processes[index].handoff.state() == HandoffState::Idle {
            self.processes[index].closed_in_activation.clear();
        }
        let request = selected.request;
        self.processes[index].request = request;
        if self.request_queue_policy == RequestQueuePolicy::BoundedLanes
            || self.processes[index]
                .waiting_policy
                .as_ref()
                .unwrap_or(&self.processes[index].policy)
                .correlated()
        {
            self.request_cursors
                .insert(index, request.map_or(0, |id| self.requests[&id].lane + 1));
        }
        let mut inputs = BTreeMap::new();
        let mut closed = vec![];
        let mut request_closed = vec![];
        for (port, queue) in selected.ports {
            let global_eos = self.closed(index, port);
            let p = &mut self.processes[index].inputs[port];
            if let Some(queue) = queue {
                let packet = self.transport.pop_at(queue.0, queue.1).unwrap();
                inputs.insert(p.name.clone(), packet.into_value());
                p.cursor =
                    (p.queues.iter().position(|q| *q == queue.0).unwrap() + 1) % p.queues.len();
                // Releasing bounded input capacity can unblock a retained
                // producer fanout even if the producer precedes this consumer.
                self.dirty_queue(queue.0);
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
            .with_correlation_lane(request.map(|id| self.requests[&id].lane))
            .with_request_closed(request_closed)
            .with_resources(self.processes[index].resources.clone())
    }
}

pub(crate) fn validate_policy<'a>(
    policy: &Readiness,
    input_names: impl IntoIterator<Item = &'a str>,
) -> Result<(), ReadinessError> {
    validate_policy_ports(input_names.into_iter(), policy)
}
