//! Wave execution owns admission, reservations, activity/color tracking and settlement.
//! Compiled topology and port policies are borrowed inputs; trace data is observational.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{
    BufferMode, Candidate, CohortId, CohortMode, ColorFact, ColorOutcome, ColorState,
    CompiledGraph, ContractValue, DeliveryId, Edge, EdgeId, Emission, Graph, IncompleteGroup,
    InputAddr, InvocationArgs, InvocationCtx, InvocationId, NodeAction, NodeId, NodeLogic,
    OutputAddr, PartitionId, PartitionIndex, Port, PortId, PortKey, PortSnapshot, Reachability,
    ReservationRecord, SelectedValues, SlotSnapshot, Store, TriggerId, Value, WaveId, output,
};

#[derive(Clone, Debug)]
struct Delivery {
    id: DeliveryId,
    wave: WaveId,
    cohort: CohortId,
    value: Value,
}
struct Invocation {
    id: InvocationId,
    wave: WaveId,
    node: NodeId,
    port: Option<PortId>,
    cohort: CohortId,
    args: InvocationArgs,
}
struct Incoming {
    to: InputAddr,
    cohort: CohortId,
    value: Value,
}

#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WaveError {
    /// The wave needed another dispatch after consuming its activation budget.
    BudgetExhausted {
        max_activations: usize,
        activations: usize,
    },
    SlotOccupied(InputAddr),
    WrongSchema(InputAddr),
    InvalidSelection(NodeId, PortId),
    InvalidArgs(NodeId, PortId, CohortId, String),
    NodeFailed(NodeId, String),
    /// Edge adapter failure is distinct from the node's domain action failing.
    EdgeFailed {
        edge: EdgeId,
        from: OutputAddr,
        to: InputAddr,
        reason: String,
    },
    InvalidOutput(NodeId, String),
    OutputRoutingFailed {
        from: OutputAddr,
        reason: String,
    },
    Incomplete {
        node: NodeId,
        port: PortId,
        group: IncompleteGroup,
    },
}
#[derive(Debug)]
struct Published {
    from: OutputAddr,
    cohort: CohortId,
    value: Value,
}
/// Publication identity remains observable without retaining its payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicationEvent {
    pub from: OutputAddr,
    pub cohort: CohortId,
}
/// A committed, compiler-owned edge delivery. Unlike a publication, this
/// records a value that actually reached a selected input after the entire
/// fan-out batch passed type, capacity and adapter checks.
/// The report is observational; scheduling never reads this list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeliveryEvent {
    pub edge: EdgeId,
    pub from: OutputAddr,
    pub to: InputAddr,
    pub cohort: CohortId,
}
/// Choose whether a wave report keeps output payloads alive after delivery.
/// `MetadataOnly` is suitable for zero-copy local pipelines; `RetainPayloads`
/// preserves the original inspection behavior for conformance and debugging.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ReportRetention {
    /// No payloads, only source/cohort metadata.
    MetadataOnly,
    /// Keep outputs that delivered to no downstream edges *in this wave*.
    /// A routed output that skips every edge is terminal for this emission.
    TerminalOutputs,
    /// Original full-wave payload inspection (may force owned consumer cloning).
    #[default]
    RetainPayloads,
}
pub struct PublicationRef<'a> {
    publication: &'a Published,
}
impl PublicationRef<'_> {
    pub fn source_node_id(&self) -> u16 {
        self.publication.from.node.0
    }
    pub fn source_slot_id(&self) -> u16 {
        self.publication.from.slot.0
    }
    pub fn cohort_id(&self) -> u64 {
        self.publication.cohort.0
    }
    pub fn value<T: ContractValue>(&self) -> Result<T, String> {
        self.publication.value.get()
    }
    pub fn shared<T: Send + Sync + 'static>(&self) -> Result<Arc<T>, String> {
        self.publication.value.shared::<T>()
    }
    pub fn borrow<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.publication.value.borrow::<T>()
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct ColorWait {
    node: NodeId,
    port: PortId,
    cohort: CohortId,
    upstream: BTreeSet<NodeId>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
enum WaveEvent {
    Started(NodeId),
    Published(OutputAddr),
    Incomplete(NodeId, PortId),
}
/// Observational data: the scheduler never reads this to make readiness decisions.
#[derive(Debug, Default)]
pub struct WaveTrace {
    invocations: Vec<NodeId>,
    executed: Vec<NodeId>,
    waits: Vec<ColorWait>,
    timeline: Vec<WaveEvent>,
}
#[derive(Debug)]
pub struct WaveReport {
    wave: WaveId,
    activations: usize,
    published: Vec<Published>,
    publication_events: Vec<PublicationEvent>,
    delivery_events: Vec<DeliveryEvent>,
    retention: ReportRetention,
    cancelled: bool,
    errors: Vec<WaveError>,
    trace: WaveTrace,
}
impl WaveReport {
    pub fn success(&self) -> bool {
        !self.cancelled && self.errors.is_empty()
    }
    pub fn cancelled(&self) -> bool {
        self.cancelled
    }
    pub fn budget_exhausted(&self) -> bool {
        self.errors
            .iter()
            .any(|error| matches!(error, WaveError::BudgetExhausted { .. }))
    }
    /// Number of issued execution tickets, including failed or cancelled work.
    pub fn activation_count(&self) -> usize {
        self.activations
    }
    pub fn wave_id(&self) -> u64 {
        self.wave.0
    }
    pub fn invocation_count(&self, node: u16) -> usize {
        self.count(node)
    }
    fn count(&self, node: u16) -> usize {
        self.trace
            .invocations
            .iter()
            .filter(|&&id| id == NodeId(node))
            .count()
    }
    pub fn error_count(&self) -> usize {
        self.errors.len()
    }
    pub fn errors(&self) -> &[WaveError] {
        &self.errors
    }
    pub fn error_messages(&self) -> Vec<String> {
        self.errors
            .iter()
            .map(|error| format!("{error:?}"))
            .collect()
    }
    pub fn publications(&self) -> impl Iterator<Item = PublicationRef<'_>> {
        self.published
            .iter()
            .map(|publication| PublicationRef { publication })
    }
    /// Every successfully published output, even when payload retention is disabled.
    pub fn publication_events(&self) -> &[PublicationEvent] {
        &self.publication_events
    }
    /// Exact committed edge deliveries, even in metadata-only retention mode.
    /// A rejected fan-out batch contributes no delivery events.
    pub fn delivery_events(&self) -> &[DeliveryEvent] {
        &self.delivery_events
    }
    pub fn retention(&self) -> ReportRetention {
        self.retention
    }
}

/// Executor is an independent runtime adapter. The local implementation needs
/// no async runtime, process manager, or codec. A delayed executor can instead
/// use `ExecutionSession::next_ready` and `complete` at arbitrary times.
pub trait Executor: Send + Sync {
    fn dispatch(
        &self,
        action: &dyn NodeAction,
        ctx: InvocationCtx,
    ) -> Result<Vec<Emission>, String>;
}
/// A synchronous, value-transforming edge adapter. This interface is only
/// suitable for side-effect-free preparation (e.g. local value conversion).
/// It is NOT a reliable network transport: it has no acknowledgment, retry,
/// delivery ledger or atomic rollback across a fan-out batch. An external
/// transport requires a separate asynchronous delivery protocol.
/// Pure, synchronous edge value preparation. An adapter may not perform
/// external delivery; success means only that a value was prepared locally.
pub trait EdgeAdapter: Send + Sync {
    fn prepare(&self, value: &Value) -> Result<Value, String>;
}
pub struct IdentityEdgeAdapter;
impl EdgeAdapter for IdentityEdgeAdapter {
    fn prepare(&self, value: &Value) -> Result<Value, String> {
        Ok(value.clone())
    }
}
type EdgeRoutes = BTreeMap<(OutputAddr, InputAddr), Arc<dyn EdgeAdapter>>;

pub struct LocalExecutor;
impl Executor for LocalExecutor {
    fn dispatch(
        &self,
        action: &dyn NodeAction,
        ctx: InvocationCtx,
    ) -> Result<Vec<Emission>, String> {
        action.execute(ctx)
    }
}

/// A ticket represents one admitted invocation; it is not the node identity.
/// Completing the same ticket twice is an error, even if the node can run again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutionTicket {
    engine: u64,
    wave: WaveId,
    id: InvocationId,
    node: NodeId,
    port: Option<PortId>,
    cohort: CohortId,
}
impl ExecutionTicket {
    pub fn node_id(self) -> u16 {
        self.node.get()
    }
    pub fn invocation_id(self) -> u64 {
        self.id.0
    }
    pub fn cohort_id(self) -> u64 {
        self.cohort.0
    }
}
pub struct ReadyInvocation {
    ticket: ExecutionTicket,
    context: InvocationCtx,
}
impl ReadyInvocation {
    pub fn into_parts(self) -> (ExecutionTicket, InvocationCtx) {
        (self.ticket, self.context)
    }
}

/// Externally drive one wave, optionally completing dispatched work later.
/// The same WaveState owns admission, reservations, coloring and settlement.
pub struct ExecutionSession<'e> {
    engine: &'e Engine,
    state: WaveState<'e>,
    cancellation: WaveCancellation,
    cancelled_tickets: Vec<ExecutionTicket>,
    max_activations: usize,
}
/// A cloneable, thread-safe request handle for one wave. Requesting cancellation
/// never executes a node or interrupts a running action; the wave driver observes
/// the request at its next scheduling/completion boundary.
#[derive(Clone, Debug)]
pub struct WaveCancellation {
    wave: u64,
    requested: Arc<std::sync::atomic::AtomicBool>,
}
impl WaveCancellation {
    pub fn wave_id(&self) -> u64 {
        self.wave
    }
    pub fn request(&self) {
        self.requested.store(true, Ordering::Release);
    }
    pub fn requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
}
impl<'e> ExecutionSession<'e> {
    pub fn wave_id(&self) -> u64 {
        self.state.report.wave.0
    }
    pub fn cancellation(&self) -> WaveCancellation {
        self.cancellation.clone()
    }
    /// Stop admitting and publishing work in this wave, discard buffered input,
    /// and invalidate every issued ticket. Returned tickets identify work whose
    /// external runner MUST be asked to stop; this cannot roll back effects.
    /// Repeated cancellation is idempotent and returns the same ticket list.
    pub fn cancel(&mut self) -> &[ExecutionTicket] {
        self.cancellation.request();
        if !self.state.report.cancelled {
            self.cancelled_tickets = self.state.activity.dispatched.values().copied().collect();
            self.state.report.cancelled = true;
            self.discard_pending();
        }
        &self.cancelled_tickets
    }
    fn discard_pending(&mut self) {
        self.state.ready.clear();
        self.state.buffers.clear();
        self.state.reservations.clear();
        self.state.rejected_cohorts.clear();
        self.state.failed_ports.clear();
        self.state.settled_colors.clear();
        self.state.activity = WaveActivity::default();
    }
    fn observe_cancel(&mut self) {
        if self.cancellation.requested() {
            self.cancel();
        }
    }
    pub fn next_ready(&mut self) -> Option<ReadyInvocation> {
        self.observe_cancel();
        if self.state.report.cancelled {
            return None;
        }
        if !self.state.ready.is_empty()
            && self.state.report.activation_count() >= self.max_activations
        {
            self.state.report.errors.push(WaveError::BudgetExhausted {
                max_activations: self.max_activations,
                activations: self.state.report.activation_count(),
            });
            self.discard_pending();
            return None;
        }
        let invocation = self.state.ready.pop_front()?;
        self.state.report.activations += 1;
        self.state.report.trace.executed.push(invocation.node);
        self.state
            .report
            .trace
            .timeline
            .push(WaveEvent::Started(invocation.node));
        let n = self
            .engine
            .graph
            .nodes
            .get(&invocation.node)
            .expect("validated node");
        let ticket = ExecutionTicket {
            engine: self.engine.identity,
            wave: invocation.wave,
            id: invocation.id,
            node: invocation.node,
            port: invocation.port,
            cohort: invocation.cohort,
        };
        assert!(
            self.state
                .activity
                .dispatched
                .insert(ticket.id, ticket)
                .is_none(),
            "invocation dispatched once"
        );
        let context = InvocationCtx {
            wave: invocation.wave,
            invocation: invocation.id,
            cohort: invocation.cohort,
            from_port: invocation.port,
            args: invocation.args,
            store: Arc::clone(&self.engine.store),
            grants: n.grants.clone(),
        };
        Some(ReadyInvocation { ticket, context })
    }
    pub fn complete(
        &mut self,
        ticket: ExecutionTicket,
        result: Result<Vec<Emission>, String>,
    ) -> Result<(), String> {
        self.observe_cancel();
        if self.state.report.cancelled {
            return Err("wave is cancelled; completion rejected".into());
        }
        if self.state.report.budget_exhausted() {
            return Err("wave activation budget exhausted; completion rejected".into());
        }
        if ticket.engine != self.engine.identity || ticket.wave != self.state.report.wave {
            return Err("ticket belongs to a different engine or wave".into());
        }
        if self.state.activity.dispatched.get(&ticket.id) != Some(&ticket) {
            return Err("unknown or already completed ticket".into());
        }
        let region = self
            .engine
            .graph
            .structural()
            .partitions
            .region(ticket.node);
        let still_admitted = self
            .state
            .activity
            .producers
            .get(&region)
            .and_then(|producers| producers.get(&ticket.id))
            .is_some_and(|active| active.node == ticket.node && active.cohort == ticket.cohort);
        if !still_admitted {
            return Err("ticket does not match an active invocation".into());
        }
        self.state.activity.dispatched.remove(&ticket.id);
        self.state.publish_via(
            Completion {
                id: ticket.id,
                node: ticket.node,
                port: ticket.port,
                cohort: ticket.cohort,
                result,
            },
            &self.engine.adapters,
        );
        self.state.activity.finished(
            ticket.id,
            ticket.node,
            &self.engine.graph.structural().partitions,
        );
        self.state.reevaluate_after_completion(ticket.node);
        Ok(())
    }
    /// Reports whether all dispatched work and ready activations are settled.
    /// Incomplete input groups are handled by the existing settlement logic.
    pub fn can_finish(&self) -> bool {
        self.cancellation.requested()
            || (self.state.activity.dispatched.is_empty() && self.state.ready.is_empty())
    }
    pub fn in_flight_count(&self) -> usize {
        self.state.activity.dispatched.len()
    }
    pub fn finish(mut self) -> Result<WaveReport, String> {
        self.observe_cancel();
        if !self.can_finish() {
            return Err("wave has uncompleted or ready invocations".into());
        }
        if !self.state.report.cancelled && !self.state.report.budget_exhausted() {
            self.state.settle();
        }
        Ok(self.state.report)
    }
    pub fn action(&self, ticket: ExecutionTicket) -> Option<&dyn NodeAction> {
        if self.cancellation.requested() || self.state.report.cancelled {
            return None;
        }
        if ticket.engine != self.engine.identity
            || ticket.wave != self.state.report.wave
            || self.state.activity.dispatched.get(&ticket.id) != Some(&ticket)
        {
            return None;
        }
        self.engine
            .instances
            .get(&ticket.node)
            .map(|action| action.as_ref())
    }
}

/// One trigger argument interface shared by `Engine::fire` and `Engine::begin`.
/// Typed handles and stable names are resolved by the same compiled graph.
pub trait TriggerSelector {
    fn resolve(self, graph: &CompiledGraph) -> Result<TriggerId, String>;
}
impl TriggerSelector for TriggerId {
    fn resolve(self, _graph: &CompiledGraph) -> Result<TriggerId, String> {
        Ok(self)
    }
}
impl TriggerSelector for u16 {
    fn resolve(self, _graph: &CompiledGraph) -> Result<TriggerId, String> {
        Ok(TriggerId(self))
    }
}
// Bare integer literals at existing low-level call sites infer i32. Reject
// negative/out-of-range IDs explicitly; never truncate or wrap a trigger.
impl TriggerSelector for i32 {
    fn resolve(self, _graph: &CompiledGraph) -> Result<TriggerId, String> {
        u16::try_from(self)
            .map(TriggerId)
            .map_err(|_| format!("invalid trigger ID {self}"))
    }
}
impl TriggerSelector for &str {
    fn resolve(self, graph: &CompiledGraph) -> Result<TriggerId, String> {
        graph
            .trigger(self)
            .ok_or_else(|| format!("unknown trigger {self:?}"))
    }
}
impl TriggerSelector for String {
    fn resolve(self, graph: &CompiledGraph) -> Result<TriggerId, String> {
        self.as_str().resolve(graph)
    }
}
static NEXT_ENGINE_ID: AtomicU64 = AtomicU64::new(1);
/// Default per-wave dispatch bound, including productive cycles in agent graphs.
pub const DEFAULT_MAX_ACTIVATIONS: usize = 1024;
pub struct Engine {
    identity: u64,
    graph: Arc<Graph>,
    store: Arc<Store>,
    next_wave: AtomicU64,
    instances: BTreeMap<NodeId, Arc<NodeLogic>>,
    executor: Arc<dyn Executor>,
    node_executors: BTreeMap<NodeId, Arc<dyn Executor>>,
    adapters: EdgeRoutes,
    report_retention: ReportRetention,
    max_activations: usize,
}
impl Engine {
    /// Resolve a named trigger's physical inputs without admitting a wave.
    /// Each outer entry corresponds to one supplied payload; its inner entries
    /// are all destinations receiving that payload, including module fan-out.
    /// Source starts consume no payload and therefore add no entries here.
    pub fn trigger_targets(&self, name: &str) -> Option<&[Vec<InputAddr>]> {
        let id = self.graph.trigger(name)?;
        self.graph.triggers.get(&id).map(Vec::as_slice)
    }

    /// Find physical nodes using this exact action allocation in this deployment.
    /// Shared dynamic actions may match multiple nodes; factory-created actions
    /// match only their actual instance. Does not invoke actions or factories.
    pub fn node_ids_for_action(&self, action: &Arc<dyn NodeAction>) -> Vec<u16> {
        self.instances
            .iter()
            .filter_map(|(id, instance)| Arc::ptr_eq(instance, action).then_some(id.get()))
            .collect()
    }

    pub(super) fn new(graph: Arc<CompiledGraph>, store: Store) -> Self {
        let instances = graph
            .nodes
            .iter()
            .map(|(&id, node)| (id, (node.logic_factory)()))
            .collect();
        Self {
            identity: NEXT_ENGINE_ID.fetch_add(1, Ordering::Relaxed),
            graph,
            store: Arc::new(store),
            next_wave: AtomicU64::new(1),
            instances,
            executor: Arc::new(LocalExecutor),
            node_executors: BTreeMap::new(),
            adapters: EdgeRoutes::new(),
            report_retention: ReportRetention::RetainPayloads,
            max_activations: DEFAULT_MAX_ACTIVATIONS,
        }
    }
    /// Bound execution tickets issued by each independent wave. Zero disables
    /// dispatch. Exhaustion discards pending work and invalidates issued tickets,
    /// but preserves prior publications. This is not a time, memory, or emission
    /// bound and cannot preempt an action or roll back its external effects.
    pub fn with_max_activations(mut self, max_activations: usize) -> Self {
        self.max_activations = max_activations;
        self
    }
    /// Select the payload retention policy without modifying scheduling.
    /// In `MetadataOnly` mode `WaveReport::publications()` is empty;
    /// `TerminalOutputs` retains only outputs without outgoing edges.
    /// `publication_events()` always retains every output's source and cohort.
    pub(super) fn with_report_retention(mut self, retention: ReportRetention) -> Self {
        self.report_retention = retention;
        self
    }
    /// Override how actions execute without changing the compiled graph.
    pub(super) fn with_executor(mut self, executor: Arc<dyn Executor>) -> Self {
        self.executor = executor;
        self
    }
    pub(super) fn with_node_executor(
        mut self,
        node: u16,
        executor: Arc<dyn Executor>,
    ) -> Result<Self, String> {
        if !self.graph.nodes.contains_key(&NodeId(node)) {
            return Err("unknown executor node".into());
        }
        if self.node_executors.contains_key(&NodeId(node)) {
            return Err(format!("node {node} already has an executor"));
        }
        self.node_executors.insert(NodeId(node), executor);
        Ok(self)
    }
    /// Register a pure edge adapter. Never use this for network side effects.
    pub(super) fn with_edge_adapter(
        mut self,
        from: OutputAddr,
        to: InputAddr,
        adapter: Arc<dyn EdgeAdapter>,
    ) -> Result<Self, String> {
        if !self
            .graph
            .by_output
            .get(&from)
            .is_some_and(|edges| edges.iter().any(|edge| edge.to == to))
        {
            return Err("cannot configure adapter for a nonexistent edge".into());
        }
        if self.adapters.contains_key(&(from, to)) {
            return Err("duplicate edge adapter configuration".into());
        }
        self.adapters.insert((from, to), adapter);
        Ok(self)
    }
    pub fn fire<T: TriggerSelector>(
        &self,
        trigger: T,
        values: Vec<Value>,
    ) -> Result<WaveReport, String> {
        let id = trigger.resolve(&self.graph)?;
        self.run_sources(self.trigger_inputs(id, values)?, self.source_targets(id))
    }
    /// Begin the same scheduler-driven wave without dispatching actions yet.
    /// The trigger accepts either an authoring handle or its public name.
    pub fn begin<T: TriggerSelector>(
        &self,
        trigger: T,
        values: Vec<Value>,
    ) -> Result<ExecutionSession<'_>, String> {
        let id = trigger.resolve(&self.graph)?;
        self.start_incoming(self.trigger_inputs(id, values)?, self.source_targets(id))
    }
    fn source_targets(&self, id: TriggerId) -> &[NodeId] {
        self.graph
            .source_triggers
            .get(&id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
    fn trigger_inputs(&self, id: TriggerId, values: Vec<Value>) -> Result<Vec<Incoming>, String> {
        let targets = self.graph.triggers.get(&id).ok_or("unknown trigger")?;
        if targets.len() != values.len() {
            return Err("trigger arity mismatch".into());
        }
        Ok(targets
            .iter()
            .zip(values)
            .flat_map(|(addresses, value)| {
                addresses.iter().map(move |&to| Incoming {
                    to,
                    value: value.clone(),
                    cohort: CohortId(0),
                })
            })
            .collect())
    }
    fn start_incoming(
        &self,
        incoming: Vec<Incoming>,
        sources: &[NodeId],
    ) -> Result<ExecutionSession<'_>, String> {
        // Preflight entire trigger event: invalid ingress cannot partially start.
        let mut counts: BTreeMap<InputAddr, usize> = BTreeMap::new();
        for x in &incoming {
            let spec = self.graph.input_slot(x.to)?;
            if spec.identity() != x.value.identity() {
                return Err(format!("wrong trigger type identity at {:?}", x.to));
            }
            *counts.entry(x.to).or_default() += 1;
            if spec.buffer == BufferMode::Immediate && counts[&x.to] > 1 {
                return Err(format!("duplicate immediate trigger target {:?}", x.to));
            }
        }
        let next_cohort = incoming
            .iter()
            .map(|delivery| delivery.cohort.0)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("cohort ID capacity exhausted")?;
        let wave = WaveId(self.next_wave.fetch_add(1, Ordering::Relaxed));
        let mut state = WaveState::new(&self.graph, wave);
        state.report.retention = self.report_retention;
        state.next_cohort = next_cohort;
        for &node in sources {
            let invocation = Invocation {
                id: InvocationId(state.next_invocation),
                wave,
                node,
                port: None,
                cohort: CohortId(0),
                args: InvocationArgs::Raw(BTreeMap::new()),
            };
            state.next_invocation += 1;
            state.report.trace.invocations.push(node);
            state.activity.admitted(
                ActiveInvocation {
                    id: invocation.id,
                    node,
                    cohort: invocation.cohort,
                },
                &self.graph.structural().partitions,
            );
            state.ready.push_back(invocation);
        }
        assert!(
            state.deliver_batch(incoming),
            "preflighted ingress must be valid"
        );
        Ok(ExecutionSession {
            engine: self,
            state,
            cancellation: WaveCancellation {
                wave: wave.0,
                requested: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            },
            cancelled_tickets: Vec::new(),
            max_activations: self.max_activations,
        })
    }
    #[cfg(test)]
    fn run(&self, incoming: Vec<Incoming>) -> Result<WaveReport, String> {
        self.run_sources(incoming, &[])
    }
    fn run_sources(
        &self,
        incoming: Vec<Incoming>,
        sources: &[NodeId],
    ) -> Result<WaveReport, String> {
        let mut session = self.start_incoming(incoming, sources)?;
        while let Some(ready) = session.next_ready() {
            let (ticket, ctx) = ready.into_parts();
            let Some(action) = session.action(ticket) else {
                if session.cancellation.requested() {
                    session.cancel();
                    break;
                }
                return Err("dispatched node action became unavailable".into());
            };
            if session.cancellation.requested() {
                session.cancel();
                break;
            }
            let executor = self
                .node_executors
                .get(&ticket.node)
                .unwrap_or(&self.executor);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                executor.dispatch(action, ctx)
            }))
            .unwrap_or_else(|_| Err("node panicked".to_string()));
            // Synchronous actions are not preemptible; suppress their results
            // when the wave was cancelled during dispatch.
            if session.cancellation.requested() {
                session.cancel();
                break;
            }
            if let Err(error) = session.complete(ticket, result) {
                if session.state.report.cancelled {
                    break;
                }
                return Err(error);
            }
        }
        session.finish()
    }
}
struct Completion {
    id: InvocationId,
    node: NodeId,
    port: Option<PortId>,
    cohort: CohortId,
    result: Result<Vec<Emission>, String>,
}

/// Derived node state. Queued invocations count as Executing because their
/// input deliveries have already been reserved, even before CPU dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeState {
    Dormant,
    InputPending,
    Executing,
}

#[derive(Clone, Copy, Debug)]
struct ActiveInvocation {
    id: InvocationId,
    node: NodeId,
    cohort: CohortId,
}

/// Scheduling indexes and completion authority, without diagnostic aggregates.
#[derive(Default)]
struct WaveActivity {
    // All admission/progress facts are owned here, including issued tickets.
    dispatched: BTreeMap<InvocationId, ExecutionTicket>,
    pending_ports: BTreeMap<PortKey, usize>, // number of nonempty input slots
    pending_by_partition: BTreeMap<PartitionId, BTreeSet<PortKey>>,
    producers: BTreeMap<PartitionId, BTreeMap<InvocationId, ActiveInvocation>>,
}
impl WaveActivity {
    fn input_occupied(&mut self, addr: InputAddr, partitions: &PartitionIndex) {
        let port = (addr.node, addr.port);
        let count = self.pending_ports.entry(port).or_default();
        if *count == 0 {
            self.pending_by_partition
                .entry(partitions.region(addr.node))
                .or_default()
                .insert(port);
        }
        *count += 1;
    }
    fn input_emptied(&mut self, addr: InputAddr, partitions: &PartitionIndex) {
        let port = (addr.node, addr.port);
        let count = self
            .pending_ports
            .get_mut(&port)
            .expect("previously nonempty port");
        *count -= 1;
        if *count == 0 {
            self.pending_ports.remove(&port);
            let region = partitions.region(addr.node);
            let pending = self
                .pending_by_partition
                .get_mut(&region)
                .expect("pending port was indexed by partition");
            assert!(pending.remove(&port));
            if pending.is_empty() {
                self.pending_by_partition.remove(&region);
            }
        }
    }
    fn admitted(&mut self, invocation: ActiveInvocation, partitions: &PartitionIndex) {
        let region = partitions.region(invocation.node);
        assert!(
            self.producers
                .entry(region)
                .or_default()
                .insert(invocation.id, invocation)
                .is_none(),
            "invocation admitted only once"
        );
    }
    fn finished(&mut self, invocation: InvocationId, node: NodeId, partitions: &PartitionIndex) {
        let region = partitions.region(node);
        let producers = self
            .producers
            .get_mut(&region)
            .expect("admitted source region");
        assert!(
            producers.remove(&invocation).is_some(),
            "completion matches an admission"
        );
        if producers.is_empty() {
            self.producers.remove(&region);
        }
    }
}

#[derive(Clone, Debug)]
// Emission and delivery are distinct: output routing may skip any declared
// edge. Color facts therefore record the actual ingress targets, not merely
// emitted slot IDs (which would invent deliveries on skipped branches).
enum SettledOutcome {
    Emitted(BTreeSet<InputAddr>),
    Failed(String),
}
#[derive(Clone, Debug)]
struct SettledColor {
    invocation: InvocationId,
    node: NodeId,
    cohort: CohortId,
    outcome: SettledOutcome,
}
struct WaveState<'a> {
    graph: &'a Graph,
    next_delivery: u64,
    next_invocation: u64,
    next_cohort: u64,
    buffers: BTreeMap<InputAddr, VecDeque<Delivery>>,
    reservations: BTreeMap<(NodeId, PortId), Vec<ReservationRecord>>,
    rejected_cohorts: BTreeMap<(NodeId, PortId), BTreeSet<CohortId>>,
    failed_ports: BTreeSet<(NodeId, PortId)>,
    ready: VecDeque<Invocation>,
    activity: WaveActivity,
    settled_colors: BTreeMap<PartitionId, Vec<SettledColor>>,
    report: WaveReport,
}
impl<'a> WaveState<'a> {
    fn new(graph: &'a Graph, wave: WaveId) -> Self {
        Self {
            graph,
            next_delivery: 1,
            next_invocation: 1,
            next_cohort: 1,
            buffers: BTreeMap::new(),
            reservations: BTreeMap::new(),
            failed_ports: BTreeSet::new(),
            rejected_cohorts: BTreeMap::new(),
            ready: VecDeque::new(),
            activity: WaveActivity::default(),
            settled_colors: BTreeMap::new(),
            report: WaveReport {
                wave,
                activations: 0,
                published: Vec::new(),
                publication_events: Vec::new(),
                delivery_events: Vec::new(),
                retention: ReportRetention::RetainPayloads,
                cancelled: false,
                errors: Vec::new(),
                trace: WaveTrace::default(),
            },
        }
    }
    /// Batch is all-or-nothing, including capacity checking. No partial fan-out.
    fn deliver_batch(&mut self, batch: Vec<Incoming>) -> bool {
        let mut counts: BTreeMap<InputAddr, usize> = BTreeMap::new();
        for incoming in &batch {
            let spec = self
                .graph
                .input_slot(incoming.to)
                .expect("validated edge/trigger");
            if spec.identity() != incoming.value.identity() {
                self.report.errors.push(WaveError::WrongSchema(incoming.to));
                return false;
            }
            *counts.entry(incoming.to).or_default() += 1;
            if spec.buffer == BufferMode::Immediate
                && self.buffers.get(&incoming.to).map_or(0, |b| b.len()) + counts[&incoming.to] > 1
            {
                self.report
                    .errors
                    .push(WaveError::SlotOccupied(incoming.to));
                return false;
            }
        }
        let mut affected = BTreeSet::new();
        for incoming in batch {
            let delivery = Delivery {
                id: DeliveryId(self.next_delivery),
                wave: self.report.wave,
                cohort: incoming.cohort,
                value: incoming.value,
            };
            self.next_delivery += 1;
            let queue = self.buffers.entry(incoming.to).or_default();
            let was_empty = queue.is_empty();
            queue.push_back(delivery);
            if was_empty {
                self.activity
                    .input_occupied(incoming.to, &self.graph.structural().partitions);
            }
            affected.insert((incoming.to.node, incoming.to.port));
        }
        let mut ports: Vec<_> = affected.into_iter().collect();
        ports.sort_by_key(|(node, port)| (self.graph.order[node], *port));
        for (node, port) in ports {
            self.reserve_all(node, port);
        }
        true
    }
    fn snapshots(&self, node: NodeId, port: &Port) -> PortSnapshot {
        let slots: Vec<SlotSnapshot> = port
            .slots
            .iter()
            .map(|s| {
                let addr = InputAddr {
                    node,
                    port: port.id,
                    slot: s.id,
                };
                let rejected = self.rejected_cohorts.get(&(node, port.id));
                let candidates = self
                    .buffers
                    .get(&addr)
                    .into_iter()
                    .flatten()
                    .filter(|delivery| {
                        rejected.is_none_or(|cohorts| !cohorts.contains(&delivery.cohort))
                    })
                    .map(|d| Candidate {
                        slot: s.id,
                        delivery: d.id,
                        cohort: d.cohort,
                    })
                    .collect();
                SlotSnapshot {
                    slot: s.id,
                    name: s.name.clone(),
                    candidates,
                }
            })
            .collect();
        let rejected = self.rejected_cohorts.get(&(node, port.id));
        let reservations: Vec<ReservationRecord> = self
            .reservations
            .get(&(node, port.id))
            .into_iter()
            .flatten()
            .filter(|record| rejected.is_none_or(|cohorts| !cohorts.contains(&record.cohort)))
            .cloned()
            .collect();
        let cohorts: BTreeSet<_> = slots
            .iter()
            .flat_map(|s| s.candidates.iter().map(|c| c.cohort))
            .chain(reservations.iter().map(|r| r.cohort))
            .collect();
        let mut colors = BTreeMap::new();
        // The region quotient only filters candidates. Exact node/port paths
        // are always checked against the reverse reachability cache.
        let regions = self.graph.structural().partitions.upstream(node);
        for cohort in cohorts {
            let mut facts = Vec::new();
            for region in regions {
                for producer in self
                    .activity
                    .producers
                    .get(region)
                    .into_iter()
                    .flat_map(|m| m.values())
                {
                    let (same_cohort, new_cohort) =
                        self.graph.can_reach_port(producer.node, (node, port.id));
                    if (same_cohort && producer.cohort == cohort) || new_cohort {
                        facts.push(ColorFact {
                            invocation: producer.id,
                            node: producer.node,
                            state: ColorState::Open,
                        });
                    }
                }
                // Historical outcomes are policy observations, not active nodes.
                // Keep them wave-local, indexed by the source partition.
                for settled in self.settled_colors.get(region).into_iter().flatten() {
                    let (same_cohort, new_cohort) =
                        self.graph.can_reach_port(settled.node, (node, port.id));
                    if !((same_cohort && settled.cohort == cohort) || new_cohort) {
                        continue;
                    }
                    let outcome = match &settled.outcome {
                        SettledOutcome::Failed(message) => ColorOutcome::Failed(message.clone()),
                        SettledOutcome::Emitted(deliveries) => {
                            let reaches = self.emitted_reachability(deliveries, (node, port.id));
                            if settled.cohort == cohort
                                && self.emitted_directly_to_port(deliveries, (node, port.id))
                            {
                                ColorOutcome::Delivered
                            } else if (settled.cohort == cohort && reaches.preserve)
                                || reaches.reset
                            {
                                ColorOutcome::EmittedTowardReachablePort
                            } else {
                                ColorOutcome::Closed
                            }
                        }
                    };
                    facts.push(ColorFact {
                        invocation: settled.invocation,
                        node: settled.node,
                        state: ColorState::Terminal(outcome),
                    });
                }
            }
            // Preserve deterministic ordering independent of partition layout.
            facts.sort_by_key(|fact| fact.invocation);
            if !facts.is_empty() {
                colors.insert(cohort, facts);
            }
        }
        PortSnapshot {
            slots,
            reservations,
            colors,
        }
    }

    // These checks use the *selected* ingress targets, not all compiled
    // edges of an output slot. That distinction matters when an output chooses
    // zero edges or when a policy narrows a diamond to one branch.
    fn emitted_directly_to_port(&self, deliveries: &BTreeSet<InputAddr>, target: PortKey) -> bool {
        deliveries.iter().any(|to| (to.node, to.port) == target)
    }

    fn emitted_reachability(
        &self,
        deliveries: &BTreeSet<InputAddr>,
        target: PortKey,
    ) -> Reachability {
        let mut result = Reachability::default();
        for to in deliveries {
            if (to.node, to.port) == target {
                result.preserve = true;
            } else {
                let (preserve, reset) = self.graph.can_reach_port(to.node, target);
                result.preserve |= preserve;
                result.reset |= reset;
            }
            if result.preserve && result.reset {
                break;
            }
        }
        result
    }

    // Retained as a conservative reference operation for conformance tests.
    // Normal execution uses the completion-targeted variant below.
    #[cfg(test)]
    fn reevaluate_waiting(&mut self) {
        let ports: Vec<_> = self.activity.pending_ports.keys().copied().collect();
        for (node, port) in ports {
            self.reserve_all(node, port);
        }
    }

    /// Completing a producer can only change the reachable pending consumers'
    /// open/terminal colors. Cross-region targets are filtered first by the
    /// partition quotient and then by the exact cached node-to-port relation.
    fn reevaluate_after_completion(&mut self, source: NodeId) {
        let partitions = &self.graph.structural().partitions;
        let targets: Vec<_> = partitions
            .downstream(source)
            .iter()
            .flat_map(|region| {
                self.activity
                    .pending_by_partition
                    .get(region)
                    .into_iter()
                    .flat_map(|ports| ports.iter().copied())
            })
            .filter(|&target| {
                target.0 == source || {
                    let (preserve, reset) = self.graph.can_reach_port(source, target);
                    preserve || reset
                }
            })
            .collect();
        for (node, port) in targets {
            self.reserve_all(node, port);
        }
    }

    fn reserve_all(&mut self, node: NodeId, port_id: PortId) {
        if self.failed_ports.contains(&(node, port_id)) {
            return;
        }
        let port = self.graph.nodes[&node]
            .ports
            .iter()
            .find(|p| p.id == port_id)
            .expect("validated port");
        loop {
            let snapshots = self.snapshots(node, port);
            let Some(selection) = port.policy.select(&snapshots) else {
                for (cohort, upstream) in snapshots.open_upstream() {
                    if snapshots
                        .slots
                        .iter()
                        .any(|s| s.candidates.iter().any(|c| c.cohort == cohort))
                    {
                        let entry = ColorWait {
                            node,
                            port: port_id,
                            cohort,
                            upstream,
                        };
                        if !self.report.trace.waits.contains(&entry) {
                            self.report.trace.waits.push(entry);
                        }
                    }
                }
                break;
            };
            // Validate ALL picks before consuming any. Even a buggy custom
            // policy cannot steal a value or split an atomic reservation.
            let mut seen = BTreeSet::new();
            let valid = !selection.picks.is_empty()
                && selection.picks.iter().all(|pick| {
                    seen.insert(pick.slot)
                        && snapshots.slots.iter().any(|s| {
                            s.slot == pick.slot
                                && s.candidates.iter().any(|c| {
                                    c.delivery == pick.delivery && c.cohort == selection.cohort
                                })
                        })
                });
            if !valid {
                self.report
                    .errors
                    .push(WaveError::InvalidSelection(node, port_id));
                let cohort_isolatable = snapshots.slots.iter().any(|slot| {
                    slot.candidates
                        .iter()
                        .any(|candidate| candidate.cohort == selection.cohort)
                });
                if cohort_isolatable {
                    self.rejected_cohorts
                        .entry((node, port_id))
                        .or_default()
                        .insert(selection.cohort);
                    continue;
                }
                // An unidentifiable selection may have corrupted any cohort;
                // fail closed for this port rather than risk consuming work.
                self.failed_ports.insert((node, port_id));
                break;
            }
            let retired = port.policy.retire(&snapshots, &selection);
            let selected_deliveries: BTreeSet<_> = selection
                .picks
                .iter()
                .map(|pick| (pick.slot, pick.delivery))
                .collect();
            let mut retired_deliveries = BTreeSet::new();
            let valid_retirement = retired.iter().all(|pick| {
                !selected_deliveries.contains(&(pick.slot, pick.delivery))
                    && retired_deliveries.insert((pick.slot, pick.delivery))
                    && snapshots.slots.iter().any(|slot| {
                        slot.slot == pick.slot
                            && slot.candidates.iter().any(|candidate| {
                                candidate.delivery == pick.delivery
                                    && candidate.cohort == selection.cohort
                            })
                    })
            });
            if !valid_retirement {
                self.report
                    .errors
                    .push(WaveError::InvalidSelection(node, port_id));
                self.rejected_cohorts
                    .entry((node, port_id))
                    .or_default()
                    .insert(selection.cohort);
                continue;
            }
            // Policy only selects; the port-owned decoder/validator runs before
            // any delivery is consumed or reservation recorded.
            let prepared_result = {
                let selected_values: BTreeMap<_, _> = selection
                    .picks
                    .iter()
                    .map(|pick| {
                        let addr = InputAddr {
                            node,
                            port: port_id,
                            slot: pick.slot,
                        };
                        let value = &self.buffers[&addr]
                            .iter()
                            .find(|d| d.id == pick.delivery)
                            .expect("selection validated")
                            .value;
                        (pick.slot, value)
                    })
                    .collect();
                let named_values = SelectedValues {
                    selected: &selected_values,
                    slots: &port.slots,
                };
                if let Some(builder) = &port.args {
                    builder.prepare(&named_values)
                } else {
                    Ok(None)
                }
            };
            let prepared = match prepared_result {
                Ok(value) => value,
                Err(error) => {
                    self.report.errors.push(WaveError::InvalidArgs(
                        node,
                        port_id,
                        selection.cohort,
                        error,
                    ));
                    self.rejected_cohorts
                        .entry((node, port_id))
                        .or_default()
                        .insert(selection.cohort);
                    continue;
                }
            };
            let consumed_slots: BTreeSet<_> = selection.picks.iter().map(|p| p.slot).collect();
            let selected_cohort = selection.cohort;
            let mut raw_args = prepared.is_none().then(BTreeMap::new);
            for pick in selection.picks {
                let addr = InputAddr {
                    node,
                    port: port_id,
                    slot: pick.slot,
                };
                let q = self.buffers.get_mut(&addr).expect("selection checked");
                let index = q
                    .iter()
                    .position(|d| d.id == pick.delivery)
                    .expect("selection checked");
                let d = q.remove(index).expect("selection checked");
                let now_empty = q.is_empty();
                if now_empty {
                    self.activity
                        .input_emptied(addr, &self.graph.structural().partitions);
                }
                debug_assert_eq!(d.wave, self.report.wave);
                if let Some(args) = &mut raw_args {
                    args.insert(pick.slot, d.value);
                }
            }
            for pick in retired {
                let addr = InputAddr {
                    node,
                    port: port_id,
                    slot: pick.slot,
                };
                let q = self.buffers.get_mut(&addr).expect("retirement checked");
                let index = q
                    .iter()
                    .position(|delivery| delivery.id == pick.delivery)
                    .expect("retirement checked");
                q.remove(index);
                if q.is_empty() {
                    self.activity
                        .input_emptied(addr, &self.graph.structural().partitions);
                }
            }
            self.reservations
                .entry((node, port_id))
                .or_default()
                .push(ReservationRecord {
                    invocation: InvocationId(self.next_invocation),
                    cohort: selected_cohort,
                    slots: consumed_slots,
                    completed: false,
                });
            let args = match prepared {
                Some(typed) => InvocationArgs::Typed(typed),
                None => InvocationArgs::Raw(raw_args.expect("raw port arguments retained")),
            };
            let invocation = Invocation {
                id: InvocationId(self.next_invocation),
                wave: self.report.wave,
                node,
                port: Some(port_id),
                cohort: selected_cohort,
                args,
            };
            self.next_invocation += 1;
            self.report.trace.invocations.push(node);
            self.activity.admitted(
                ActiveInvocation {
                    id: invocation.id,
                    node: invocation.node,
                    cohort: invocation.cohort,
                },
                &self.graph.structural().partitions,
            );
            self.ready.push_back(invocation);
        }
    }
    #[cfg(test)]
    fn publish(&mut self, completion: Completion) {
        self.publish_via(completion, &EdgeRoutes::new());
    }
    fn publish_via(&mut self, completion: Completion, routes: &EdgeRoutes) {
        let n = &self.graph.nodes[&completion.node];
        let emissions = match completion.result {
            Ok(outputs) => outputs,
            Err(e) => {
                self.report
                    .errors
                    .push(WaveError::NodeFailed(completion.node, e.clone()));
                self.record_settled(
                    completion.id,
                    completion.node,
                    completion.cohort,
                    SettledOutcome::Failed(e),
                );
                return;
            }
        };
        // Executor results are untrusted. Validate the whole zero-or-one batch
        // before routing or publishing anything.
        if emissions.len() > 1
            || emissions.iter().any(|em| {
                em.slot.get() != 0
                    || em.slot != n.output.id
                    || n.output.identity() != em.value.identity()
            })
        {
            let message =
                "expected zero or one correctly typed emission at output slot 0".to_string();
            self.report
                .errors
                .push(WaveError::InvalidOutput(completion.node, message.clone()));
            self.record_settled(
                completion.id,
                completion.node,
                completion.cohort,
                SettledOutcome::Failed(message),
            );
            return;
        }
        let cohort = match n.cohort_mode {
            CohortMode::Inherit => completion.cohort,
            CohortMode::FreshPerInvocation => {
                let result = CohortId(self.next_cohort);
                let Some(next) = self.next_cohort.checked_add(1) else {
                    let message = "fresh cohort ID capacity exhausted".to_string();
                    self.report
                        .errors
                        .push(WaveError::NodeFailed(completion.node, message.clone()));
                    self.record_settled(
                        completion.id,
                        completion.node,
                        completion.cohort,
                        SettledOutcome::Failed(message),
                    );
                    return;
                };
                self.next_cohort = next;
                result
            }
        };
        let mut batch = Vec::new();
        let mut edge_deliveries = Vec::new();
        for em in &emissions {
            let from = OutputAddr {
                node: completion.node,
                slot: em.slot,
            };
            let candidates = self
                .graph
                .by_output
                .get(&from)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let candidate_ids: Vec<EdgeId> = candidates.iter().map(|edge| edge.id).collect();
            // An edge ID is looked up for every selected delivery. Indexing
            // once avoids repeated linear membership and endpoint scans when
            // fan-out is large, without changing the compiled edge ordering.
            let candidate_edges: BTreeMap<EdgeId, &Edge> =
                candidates.iter().map(|edge| (edge.id, edge)).collect();
            let selected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                match &em.selection {
                    Some(output::OutputSelection::All) => Ok(candidate_ids.clone()),
                    Some(output::OutputSelection::Targets(targets)) => {
                        self.graph.targets_for_output(from, targets, candidates)
                    }
                    Some(output::OutputSelection::Filter(filter)) => Ok(candidates
                        .iter()
                        .filter(|edge| filter(&self.graph.destination(**edge)))
                        .map(|edge| edge.id)
                        .collect()),
                    // A plain return is exactly Output::all(value).
                    None => Ok(candidate_ids.clone()),
                }
            }))
            .unwrap_or_else(|_| Err("output selector panicked".into()));
            let selected = match selected {
                Ok(edges) => edges,
                Err(error) => {
                    self.report.errors.push(WaveError::OutputRoutingFailed {
                        from,
                        reason: error.clone(),
                    });
                    self.record_settled(
                        completion.id,
                        completion.node,
                        cohort,
                        SettledOutcome::Failed(error),
                    );
                    return;
                }
            };
            let mut seen_edges = BTreeSet::new();
            for chosen in selected {
                if !candidate_edges.contains_key(&chosen) || !seen_edges.insert(chosen) {
                    let message =
                        format!("output selected unknown or duplicate edge {}", chosen.get());
                    self.report.errors.push(WaveError::OutputRoutingFailed {
                        from,
                        reason: message.clone(),
                    });
                    self.record_settled(
                        completion.id,
                        completion.node,
                        cohort,
                        SettledOutcome::Failed(message),
                    );
                    return;
                }
                let e = candidate_edges
                    .get(&chosen)
                    .copied()
                    .expect("validated selector edge exists");
                // The adapter is a synchronous *preparation* stage, not an
                // irreversible send. Catch panics so one misbehaving adapter
                // cannot bypass the invocation's settlement obligation.
                let value = match routes.get(&(from, e.to)) {
                    Some(adapter) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        adapter.prepare(&em.value)
                    }))
                    .unwrap_or_else(|_| Err("edge adapter panicked".into())),
                    None => Ok(em.value.clone()),
                };
                let value = match value {
                    Ok(value) => value,
                    Err(error) => {
                        let message = format!("edge delivery failed: {error}");
                        self.report.errors.push(WaveError::EdgeFailed {
                            edge: e.id,
                            from,
                            to: e.to,
                            reason: message.clone(),
                        });
                        self.record_settled(
                            completion.id,
                            completion.node,
                            cohort,
                            SettledOutcome::Failed(message),
                        );
                        return;
                    }
                };
                if self
                    .graph
                    .input_slot(e.to)
                    .is_ok_and(|slot| slot.identity() != value.identity())
                {
                    let message = format!(
                        "edge adapter changed the declared type on edge {from:?} -> {:?}",
                        e.to
                    );
                    self.report.errors.push(WaveError::EdgeFailed {
                        edge: e.id,
                        from,
                        to: e.to,
                        reason: message.clone(),
                    });
                    self.record_settled(
                        completion.id,
                        completion.node,
                        cohort,
                        SettledOutcome::Failed(message),
                    );
                    return;
                }
                batch.push(Incoming {
                    to: e.to,
                    cohort,
                    value,
                });
                edge_deliveries.push(DeliveryEvent {
                    edge: e.id,
                    from,
                    to: e.to,
                    cohort,
                });
            }
        }
        // Capture exact selected destinations before the atomic delivery
        // batch is consumed. Skipped edges must never become terminal colors.
        let delivered: BTreeSet<InputAddr> = batch.iter().map(|message| message.to).collect();
        if !self.deliver_batch(batch) {
            self.record_settled(
                completion.id,
                completion.node,
                cohort,
                SettledOutcome::Failed("output publication rejected".into()),
            );
            return;
        }
        // Only a fully accepted batch is observable as delivered. In particular,
        // selector/adaptor failures and immediate-slot collisions never report
        // partial fan-out, even when earlier edges were successfully prepared.
        let delivered_outputs: BTreeSet<OutputAddr> =
            edge_deliveries.iter().map(|event| event.from).collect();
        self.report.delivery_events.extend(edge_deliveries);
        if let Some(record) = completion
            .port
            .and_then(|port| self.reservations.get_mut(&(completion.node, port)))
            .and_then(|records| records.iter_mut().find(|r| r.invocation == completion.id))
        {
            record.completed = true;
        }
        self.record_settled(
            completion.id,
            completion.node,
            cohort,
            SettledOutcome::Emitted(delivered),
        );
        for em in emissions {
            let from = OutputAddr {
                node: completion.node,
                slot: em.slot,
            };
            self.report.trace.timeline.push(WaveEvent::Published(from));
            self.report
                .publication_events
                .push(PublicationEvent { from, cohort });
            let retain = match self.report.retention {
                ReportRetention::MetadataOnly => false,
                ReportRetention::TerminalOutputs => !delivered_outputs.contains(&from),
                ReportRetention::RetainPayloads => true,
            };
            if retain {
                self.report.published.push(Published {
                    from,
                    cohort,
                    value: em.value,
                });
            }
        }
    }

    fn record_settled(
        &mut self,
        invocation: InvocationId,
        node: NodeId,
        cohort: CohortId,
        outcome: SettledOutcome,
    ) {
        let region = self.graph.structural().partitions.region(node);
        self.settled_colors
            .entry(region)
            .or_default()
            .push(SettledColor {
                invocation,
                node,
                cohort,
                outcome,
            });
    }

    fn settle(&mut self) {
        // By this point ready is empty and every synchronous invocation completed.
        // A policy may still owe a *future phase* after consuming its input
        // values. Therefore settlement must consider reservation history as
        // well as buffered deliveries. Looking only at nonempty buffers loses
        // PairThenThird's slot-2 obligation once slots 0 and 1 are consumed.
        assert!(self.ready.is_empty());
        let active_ports: BTreeSet<_> = self
            .activity
            .pending_ports
            .keys()
            .copied()
            .chain(self.reservations.keys().copied())
            .collect();
        for (node, port_id) in active_ports {
            // An invalid policy result already has its own terminal error.
            // Do not call that policy again or manufacture a second incomplete error.
            if self.failed_ports.contains(&(node, port_id)) {
                continue;
            }
            let port = self.graph.nodes[&node]
                .ports
                .iter()
                .find(|p| p.id == port_id)
                .unwrap();
            let snapshots = self.snapshots(node, port);
            let rejected = self.rejected_cohorts.get(&(node, port_id));
            for group in port
                .policy
                .incomplete(&snapshots)
                .into_iter()
                .filter(|group| rejected.is_none_or(|cohorts| !cohorts.contains(&group.cohort)))
            {
                self.report
                    .trace
                    .timeline
                    .push(WaveEvent::Incomplete(node, port_id));
                self.report.errors.push(WaveError::Incomplete {
                    node,
                    port: port_id,
                    group,
                });
            }
        }
    }
}

impl fmt::Debug for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Engine").finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[cfg(test)]
#[path = "inspection_tests.rs"]
mod inspection_tests;

#[cfg(test)]
mod accounting_tests {
    use crate::{Deployment, GraphBuilder, NodeId, Value};

    #[test]
    fn observational_trace_does_not_control_dispatch_budget() {
        let mut graph = GraphBuilder::new("trace-independent-budget");
        let node = graph.node("step", |x: i64| x + 1).unwrap();
        graph.connect(node.output(), node.inlet()).unwrap();
        let input = graph.expose_input("input", node.inlet()).unwrap();
        graph.trigger("start", [input]).unwrap();
        let engine = Deployment::new(graph.compile().unwrap())
            .max_activations(1)
            .build()
            .unwrap();
        let mut wave = engine.begin("start", vec![Value::local(0_i64)]).unwrap();
        wave.state.report.trace.executed.push(NodeId::new(999));
        let (ticket, ctx) = wave.next_ready().unwrap().into_parts();
        let result = wave.action(ticket).unwrap().execute(ctx);
        wave.complete(ticket, result).unwrap();
        wave.state.report.trace.executed.clear();
        assert!(wave.next_ready().is_none());
        let report = wave.finish().unwrap();
        assert_eq!(report.activation_count(), 1);
        assert!(report.budget_exhausted());
    }
}
