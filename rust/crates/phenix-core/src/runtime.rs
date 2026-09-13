use crate::{
    prepared_mutation::PreparedMutationScope, ArtifactRevision, Authority, CallCancellationToken,
    CapabilityId, ComponentGraphError, ComponentId, ComponentInterface, ComponentInvocationError,
    DurableSchema, EventAdmissionReceipt, EventBus, EventEnvelope, EventError, EventHandler,
    EventSubscription, EventTypeId, GraphGenerationId, InterfaceId, KernelConfig, KernelError,
    KernelEvent, KernelPolicyIdentity, LocalPersistence, PersistenceBackend, PluginArtifact,
    PluginExecution, PluginId, PluginManifest, ProviderFallbackReason, ProviderSelectionReason,
    ResolvedComponentGraph, ResolvedImportHandle, ResolvedListener, ResolvedProviderPlan,
    ResolvedServiceChain, ResourceNamespace, RuntimeId, SchemaMigration, ServiceId, ServiceRole,
    SkillResourceMetadata, TaskRuntime, TaskScope, TransactionOp,
};
use parking_lot::Mutex;
use std::{
    collections::{BTreeMap, BTreeSet},
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

mod dispatch;
mod host;
mod kernel;
mod listener;
mod persistence_bootstrap;
mod reconciliation;
#[cfg(test)]
mod tests;

pub use listener::PluginListener;

const PERSISTENCE_SCHEMA: &str = "kernel.persistence.schema";
const PERSISTENCE_READ: &str = "kernel.persistence.read";
const PERSISTENCE_WRITE: &str = "kernel.persistence.write";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PluginState {
    Registered,
    Active,
    Stopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceParticipantOutcome {
    Handled,
    Delegated,
    Denied,
    Failed,
    Succeeded,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceParticipantProvenance {
    pub plugin: PluginId,
    pub role: ServiceRole,
    pub effective_authority: Authority,
    pub outcome: ServiceParticipantOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderEndpointProvenance {
    pub component: ComponentId,
    pub plugin: PluginId,
    pub runtime: Option<RuntimeId>,
    pub artifact_revision: Option<ArtifactRevision>,
}

impl ProviderEndpointProvenance {
    fn from_handle(handle: &ResolvedImportHandle) -> Self {
        let (runtime, artifact_revision) = match handle.execution() {
            PluginExecution::Runtime { runtime, artifact } => {
                (Some(runtime.clone()), Some(artifact.revision.clone()))
            }
            PluginExecution::Embedded | PluginExecution::ResourceOnly => (None, None),
        };
        Self {
            component: handle.exporter().clone(),
            plugin: handle.owning_plugin().clone(),
            runtime,
            artifact_revision,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentProviderProvenance {
    pub interface: InterfaceId,
    pub primary: ProviderEndpointProvenance,
    pub fallbacks: Vec<ProviderEndpointProvenance>,
    pub selection_reason: ProviderSelectionReason,
    pub executed_provider: ProviderEndpointProvenance,
    pub fallback_reason: Option<ProviderFallbackReason>,
    pub effective_authority: Authority,
}

impl ComponentProviderProvenance {
    fn from_plan(
        interface: InterfaceId,
        plan: &ResolvedProviderPlan,
        executed: &ResolvedImportHandle,
        fallback_reason: Option<ProviderFallbackReason>,
        effective_authority: Authority,
    ) -> Self {
        Self {
            interface,
            primary: ProviderEndpointProvenance::from_handle(plan.primary()),
            fallbacks: plan
                .fallbacks()
                .iter()
                .map(ProviderEndpointProvenance::from_handle)
                .collect(),
            selection_reason: plan.selection_reason(),
            executed_provider: ProviderEndpointProvenance::from_handle(executed),
            fallback_reason,
            effective_authority,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceInvocationProvenance {
    pub graph_generation: Option<GraphGenerationId>,
    pub policy_identity: KernelPolicyIdentity,
    pub service: ServiceId,
    pub planned_chain: ResolvedServiceChain,
    pub component_provider: Option<ComponentProviderProvenance>,
    pub caller_authority: Authority,
    pub participants: Vec<ServiceParticipantProvenance>,
    pub terminal_reached: bool,
}

#[derive(Clone, Debug)]
struct PendingParticipantProvenance {
    plugin: PluginId,
    role: ServiceRole,
    effective_authority: Authority,
    outcome: Option<ServiceParticipantOutcome>,
}

#[derive(Clone, Debug)]
struct InvocationTrace {
    graph_generation: Option<GraphGenerationId>,
    service: ServiceId,
    planned_chain: ResolvedServiceChain,
    component_provider: Option<ComponentProviderProvenance>,
    caller_authority: Authority,
    participants: Vec<PendingParticipantProvenance>,
    terminal_reached: bool,
}

impl InvocationTrace {
    fn new(
        chain: &ResolvedServiceChain,
        caller_authority: &Authority,
        graph_generation: Option<&GraphGenerationId>,
        component_provider: Option<ComponentProviderProvenance>,
    ) -> Self {
        Self {
            graph_generation: graph_generation.cloned(),
            service: chain.service.clone(),
            planned_chain: chain.clone(),
            component_provider,
            caller_authority: caller_authority.clone(),
            participants: Vec::new(),
            terminal_reached: false,
        }
    }

    fn enter(
        &mut self,
        plugin: PluginId,
        role: ServiceRole,
        effective_authority: Authority,
    ) -> usize {
        if role == ServiceRole::Terminal {
            self.terminal_reached = true;
        }
        let index = self.participants.len();
        self.participants.push(PendingParticipantProvenance {
            plugin,
            role,
            effective_authority,
            outcome: None,
        });
        index
    }

    fn set_outcome(&mut self, index: usize, outcome: ServiceParticipantOutcome) {
        self.participants[index].outcome = Some(outcome);
    }

    fn finish(self) -> ServiceInvocationProvenance {
        ServiceInvocationProvenance {
            graph_generation: self.graph_generation,
            policy_identity: self.planned_chain.policy_identity,
            service: self.service,
            planned_chain: self.planned_chain,
            component_provider: self.component_provider,
            caller_authority: self.caller_authority,
            participants: self
                .participants
                .into_iter()
                .map(|participant| ServiceParticipantProvenance {
                    plugin: participant.plugin,
                    role: participant.role,
                    effective_authority: participant.effective_authority,
                    outcome: participant
                        .outcome
                        .unwrap_or(ServiceParticipantOutcome::Failed),
                })
                .collect(),
            terminal_reached: self.terminal_reached,
        }
    }
}

#[derive(Clone)]
struct ContinuationState {
    chain: ResolvedServiceChain,
    terminal_component: Option<ComponentId>,
    next_position: usize,
    used: Arc<AtomicBool>,
    trace: Arc<Mutex<InvocationTrace>>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct ComponentServiceEndpoint {
    pub(super) component: ComponentId,
    pub(super) service: ServiceId,
}

pub struct PluginHost<'a> {
    graph_generation: Option<&'a GraphGenerationId>,
    component_graph: &'a ResolvedComponentGraph,
    config: &'a KernelConfig,
    states: &'a BTreeMap<PluginId, PluginState>,
    instances: &'a BTreeMap<PluginId, Arc<Mutex<Box<dyn PluginInstance>>>>,
    plugin: &'a PluginId,
    authority: &'a Authority,
    call_cancellation: Option<CallCancellationToken>,
    call_stack: BTreeSet<PluginId>,
    events: &'a EventBus,
    tasks: &'a TaskRuntime,
    persistence: &'a Mutex<Box<dyn PersistenceBackend>>,
    prepared_mutations: &'a PreparedMutationScope,
    provenance: &'a Mutex<Vec<ServiceInvocationProvenance>>,
    continuation: Option<ContinuationState>,
    active_services: BTreeSet<ServiceId>,
    active_component_endpoints: BTreeSet<ComponentServiceEndpoint>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LayerResult {
    Handled(Vec<u8>),
    Denied(String),
}

#[derive(Clone, Copy, Debug)]
pub struct RuntimePluginCandidate<'a> {
    pub manifest: &'a PluginManifest,
    pub artifact: &'a PluginArtifact,
    pub guest_authority: &'a Authority,
}

pub trait PluginRuntimeProvider: Send {
    fn prepare(
        &mut self,
        candidate: RuntimePluginCandidate<'_>,
    ) -> Result<Box<dyn PluginInstance>, String>;

    /// Canonical preparation entry point. Core supplies the runtime provider's own host separately
    /// from the guest authority carried by `candidate`.
    fn prepare_with_host(
        &mut self,
        candidate: RuntimePluginCandidate<'_>,
        _host: &PluginHost<'_>,
    ) -> Result<Box<dyn PluginInstance>, String> {
        self.prepare(candidate)
    }
}

/// Reentrant invocation endpoint that does not require mutable access to a whole Plugin instance.
///
/// Implementations keep mutable domain state behind their own narrow synchronization handles.
/// Core may call this endpoint while another endpoint owned by the same Plugin is active.
pub trait SharedPluginInvocation: Send + Sync {
    fn invoke(
        &self,
        _service: &ServiceId,
        _input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        Err("service invocation is not implemented".into())
    }

    fn invoke_component(
        &self,
        _component: &ComponentId,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        self.invoke(service, input, host)
    }

    fn invoke_layer(
        &self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<LayerResult, String> {
        self.invoke(service, input, host).map(LayerResult::Handled)
    }
}

pub trait PluginInstance: Send {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String>;

    fn runtime_provider(&mut self) -> Option<&mut dyn PluginRuntimeProvider> {
        None
    }

    fn shared_invocation(&self) -> Option<Arc<dyn SharedPluginInvocation>> {
        None
    }

    fn invoke(
        &mut self,
        _service: &ServiceId,
        _input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        Err("service invocation is not implemented".into())
    }

    fn invoke_component(
        &mut self,
        _component: &ComponentId,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        self.invoke(service, input, host)
    }

    fn invoke_layer(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<LayerResult, String> {
        self.invoke(service, input, host).map(LayerResult::Handled)
    }

    /// Legacy transport-only listener binding.
    ///
    /// New plugin authoring paths should prefer `bind_plugin_listener`, which
    /// receives a scoped `PluginHost` at delivery time.
    fn bind_listener(
        &mut self,
        listener: &ResolvedListener,
        _generation: &GraphGenerationId,
    ) -> Result<Arc<dyn EventHandler>, String> {
        Err(format!(
            "plugin does not implement listener {}/{}",
            listener.component, listener.declaration.method
        ))
    }

    /// Bind a runtime listener that receives a fresh scoped host for each delivery.
    fn bind_plugin_listener(
        &mut self,
        _listener: &ResolvedListener,
        _generation: &GraphGenerationId,
    ) -> Option<Result<Arc<dyn PluginListener>, String>> {
        None
    }

    fn stop(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }
}

fn stage_listener_subscriptions(
    sources: listener::ListenerRuntimeSources<'_>,
) -> Result<Vec<EventSubscription>, KernelError> {
    let mut subscriptions = Vec::new();
    for resolved_listener in sources.graph.listeners() {
        let instance = sources
            .instances
            .get(&resolved_listener.owning_plugin)
            .ok_or_else(|| KernelError::PluginNotActive(resolved_listener.owning_plugin.clone()))?;
        let mut instance = instance.lock();
        let handler = catch_unwind(AssertUnwindSafe(|| {
            match instance.bind_plugin_listener(resolved_listener, sources.generation) {
                Some(handler) => handler.map(|handler| {
                    listener::scoped_event_handler(
                        &resolved_listener.owning_plugin,
                        handler,
                        sources,
                    )
                }),
                None => instance.bind_listener(resolved_listener, sources.generation),
            }
        }))
        .map_err(|_| KernelError::ListenerBinding {
            plugin: resolved_listener.owning_plugin.clone(),
            component: resolved_listener.component.clone(),
            method: resolved_listener.declaration.method.clone(),
            message: "plugin listener binding panicked".into(),
        })?
        .map_err(|message| KernelError::ListenerBinding {
            plugin: resolved_listener.owning_plugin.clone(),
            component: resolved_listener.component.clone(),
            method: resolved_listener.declaration.method.clone(),
            message,
        })?;
        subscriptions.push(EventSubscription {
            spec: resolved_listener.subscription_spec(sources.config.policy_identity().get()),
            handler,
        });
    }
    EventBus::validate_subscriptions(subscriptions.iter().cloned())?;
    Ok(subscriptions)
}

type EmbeddedFactory = Arc<dyn Fn() -> Box<dyn PluginInstance> + Send + Sync>;

#[derive(Clone, Copy)]
struct InvocationContext<'a> {
    graph_generation: Option<&'a GraphGenerationId>,
    component_graph: &'a ResolvedComponentGraph,
    config: &'a KernelConfig,
    states: &'a BTreeMap<PluginId, PluginState>,
    instances: &'a BTreeMap<PluginId, Arc<Mutex<Box<dyn PluginInstance>>>>,
    events: &'a EventBus,
    tasks: &'a TaskRuntime,
    persistence: &'a Mutex<Box<dyn PersistenceBackend>>,
    prepared_mutations: &'a PreparedMutationScope,
    provenance: &'a Mutex<Vec<ServiceInvocationProvenance>>,
}

pub struct Kernel {
    graph_generation: Option<GraphGenerationId>,
    component_graph: ResolvedComponentGraph,
    active_resources: Vec<SkillResourceMetadata>,
    config: KernelConfig,
    states: BTreeMap<PluginId, PluginState>,
    embedded_factories: BTreeMap<PluginId, EmbeddedFactory>,
    prepared_embedded_instances: BTreeMap<PluginId, Box<dyn PluginInstance>>,
    instances: BTreeMap<PluginId, Arc<Mutex<Box<dyn PluginInstance>>>>,
    events: Arc<EventBus>,
    tasks: Arc<TaskRuntime>,
    persistence: Arc<Mutex<Box<dyn PersistenceBackend>>>,
    persistence_bootstrap: Option<crate::ResolvedPersistenceBootstrap>,
    provenance: Arc<Mutex<Vec<ServiceInvocationProvenance>>>,
    runtime_active: bool,
}
