use super::*;
use phenix_graph::{
    Emission, Executor, InputSlot, InvocationCtx, NodeAction, NodeTemplate, OutputSlot, Port,
    Value, WaveReport, emit_local,
};
use phenix_kernel::{Kernel, ProducerEvent, WakeHandle};
use phenix_plugin::PluginSnapshot;
use std::{cell::Cell, marker::PhantomData};

thread_local! { static DISPATCHING: Cell<bool> = const { Cell::new(false) }; }

struct DispatchGuard(bool);
impl DispatchGuard {
    fn enter() -> Self {
        Self(DISPATCHING.replace(true))
    }
}
impl Drop for DispatchGuard {
    fn drop(&mut self) {
        DISPATCHING.set(self.0);
    }
}

struct HostOnlyExecutor(Arc<dyn Executor>);
struct GuardedAction<'a>(&'a dyn NodeAction);
impl NodeAction for GuardedAction<'_> {
    fn execute(&self, ctx: InvocationCtx) -> Result<Vec<Emission>, String> {
        // Also guard the execution thread if the delegated executor offloads work.
        let _guard = DispatchGuard::enter();
        self.0.execute(ctx)
    }
}
impl Executor for HostOnlyExecutor {
    fn dispatch(
        &self,
        action: &dyn NodeAction,
        ctx: InvocationCtx,
    ) -> Result<Vec<Emission>, String> {
        let _guard = DispatchGuard::enter();
        self.0.dispatch(&GuardedAction(action), ctx)
    }
}

/// A lifetime-pinning integration wrapper, not a scheduler. Builds an ordinary
/// Kernel and delegates each dispatch exactly once to the supplied executor,
/// with a panic-safe host-only query guard. There is no ambient build registry.
pub struct DerivedRuntime {
    kernel: Kernel,
}
impl DerivedRuntime {
    pub fn build(
        name: impl Into<String>,
        snapshots: impl IntoIterator<Item = PluginSnapshot>,
        executor: Arc<dyn Executor>,
    ) -> Result<Self, Error> {
        let kernel = Kernel::build(name, snapshots, Arc::new(HostOnlyExecutor(executor)))
            .map_err(Error::Graph)?;
        Ok(Self { kernel })
    }
    pub fn kernel(&self) -> &Kernel {
        &self.kernel
    }
    pub fn drain(&mut self, max_polls: usize) -> Vec<ProducerEvent> {
        self.kernel.drain(max_polls)
    }
    pub fn run_until(
        &mut self,
        wake: &WakeHandle,
        on_event: impl FnMut(ProducerEvent),
    ) -> Result<(), Error> {
        self.kernel.run_until(wake, on_event).map_err(Error::Graph)
    }
    pub fn start(&mut self) -> Result<(), Error> {
        self.kernel.start().map_err(Error::Graph)
    }
    pub fn bind_query<T: Clone + Send + Sync + 'static>(
        &self,
        spec: &QuerySpec<T>,
    ) -> Result<QueryHandle<'_, T>, Error> {
        let engine = self.kernel.engine();
        let taps = engine.node_ids_for_action(&spec.action);
        let roots = engine.node_ids_for_action(&spec.value.root.action);
        let ([tap_node], [root_node]) = (taps.as_slice(), roots.as_slice()) else {
            return Err(Error::WrongGeneration);
        };
        let route = spec.value.refresh_route();
        if !engine.trigger_targets(route).is_some_and(|groups| {
            let [targets] = groups else { return false };
            let [target] = targets.as_slice() else {
                return false;
            };
            target.node_id() == *root_node && target.port_id() == 0 && target.slot_id() == 0
        }) {
            return Err(Error::InvalidRefreshRoute(route.into()));
        }
        Ok(QueryHandle {
            runtime: self,
            spec: spec.clone(),
            tap_node: *tap_node,
            root_node: *root_node,
        })
    }
}

/// Explicit terminal tap. Keep this alongside the DerivedRef in the contribution.
/// The stateless action allocation is deliberately shared across deployments of
/// the same compiled graph. More than one matching physical node is ambiguous.
#[derive(Clone)]
pub struct QuerySpec<T: Clone + Send + Sync + 'static> {
    action: Arc<dyn NodeAction>,
    value: DerivedRef<T>,
}
pub struct QueryHandle<'a, T: Clone + Send + Sync + 'static> {
    runtime: &'a DerivedRuntime,
    spec: QuerySpec<T>,
    tap_node: u16,
    root_node: u16,
}
impl<T: Clone + Send + Sync + 'static> QueryHandle<'_, T> {
    /// Fire exactly one normal kernel wave. Binding itself does no IO or waves.
    pub fn query(&self) -> Result<T, Error> {
        if DISPATCHING.get() {
            return Err(Error::NestedQuery);
        }
        let report = self
            .runtime
            .kernel
            .fire(
                self.spec.value.refresh_route(),
                vec![Value::local(RefreshCause::Demand)],
            )
            .map_err(Error::Graph)?;
        extract(&report, self.tap_node, self.root_node)
    }
}

// The source's second, terminal output records the actual refresh cohort, without
// retaining source values. Expected endpoints come from the owning Engine, not
// from identities claimed by publications or from factory registration counts.
pub(crate) struct RootReceipt {
    pub(crate) wave: u64,
    pub(crate) cohort: u64,
}
struct Terminal<T> {
    wave: u64,
    cohort: u64,
    value: Arc<T>,
}
struct Tap<T>(PhantomData<fn() -> T>);
impl<T: Clone + Send + Sync + 'static> NodeAction for Tap<T> {
    fn execute(&self, ctx: InvocationCtx) -> Result<Vec<Emission>, String> {
        Ok(vec![emit_local(
            0,
            Terminal {
                wave: ctx.wave_id(),
                cohort: ctx.cohort_id(),
                value: ctx.shared::<T>(0)?,
            },
        )])
    }
}
fn extract<T: Clone + Send + Sync + 'static>(
    report: &WaveReport,
    tap_node: u16,
    root_node: u16,
) -> Result<T, Error> {
    if !report.success() {
        return Err(Error::Wave(report.error_messages()));
    }
    let values: Vec<_> = report
        .publications()
        .filter(|p| p.source_node_id() == tap_node && p.source_slot_id() == 0)
        .collect();
    if values.len() != 1 {
        return Err(Error::PublicationCount(values.len()));
    }
    let publication = &values[0];
    let terminal = publication
        .borrow::<Terminal<T>>()
        .ok_or(Error::InvalidPublication)?;
    let receipts: Vec<_> = report
        .publications()
        .filter(|p| p.source_node_id() == root_node && p.source_slot_id() == 1)
        .collect();
    if receipts.len() != 1 {
        return Err(Error::InvalidPublication);
    }
    let source = &receipts[0];
    let receipt = source
        .borrow::<RootReceipt>()
        .ok_or(Error::InvalidPublication)?;
    if terminal.wave != report.wave_id()
        || terminal.cohort != publication.cohort_id()
        || receipt.wave != report.wave_id()
        || receipt.cohort != source.cohort_id()
        || receipt.cohort != terminal.cohort
        || report
            .publication_events()
            .iter()
            .filter(|e| e.from.node_id() == tap_node && e.from.slot_id() == 0)
            .count()
            != 1
        || report
            .publication_events()
            .iter()
            .filter(|e| e.from.node_id() == root_node && e.from.slot_id() == 1)
            .count()
            != 1
    {
        return Err(Error::InvalidPublication);
    }
    Ok((*terminal.value).clone())
}

pub(crate) fn lower<T: Clone + Send + Sync + 'static>(
    graph: &mut GraphBuilder,
    name: &str,
    value: &DerivedRef<T>,
) -> Result<QuerySpec<T>, Error> {
    let spec = QuerySpec {
        action: Arc::new(Tap::<T>(PhantomData)),
        value: value.clone(),
    };
    staged(graph, |g| {
        g.add(
            name,
            NodeTemplate::with_dyn_action(
                0,
                vec![Port::all(0, vec![InputSlot::local::<T>(0).named("value")]).named("run")],
                vec![OutputSlot::local::<Terminal<T>>(0).named("value")],
                spec.action.clone(),
            ),
        )
        .map_err(|e| Error::Graph(e.to_string()))?;
        g.connect(value.output(), g.local_input::<T>(name, "run", "value")?)?;
        Ok(())
    })?;
    Ok(spec)
}
