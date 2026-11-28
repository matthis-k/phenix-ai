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

/// Declarative input requirements. These rules define selection, exhaustion
/// and passive inspection together. Arbitrary selectors use an opaque policy
/// instead; either form is available to all callers.
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
    /// A rule determines selection from the supplied input heads. None means
    /// the policy's opaque selector determines selection.
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
    implementation: Option<PolicyRef>,
    declaration: PolicyDeclaration,
}

impl Readiness {
    pub const ANY: Self = Self::declared(SelectionRule::Any, false);
    pub const ALL: Self = Self::declared(SelectionRule::All, false);
    pub const CORRELATED_ALL: Self = Self::declared(SelectionRule::All, true);

    /// Construct a declarative policy without allocation or application code.
    /// The same rule drives execution, validation, closure and inspection.
    pub const fn declared(rule: SelectionRule, correlated: bool) -> Self {
        Self {
            implementation: None,
            declaration: PolicyDeclaration {
                rule: Some(rule),
                correlated,
            },
        }
    }

    /// Capture a policy's immutable declaration once, at construction.
    pub fn new(policy: Arc<dyn ReadinessPolicy>) -> Self {
        let declaration = policy.declaration();
        Self {
            implementation: Some(PolicyRef::Shared(policy)),
            declaration,
        }
    }

    pub fn from_policy(policy: impl ReadinessPolicy + 'static) -> Self {
        Self::new(Arc::new(policy))
    }

    pub fn from_static(policy: &'static dyn ReadinessPolicy) -> Self {
        Self {
            implementation: Some(PolicyRef::Static(policy)),
            declaration: policy.declaration(),
        }
    }

    pub fn at_least(n: NonZeroUsize) -> Self {
        Self::declared(SelectionRule::AtLeast(n), false)
    }

    pub fn optional(required: Vec<String>, optional: Vec<String>) -> Self {
        Self::declared(SelectionRule::Optional { required, optional }, false)
    }

    pub fn declaration(&self) -> &PolicyDeclaration {
        &self.declaration
    }

    fn implementation(&self) -> Option<&dyn ReadinessPolicy> {
        Some(match self.implementation.as_ref()? {
            PolicyRef::Static(policy) => *policy,
            PolicyRef::Shared(policy) => policy.as_ref(),
        })
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
    /// If a rule is declared, the runtime evaluates that rule instead of
    /// calling the selector. Opaque policies need not describe their algorithm.
    fn declaration(&self) -> PolicyDeclaration {
        PolicyDeclaration::default()
    }

    /// Request-local closure obligations in this process's input namespace. Called
    /// once before a request scope is admitted; a panic rejects that admission.
    fn closure_ports(&self, _lane: usize) -> Vec<String> {
        vec![]
    }
    /// Select input ports for an opaque policy (one without a declared rule).
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

/// Preserve first discovery order without quadratic duplicate searches in
/// wide request sets. Small scans need no second allocation.
#[derive(Default)]
struct RequestCandidates {
    ordered: Vec<Option<RequestId>>,
    membership: Option<BTreeSet<Option<RequestId>>>,
}

impl RequestCandidates {
    fn insert(&mut self, request: Option<RequestId>) {
        if let Some(membership) = &mut self.membership {
            if membership.insert(request) {
                self.ordered.push(request);
            }
        } else if !self.ordered.contains(&request) {
            self.ordered.push(request);
            if self.ordered.len() > 16 {
                self.membership = Some(self.ordered.iter().copied().collect());
            }
        }
    }
}

/// Validate only declarations. Never inspect application code or spawn a factory.
pub(super) fn validate_policy_ports<'a>(
    ports: impl Iterator<Item = &'a str>,
    policy: &Readiness,
) -> Result<(), ReadinessError> {
    match policy.declaration().rule.as_ref() {
        Some(SelectionRule::AtLeast(n)) => {
            let count = ports.collect::<BTreeSet<_>>().len();
            if n.get() > count {
                return Err(ReadinessError::Threshold {
                    required: n.get(),
                    ports: count,
                });
            }
        }
        Some(SelectionRule::Optional { required, optional }) => {
            let names: BTreeSet<&str> = ports.collect();
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

    /// Both execution and passive inspection evaluate the same rule.
    fn passive_selection(&self, snapshot: &ReadinessSnapshot) -> Option<Vec<String>> {
        let rule = self.declaration.rule.as_ref()?;
        Some(match rule {
            SelectionRule::Optional { required, optional } => {
                select_optional(required, optional, snapshot)
            }
            rule => select_available(
                packet_requirement(rule, snapshot.heads.len()).expect("data-only rule"),
                snapshot
                    .heads
                    .iter()
                    .filter(|head| head.available)
                    .map(|head| head.port.clone()),
            ),
        })
    }
}

// The data-only rules require the first N available ports in selection order.
// Keep the rule independent of whether a caller needs names or queue indices.
fn packet_requirement(rule: &SelectionRule, port_count: usize) -> Option<usize> {
    match rule {
        SelectionRule::Any => Some(1),
        SelectionRule::All => Some(port_count),
        SelectionRule::AtLeast(n) => Some(n.get()),
        SelectionRule::Optional { .. } => None,
    }
}

fn select_available<T>(required: usize, available: impl Iterator<Item = T>) -> Vec<T> {
    let selected: Vec<_> = available.take(required).collect();
    if selected.len() == required {
        selected
    } else {
        vec![]
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
        self.passive_selection(snapshot).unwrap_or_else(|| {
            self.implementation()
                .expect("opaque readiness has an implementation")
                .select(snapshot)
        })
    }

    fn closure_ports(&self, lane: usize) -> Vec<String> {
        self.implementation()
            .map_or_else(Vec::new, |policy| policy.closure_ports(lane))
    }

    fn notification_driven(&self) -> bool {
        self.implementation()
            .is_some_and(|policy| policy.notification_driven())
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
            // A present packet resolves this reservation regardless of closure.
            // Only an absent packet needs the upstream absence proof.
            let closed = queue.is_none() && self.input_closed_for(index, port, p.request);
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
        // A staged continuation already owns a request. Scanning every input
        // queue and every sealed scope cannot change that candidate.
        if p.handoff.state() == HandoffState::WaitingInput {
            return vec![p.request];
        }
        let policy = p.waiting_policy.as_ref().unwrap_or(&p.policy);
        let correlated = policy.correlated();
        let mut candidates = RequestCandidates::default();
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
                    if position == 0 || request.is_some() {
                        candidates.insert(request);
                    }
                }
            }
        }
        // Only base policies declaring zero-packet closure obligations can
        // admit work from a sealed scope without a packet. Opaque policies may
        // declare such obligations per lane at admission, so retain their scan.
        // Data-only rules and Optional with a required input cannot use it.
        if p.policy.opaque() || p.policy.has_zero_packet_closure() {
            for (request, scope) in &self.requests {
                if scope.sealed && self.closure_obligation(index, *request) {
                    candidates.insert(Some(*request));
                }
            }
        }
        let mut candidates = candidates.ordered;
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
        // Base declarations are validated at bootstrap and staged
        // declarations when their WaitInputs step is accepted.
        if p.waiting.is_some() {
            return self.exact_wait_selection(index);
        }
        let order = self.selection_order(index);
        let policy = p.waiting_policy.as_ref().unwrap_or(&p.policy);
        // Idle data-only rules need neither named snapshots nor closure
        // proofs. Reserve actual queue positions using the same rule as
        // passive selection. Staged, optional and opaque policies still
        // receive full snapshots, including closure and correlation.
        let required = if p.handoff.state() == HandoffState::WaitingInput {
            None
        } else {
            policy
                .declaration()
                .rule
                .as_ref()
                .and_then(|rule| packet_requirement(rule, order.len()))
        };
        for request in self.selection_candidates(index, &order) {
            if let Some(required) = required {
                let ports = select_available(
                    required,
                    order.iter().filter_map(|&port| {
                        self.packet_for(index, port, request, policy.correlated())
                            .map(|queue| (port, Some(queue)))
                    }),
                );
                if !ports.is_empty() {
                    return Ok(Reservation { ports, request });
                }
                continue;
            }
            let mut snapshot = self.selection_snapshot(index, &order, request);
            if let Some(error) = self.staged_selection_error(index, &snapshot) {
                return Err(error);
            }
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

#[cfg(test)]
mod candidate_tests {
    use super::*;

    #[test]
    fn candidate_membership_preserves_first_discovery_across_promotion() {
        let request = |sequence| {
            Some(RequestId {
                session: 1,
                generation: 2,
                sequence,
            })
        };
        let mut candidates = RequestCandidates::default();
        let mut reference = vec![];
        for candidate in [None, request(99), None]
            .into_iter()
            .chain((0..15).rev().map(request))
            .chain([request(99), None])
            .chain((0..128).map(request))
            .chain((0..128).rev().map(request))
        {
            if !reference.contains(&candidate) {
                reference.push(candidate);
            }
            candidates.insert(candidate);
            assert_eq!(candidates.ordered, reference);
            assert_eq!(candidates.membership.is_some(), reference.len() > 16);
        }
    }
}

#[cfg(test)]
mod packet_only_selection_tests {
    use super::*;

    #[test]
    fn data_only_selection_preserves_rotated_order_and_threshold() {
        for cursor in 0..4 {
            let order: Vec<_> = (0..4).map(|offset| (cursor + offset) % 4).collect();
            for mask in 0..16 {
                let available: Vec<_> = order
                    .iter()
                    .copied()
                    .filter(|port| mask & (1 << port) != 0)
                    .collect();
                for required in 0..=4 {
                    let expected = if available.len() >= required {
                        available[..required].to_vec()
                    } else {
                        vec![]
                    };
                    assert_eq!(
                        select_available(
                            required,
                            order.iter().copied().filter(|port| mask & (1 << port) != 0),
                        ),
                        expected
                    );
                }
            }
        }
    }

    #[test]
    fn any_stops_after_first_available_port() {
        let available = std::iter::once(7).chain(std::iter::from_fn(|| {
            panic!("Any must not inspect a later available port")
        }));
        assert_eq!(select_available(1, available), vec![7]);
    }

    #[test]
    fn data_only_rules_ignore_closure_for_every_three_port_snapshot() {
        let policies = [
            Readiness::ANY,
            Readiness::ALL,
            Readiness::CORRELATED_ALL,
            Readiness::at_least(NonZeroUsize::new(2).unwrap()),
        ];
        for policy in policies {
            for flags in 0..64 {
                let full = ReadinessSnapshot {
                    request: None,
                    correlation_lane: None,
                    heads: (0..3)
                        .map(|i| InputHead {
                            port: format!("p{i}"),
                            available: flags & (1 << i) != 0,
                            closed: flags & (1 << (i + 3)) != 0,
                        })
                        .collect(),
                };
                let mut packet_only = full.clone();
                for head in &mut packet_only.heads {
                    head.closed = false;
                }
                assert_eq!(
                    policy.passive_selection(&full),
                    policy.passive_selection(&packet_only)
                );
            }
        }
    }
}

#[cfg(test)]
mod policy_validation_tests {
    use super::*;

    #[test]
    fn unconstrained_policies_do_not_enumerate_input_names() {
        struct Opaque;

        impl ReadinessPolicy for Opaque {
            fn select(&self, _: &ReadinessSnapshot) -> Vec<String> {
                vec![]
            }
        }

        for policy in [
            Readiness::ANY,
            Readiness::ALL,
            Readiness::CORRELATED_ALL,
            Readiness::from_policy(Opaque),
        ] {
            let ports = std::iter::from_fn(|| -> Option<&'static str> {
                panic!("policy has no input-name constraints")
            });
            assert_eq!(validate_policy_ports(ports, &policy), Ok(()));
        }
    }

    #[test]
    fn constrained_policies_still_validate_distinct_inputs_and_names() {
        assert_eq!(
            validate_policy_ports(
                ["a", "a"].into_iter(),
                &Readiness::at_least(NonZeroUsize::new(2).unwrap()),
            ),
            Err(ReadinessError::Threshold {
                required: 2,
                ports: 1,
            })
        );
        assert_eq!(
            validate_policy_ports(
                ["a"].into_iter(),
                &Readiness::optional(vec!["a".into()], vec!["b".into()]),
            ),
            Err(ReadinessError::InvalidOptional { port: "b".into() })
        );
    }
}
