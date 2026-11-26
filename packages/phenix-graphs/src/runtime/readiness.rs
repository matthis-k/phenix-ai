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

/// Declarative input requirements. This is a closed description language,
/// not a list of executable policies. Any implementation may declare a rule
/// whose selection and exhaustion semantics it satisfies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectionRule {
    Any,
    All,
    AtLeast(NonZeroUsize),
    Optional {
        required: Vec<String>,
        optional: Vec<String>,
    },
}

/// Immutable policy facts used without invoking application code.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PolicyDeclaration {
    /// A rule promises selection depends only on the supplied input heads
    /// and is equivalent to the declared rule. None means opaque selection.
    pub rule: Option<SelectionRule>,
    /// Permit checked request-local selection behind foreign FIFO heads.
    /// Unscoped packets remain head-only.
    pub correlated: bool,
}

#[derive(Clone)]
enum PolicyRef {
    Static(&'static dyn ReadinessPolicy),
    Shared(Arc<dyn ReadinessPolicy>),
}

/// An ordinary policy handle. Ownership is independent of behavior.
#[derive(Clone)]
pub struct Readiness {
    implementation: PolicyRef,
    declaration: PolicyDeclaration,
}

impl Readiness {
    pub const ANY: Self = Self::declared_static(&Any, Any::DECLARATION);
    pub const ALL: Self = Self::declared_static(&All, All::DECLARATION);
    pub const CORRELATED_ALL: Self =
        Self::declared_static(&CorrelatedAll, CorrelatedAll::DECLARATION);

    /// Capture a policy's immutable declaration once, at construction.
    pub fn new(policy: Arc<dyn ReadinessPolicy>) -> Self {
        let declaration = policy.declaration();
        Self {
            implementation: PolicyRef::Shared(policy),
            declaration,
        }
    }

    pub fn from_policy(policy: impl ReadinessPolicy + 'static) -> Self {
        Self::new(Arc::new(policy))
    }

    pub fn from_static(policy: &'static dyn ReadinessPolicy) -> Self {
        Self::declared_static(policy, policy.declaration())
    }

    /// Construct a static policy with its exact declaration, without allocation.
    /// The declaration must describe the implementation's actual semantics.
    pub const fn declared_static(
        policy: &'static dyn ReadinessPolicy,
        declaration: PolicyDeclaration,
    ) -> Self {
        Self {
            implementation: PolicyRef::Static(policy),
            declaration,
        }
    }

    pub fn at_least(n: NonZeroUsize) -> Self {
        Self::from_policy(AtLeast(n))
    }

    pub fn optional(required: Vec<String>, optional: Vec<String>) -> Self {
        Self::from_policy(Optional { required, optional })
    }

    pub fn declaration(&self) -> &PolicyDeclaration {
        &self.declaration
    }

    fn implementation(&self) -> &dyn ReadinessPolicy {
        match &self.implementation {
            PolicyRef::Static(policy) => *policy,
            PolicyRef::Shared(policy) => policy.as_ref(),
        }
    }
}

impl Default for Readiness {
    fn default() -> Self {
        Self::ANY
    }
}

impl std::fmt::Debug for Readiness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Readiness")
            .field("declaration", &self.declaration)
            .finish_non_exhaustive()
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
    /// Immutable declaration, captured when constructing the policy handle.
    /// Opaque implementations need not describe their selection algorithm.
    fn declaration(&self) -> PolicyDeclaration {
        PolicyDeclaration::default()
    }

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
    match policy.declaration().rule.as_ref() {
        Some(SelectionRule::AtLeast(n)) if n.get() > names.len() => {
            return Err(ReadinessError::Threshold {
                required: n.get(),
                ports: names.len(),
            });
        }
        Some(SelectionRule::Optional { required, optional }) => {
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
        self.declaration.correlated
    }

    pub(super) fn opaque(&self) -> bool {
        self.declaration.rule.is_none()
    }

    pub(super) fn watches_scoped_closure(&self) -> bool {
        matches!(self.declaration.rule, Some(SelectionRule::Optional { .. }))
    }

    pub(super) fn has_zero_packet_closure(&self) -> bool {
        !self.declared_closure_ports().is_empty()
    }

    pub(super) fn declared_closure_ports(&self) -> &[String] {
        match &self.declaration.rule {
            Some(SelectionRule::Optional { required, optional }) if required.is_empty() => optional,
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
        match self.declaration.rule.as_ref() {
            Some(SelectionRule::Optional { required, .. }) => required
                .iter()
                .find(|name| missing(name))
                .map(|name| ReadinessError::RequiredClosed { port: name.clone() }),
            Some(SelectionRule::All) => snapshot
                .heads
                .iter()
                .find(|h| !h.available && h.closed)
                .map(|h| ReadinessError::RequiredClosed {
                    port: h.port.clone(),
                }),
            Some(SelectionRule::Any) if snapshot.heads.iter().all(|h| !h.available && h.closed) => {
                Some(ReadinessError::AnyExhausted)
            }
            Some(SelectionRule::AtLeast(n))
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

    /// Passive selection evaluates only immutable declarative facts.
    fn passive_selection(&self, snapshot: &ReadinessSnapshot) -> Option<Vec<String>> {
        Some(match self.declaration.rule.as_ref()? {
            SelectionRule::Any => Any.select(snapshot),
            SelectionRule::All => All.select(snapshot),
            SelectionRule::AtLeast(n) => AtLeast(*n).select(snapshot),
            SelectionRule::Optional { required, optional } => {
                select_optional(required, optional, snapshot)
            }
        })
    }
}

pub struct Any;
impl Any {
    const DECLARATION: PolicyDeclaration = PolicyDeclaration {
        rule: Some(SelectionRule::Any),
        correlated: false,
    };
}
impl ReadinessPolicy for Any {
    fn declaration(&self) -> PolicyDeclaration {
        Self::DECLARATION
    }

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

pub struct All;
impl All {
    const DECLARATION: PolicyDeclaration = PolicyDeclaration {
        rule: Some(SelectionRule::All),
        correlated: false,
    };
}
impl ReadinessPolicy for All {
    fn declaration(&self) -> PolicyDeclaration {
        Self::DECLARATION
    }

    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        if snapshot.heads.iter().all(|head| head.available) {
            snapshot
                .heads
                .iter()
                .map(|head| head.port.clone())
                .collect()
        } else {
            vec![]
        }
    }
}

pub struct CorrelatedAll;
impl CorrelatedAll {
    const DECLARATION: PolicyDeclaration = PolicyDeclaration {
        rule: Some(SelectionRule::All),
        correlated: true,
    };
}
impl ReadinessPolicy for CorrelatedAll {
    fn declaration(&self) -> PolicyDeclaration {
        Self::DECLARATION
    }

    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        All.select(snapshot)
    }
}

pub struct AtLeast(pub NonZeroUsize);
impl ReadinessPolicy for AtLeast {
    fn declaration(&self) -> PolicyDeclaration {
        PolicyDeclaration {
            rule: Some(SelectionRule::AtLeast(self.0)),
            correlated: false,
        }
    }

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

pub struct Optional {
    pub required: Vec<String>,
    pub optional: Vec<String>,
}
impl ReadinessPolicy for Optional {
    fn declaration(&self) -> PolicyDeclaration {
        PolicyDeclaration {
            rule: Some(SelectionRule::Optional {
                required: self.required.clone(),
                optional: self.optional.clone(),
            }),
            correlated: false,
        }
    }

    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        select_optional(&self.required, &self.optional, snapshot)
    }
}

fn select_optional(
    required: &[String],
    optional: &[String],
    snapshot: &ReadinessSnapshot,
) -> Vec<String> {
    let found = |name: &String| snapshot.heads.iter().find(|h| &h.port == name);
    if required
        .iter()
        .all(|name| found(name).is_some_and(|head| head.available))
        && optional
            .iter()
            .all(|name| found(name).is_some_and(|head| head.available || head.closed))
    {
        required.iter().chain(optional).cloned().collect()
    } else {
        vec![]
    }
}

impl ReadinessPolicy for Readiness {
    fn declaration(&self) -> PolicyDeclaration {
        self.declaration.clone()
    }

    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        self.implementation().select(snapshot)
    }

    fn closure_ports(&self, lane: usize) -> Vec<String> {
        self.implementation().closure_ports(lane)
    }

    fn notification_driven(&self) -> bool {
        self.implementation().notification_driven()
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

    pub(super) fn declared_selection_names(
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
            let names =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| policy.select(&snapshot)))
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
