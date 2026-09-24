use super::*;
use std::sync::Weak;

/// Runtime callback bound to one resolved plugin listener.
///
/// `EventHandler` remains the generic transport boundary. `PluginListener` is
/// the plugin-runtime boundary and receives the same scoped host used by other
/// kernel-mediated callbacks.
pub trait PluginListener: Send + Sync {
    fn handle(&self, event: &EventEnvelope, host: &PluginHost<'_>) -> Result<(), String>;
}

struct ListenerRuntimeSnapshot {
    runtime: RuntimeGeneration,
    states: BTreeMap<PluginId, PluginState>,
    instances: BTreeMap<PluginId, Arc<Mutex<Box<dyn PluginInstance>>>>,
    events: Weak<EventBus>,
    tasks: Arc<TaskRuntime>,
    persistence: Arc<Mutex<Box<dyn PersistenceBackend>>>,
    trace_sink: Arc<dyn RuntimeTraceSink>,
    provenance: Arc<ProvenanceBuffer>,
}

struct ScopedPluginListener {
    owner: PluginId,
    inner: Arc<dyn PluginListener>,
    runtime: ListenerRuntimeSnapshot,
}

#[derive(Clone, Copy)]
pub(super) struct ListenerRuntimeSources<'a> {
    pub(super) runtime: &'a RuntimeGeneration,
    pub(super) states: &'a BTreeMap<PluginId, PluginState>,
    pub(super) instances: &'a BTreeMap<PluginId, Arc<Mutex<Box<dyn PluginInstance>>>>,
    pub(super) events: &'a Arc<EventBus>,
    pub(super) tasks: &'a Arc<TaskRuntime>,
    pub(super) persistence: &'a Arc<Mutex<Box<dyn PersistenceBackend>>>,
    pub(super) trace_sink: &'a Arc<dyn RuntimeTraceSink>,
    pub(super) provenance: &'a Arc<ProvenanceBuffer>,
}

pub(super) fn scoped_event_handler(
    owner: &PluginId,
    inner: Arc<dyn PluginListener>,
    sources: ListenerRuntimeSources<'_>,
) -> Arc<dyn EventHandler> {
    Arc::new(ScopedPluginListener {
        owner: owner.clone(),
        inner,
        runtime: ListenerRuntimeSnapshot {
            runtime: sources.runtime.clone(),
            states: sources.states.clone(),
            instances: sources.instances.clone(),
            events: Arc::downgrade(sources.events),
            tasks: Arc::clone(sources.tasks),
            persistence: Arc::clone(sources.persistence),
            trace_sink: Arc::clone(sources.trace_sink),
            provenance: Arc::clone(sources.provenance),
        },
    })
}

impl ScopedPluginListener {
    fn run(&self, event: &EventEnvelope, authority: &Authority) -> Result<(), String> {
        let events = self
            .runtime
            .events
            .upgrade()
            .ok_or_else(|| "listener runtime is unavailable".to_owned())?;
        let generation = self
            .runtime
            .runtime
            .generation()
            .expect("listener runtime requires a resolved generation");
        let live_call = self.runtime.tasks.begin_call(&self.owner, Some(generation));
        let cancellation = live_call.cancellation_token().clone();
        let prepared_mutations = PreparedMutationScope::new(Some(generation));
        let host = PluginHost {
            graph_generation: Some(generation),
            component_graph: self.runtime.runtime.component_graph(),
            dispatch_topology: self.runtime.runtime.dispatch_topology(),
            config: self.runtime.runtime.config(),
            states: &self.runtime.states,
            instances: &self.runtime.instances,
            plugin: &self.owner,
            scope: CallScope::root(&self.owner, authority, Some(cancellation.clone())),
            events: &events,
            tasks: &self.runtime.tasks,
            persistence: &self.runtime.persistence,
            prepared_mutations: &prepared_mutations,
            trace_sink: self.runtime.trace_sink.as_ref(),
            provenance: &self.runtime.provenance,
            continuation: None,
        };
        let result = catch_unwind(AssertUnwindSafe(|| self.inner.handle(event, &host)))
            .map_err(|_| "plugin listener panicked".to_owned())?;
        if cancellation.is_cancelled() {
            prepared_mutations.clear();
            return Err("plugin listener cancelled".into());
        }
        result
    }
}

impl EventHandler for ScopedPluginListener {
    fn handle(&self, event: &EventEnvelope, authority: &Authority) -> Result<(), String> {
        self.run(event, authority)
    }

    fn handle_with_provenance(
        &self,
        _bus: &EventBus,
        event: &EventEnvelope,
        authority: &Authority,
        graph_generation: Option<&GraphGenerationId>,
    ) -> Result<(), String> {
        let expected = self
            .runtime
            .runtime
            .generation()
            .expect("listener runtime requires a resolved generation");
        if graph_generation.is_some_and(|generation| generation != expected) {
            return Err(format!(
                "listener generation mismatch: expected {:?}, got {:?}",
                expected, graph_generation
            ));
        }
        self.run(event, authority)
    }
}
