use super::encode_result_runtime;

use phenix_contract::ContributionSet;
pub use phenix_core::ListenerProjection;
use phenix_core::{
    Authority, ComponentId, ComponentInterface, ComponentInvocationError, ComponentManifest,
    EventAdmissionReceipt, EventBus, EventEnvelope, EventError, EventFailurePolicy,
    EventSubscription, EventTypeId, Exact, InterfaceId, KernelAccess, PhenixValue, PluginContext,
    PluginExecution, PluginHost, PluginId, PluginManifest, Project, SdkClient, SubscriptionId,
    SubscriptionSpec, ValueError,
};
use std::{
    any::TypeId,
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt::{self, Display, Formatter},
    marker::PhantomData,
    sync::{Arc, Weak},
};

#[doc(hidden)]
pub mod __phenix_plugin {
    pub use phenix_core::{
        Authority, ComponentExport, ComponentId, ComponentImport, ComponentInterface,
        ComponentInvocationError, ComponentManifest, EventBus, EventHandler, EventSubscription,
        GenerationId, InterfaceId, InterfaceSchema, PhenixValue, PluginContext, PluginExecution,
        PluginHost, PluginId, PluginInstance, PluginListener, PluginManifest, ServiceContribution,
        ServiceId, ServiceRole,
    };
}

#[derive(Clone)]
pub struct StaticPluginDependency {
    descriptor: fn() -> StaticPluginDescriptor,
    // Rust-only visitation key. Never part of portable descriptor identity.
    definition_type: TypeId,
}

impl StaticPluginDependency {
    #[must_use]
    pub fn of<T: StaticPluginDefinition>() -> Self {
        Self {
            descriptor: T::descriptor,
            definition_type: TypeId::of::<T>(),
        }
    }

    pub(super) fn descriptor(&self) -> StaticPluginDescriptor {
        (self.descriptor)()
    }
}

pub type StaticEmbeddedFactory = fn() -> Box<dyn phenix_core::PluginInstance>;

#[derive(Clone)]
pub struct StaticPluginDescriptor {
    pub id: PluginId,
    pub definition: &'static str,
    pub version: u32,
    pub execution: PluginExecution,
    pub maximum_authority: Authority,
    pub dependencies: Vec<StaticPluginDependency>,
    pub embedded_factory: Option<StaticEmbeddedFactory>,
    /// Static, inert contributions from this plugin's authored declarations.
    pub contributions: fn() -> Result<ContributionSet, String>,
}

pub trait StaticPluginFactory {
    fn factory() -> Box<dyn phenix_core::PluginInstance>;
}

pub trait StaticPluginDefinition: 'static {
    fn descriptor() -> StaticPluginDescriptor;

    /// Inert declarations authored by this plugin. Existing plugins emit none.
    /// Consumers must validate these during candidate-generation preparation.
    fn contributions() -> Result<ContributionSet, String> {
        Ok(ContributionSet::default())
    }

    fn manifest() -> PluginManifest
    where
        Self: Sized + super::StaticPluginComponents + super::StaticPluginResources,
    {
        let descriptor = Self::descriptor();
        let components = <Self as super::StaticPluginComponents>::components();
        let resources = <Self as super::StaticPluginResources>::resources();

        PluginManifest {
            id: descriptor.id,
            version: descriptor.version,
            execution: descriptor.execution,
            dependencies: descriptor
                .dependencies
                .iter()
                .map(|dependency| dependency.descriptor().id)
                .collect(),
            services: components
                .iter()
                .flat_map(|component| component.services())
                .collect(),
            resource_namespaces: resources.into_iter().map(|resource| resource.id).collect(),
            maximum_authority: descriptor.maximum_authority,
        }
    }

    fn component_manifests() -> Vec<ComponentManifest>
    where
        Self: Sized + super::StaticPluginComponents,
    {
        let descriptor = Self::descriptor();
        <Self as super::StaticPluginComponents>::components()
            .iter()
            .map(|component| {
                component.manifest_with_authority(&descriptor.id, &descriptor.maximum_authority)
            })
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StaticPluginGraphError {
    DuplicateId {
        id: PluginId,
        first: &'static str,
        second: &'static str,
    },
    Cycle {
        path: Vec<PluginId>,
    },
    InvalidExecutionFactory {
        plugin: PluginId,
    },
    InvalidContributions {
        plugin: PluginId,
        reason: String,
    },
    ContributionConflict(phenix_contract::ContributionSetError),
}

impl Display for StaticPluginGraphError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateId { id, first, second } => {
                write!(
                    f,
                    "plugin id {id} has incompatible definitions {first} and {second}"
                )
            }
            Self::Cycle { path } => {
                write!(f, "static plugin dependency cycle: ")?;
                for (index, id) in path.iter().enumerate() {
                    if index > 0 {
                        write!(f, " -> ")?;
                    }
                    write!(f, "{id}")?;
                }
                Ok(())
            }
            Self::InvalidExecutionFactory { plugin } => {
                write!(
                    f,
                    "non-embedded plugin {plugin} cannot declare an embedded factory"
                )
            }
            Self::InvalidContributions { plugin, reason } => {
                write!(
                    f,
                    "plugin {plugin} has invalid static contributions: {reason}"
                )
            }
            Self::ContributionConflict(error) => Display::fmt(error, f),
        }
    }
}

impl Error for StaticPluginGraphError {}

#[derive(Clone)]
pub struct StaticPluginGraph {
    nodes: BTreeMap<PluginId, StaticPluginDescriptor>,
    validated_origins: BTreeSet<(PluginId, TypeId)>,
    // Freeze each selected Rust definition once per candidate, even when
    // multiple parents refer to it. TypeId never leaves this SDK graph.
    authored_descriptors: BTreeMap<TypeId, StaticPluginDescriptor>,
    // Freeze dependency identities alongside each selected descriptor. A
    // dependency's author function must not be rerun during equivalence checks.
    dependency_snapshots: BTreeMap<PluginId, Vec<PluginId>>,
    contribution_snapshots: BTreeMap<PluginId, ContributionSet>,
    contributions: ContributionSet,
}

impl StaticPluginGraph {
    pub fn compose<T: StaticPluginDefinition>() -> Result<Self, StaticPluginGraphError> {
        let mut graph = Self {
            nodes: BTreeMap::new(),
            validated_origins: BTreeSet::new(),
            authored_descriptors: BTreeMap::new(),
            dependency_snapshots: BTreeMap::new(),
            contribution_snapshots: BTreeMap::new(),
            contributions: ContributionSet::default(),
        };
        let mut visiting = Vec::new();
        let root = T::descriptor();
        graph
            .authored_descriptors
            .insert(TypeId::of::<T>(), root.clone());
        graph.collect(root, TypeId::of::<T>(), &mut visiting)?;
        // Do not hand an activation caller a graph with conflicting or
        // forged contributions. Freeze the validated bytes at preparation.
        graph.contributions = graph.collect_contributions()?;
        Ok(graph)
    }

    fn collect(
        &mut self,
        descriptor: StaticPluginDescriptor,
        origin: TypeId,
        visiting: &mut Vec<PluginId>,
    ) -> Result<(), StaticPluginGraphError> {
        if !matches!(&descriptor.execution, PluginExecution::Embedded)
            && descriptor.embedded_factory.is_some()
        {
            return Err(StaticPluginGraphError::InvalidExecutionFactory {
                plugin: descriptor.id,
            });
        }
        if let Some(start) = visiting.iter().position(|id| id == &descriptor.id) {
            let mut path = visiting[start..].to_vec();
            path.push(descriptor.id);
            return Err(StaticPluginGraphError::Cycle { path });
        }
        // Revisit each Rust type only once after its entire transitive closure
        // was validated. This avoids exponential work on nested diamonds.
        // Different types with matching plugin IDs still require full checks.
        if self
            .validated_origins
            .contains(&(descriptor.id.clone(), origin))
        {
            return Ok(());
        }
        // Each distinct selected Rust definition lowers its declarations once.
        // Duplicate checks reuse the previously captured snapshot; no later
        // graph read or final collection reruns user-authored lowering.
        let declared = (descriptor.contributions)().map_err(|reason| {
            StaticPluginGraphError::InvalidContributions {
                plugin: descriptor.id.clone(),
                reason,
            }
        })?;
        for item in declared.iter() {
            if item.owner != descriptor.id {
                return Err(StaticPluginGraphError::InvalidContributions {
                    plugin: descriptor.id.clone(),
                    reason: format!(
                        "contribution {} claims unexpected owner {}",
                        item.id, item.owner
                    ),
                });
            }
        }
        // Resolve a Rust definition at most once per candidate, including
        // diamonds and equivalent aliases. Later paths reuse that same
        // immutable descriptor instead of executing author code again.
        let dependencies = descriptor
            .dependencies
            .iter()
            .map(|dependency| {
                let authored = self
                    .authored_descriptors
                    .entry(dependency.definition_type)
                    .or_insert_with(|| dependency.descriptor())
                    .clone();
                (dependency.definition_type, authored)
            })
            .collect::<Vec<_>>();
        let mut dependency_ids = dependencies
            .iter()
            .map(|(_, dependency)| dependency.id.clone())
            .collect::<Vec<_>>();
        dependency_ids.sort();
        if let Some(existing) = self.nodes.get(&descriptor.id) {
            // The Rust definition name alone cannot prove equivalence.
            // Compare portable metadata rather than unstable function addresses.
            if existing.definition == descriptor.definition
                && existing.version == descriptor.version
                && existing.execution == descriptor.execution
                && existing.maximum_authority == descriptor.maximum_authority
                && self.dependency_snapshots.get(&descriptor.id) == Some(&dependency_ids)
                && existing.embedded_factory.is_some() == descriptor.embedded_factory.is_some()
                // Two distinct Rust types with Embedded execution cannot
                // share an identity even without a factory: either type may
                // supply executable state through explicit instance preload.
                // Repeated visits to the same Rust TypeId exited above.
                && !matches!(&existing.execution, PluginExecution::Embedded)
                && self.contribution_snapshots.get(&descriptor.id) == Some(&declared)
            {
                // Matching immediate dependency IDs do not prove that their
                // transitive declarations match. A second path through a
                // diamond must still validate the candidate dependency
                // closure instead of silently accepting the first branch.
                visiting.push(descriptor.id.clone());
                for (dependency_type, dependency) in dependencies {
                    self.collect(dependency, dependency_type, visiting)?;
                }
                visiting.pop();
                self.validated_origins.insert((descriptor.id, origin));
                return Ok(());
            }
            let (first, second) = if existing.definition <= descriptor.definition {
                (existing.definition, descriptor.definition)
            } else {
                (descriptor.definition, existing.definition)
            };
            return Err(StaticPluginGraphError::DuplicateId {
                id: descriptor.id,
                first,
                second,
            });
        }

        let id = descriptor.id.clone();
        visiting.push(id.clone());
        for (dependency_type, dependency) in dependencies {
            self.collect(dependency, dependency_type, visiting)?;
        }
        visiting.pop();
        self.nodes.insert(id.clone(), descriptor);
        self.dependency_snapshots.insert(id.clone(), dependency_ids);
        self.contribution_snapshots.insert(id.clone(), declared);
        self.validated_origins.insert((id, origin));
        Ok(())
    }

    /// Gather and validate declarations from the selected dependency closure.
    /// This runs once while composing a candidate, not on later reads.
    fn collect_contributions(&self) -> Result<ContributionSet, StaticPluginGraphError> {
        let mut all = Vec::new();
        for declarations in self.contribution_snapshots.values() {
            all.extend(declarations.iter().cloned());
        }
        ContributionSet::collect(all).map_err(StaticPluginGraphError::ContributionConflict)
    }

    /// Return the validated, immutable declaration snapshot from composition.
    /// Re-reading cannot invoke author code or change generation input.
    pub fn contributions(&self) -> Result<ContributionSet, StaticPluginGraphError> {
        Ok(self.contributions.clone())
    }

    /// Canonical metadata envelopes captured from each selected Rust plugin.
    /// No author callback runs after composition.
    pub(super) fn portable_contribution_envelopes(
        &self,
    ) -> Result<Vec<(PluginId, Vec<u8>)>, phenix_contract::ContributionSetError> {
        self.contribution_snapshots
            .iter()
            .map(|(owner, declarations)| {
                declarations
                    .canonical_bytes()
                    .map(|bytes| (owner.clone(), bytes))
            })
            .collect()
    }

    /// Only a Rust definition included in the validated dependency closure
    /// may preload a stateful instance into this prepared graph. An identical
    /// declaration string supplied by a different type is not proof of origin.
    pub(super) fn contains_origin<T: StaticPluginDefinition>(&self, id: &PluginId) -> bool {
        self.validated_origins
            .contains(&(id.clone(), TypeId::of::<T>()))
    }

    /// Resolve a selected Rust type from preparation, without calling its
    /// descriptor again after the candidate's metadata was frozen.
    pub(super) fn prepared_descriptor<T: StaticPluginDefinition>(
        &self,
    ) -> Option<&StaticPluginDescriptor> {
        let authored = self.authored_descriptors.get(&TypeId::of::<T>())?;
        if !self.contains_origin::<T>(&authored.id) {
            return None;
        }
        self.nodes.get(&authored.id)
    }

    #[must_use]
    pub fn descriptor(&self, id: &PluginId) -> Option<&StaticPluginDescriptor> {
        self.nodes.get(id)
    }

    /// Dependencies are frozen with the selected descriptor during preparation.
    /// Looking them up here must not evaluate authored dependency callbacks.
    pub(super) fn prepared_dependencies(&self, id: &PluginId) -> Option<&[PluginId]> {
        self.dependency_snapshots.get(id).map(Vec::as_slice)
    }

    pub fn ids(&self) -> impl Iterator<Item = &PluginId> {
        self.nodes.keys()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

const STRUCTURAL_MISMATCH_EVENT: &str = "kernel.structural_value_mismatch";
const STRUCTURAL_MISMATCH_EVENT_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EventName(EventTypeId);

impl EventName {
    pub fn parse(value: impl Into<String>) -> Result<Self, &'static str> {
        EventTypeId::parse(value).map(Self)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    fn event_type(&self) -> EventTypeId {
        self.0.clone()
    }
}

impl Display for EventName {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct HookName(InterfaceId);

impl HookName {
    pub fn parse(value: impl Into<String>) -> Result<Self, &'static str> {
        InterfaceId::parse(value).map(Self)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    #[must_use]
    pub fn interface_id(&self) -> InterfaceId {
        self.0.clone()
    }
}

impl Display for HookName {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

#[derive(Debug)]
pub enum EventEmitError {
    Encode(String),
    Dispatch(EventError),
}

impl Display for EventEmitError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encode(message) => write!(f, "event payload encoding failed: {message}"),
            Self::Dispatch(error) => Display::fmt(error, f),
        }
    }
}

impl Error for EventEmitError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Encode(_) => None,
            Self::Dispatch(error) => Some(error),
        }
    }
}

#[derive(Clone)]
pub struct EventEmitter<'host, 'runtime> {
    kernel: KernelAccess<'host, 'runtime>,
    name: EventName,
}

impl<'host, 'runtime> EventEmitter<'host, 'runtime> {
    #[must_use]
    pub fn new(host: &'host PluginHost<'runtime>, name: EventName) -> Self {
        let kernel = PluginContext::new(host, (), (), ()).kernel;
        Self { kernel, name }
    }

    #[must_use]
    pub fn name(&self) -> &EventName {
        &self.name
    }

    pub fn emit<T>(&self, payload: &T) -> Result<EventAdmissionReceipt, EventEmitError>
    where
        for<'value> PhenixValue: From<&'value T>,
    {
        self.emit_value(&PhenixValue::from(payload))
    }

    pub fn emit_value(
        &self,
        payload: &PhenixValue,
    ) -> Result<EventAdmissionReceipt, EventEmitError> {
        let payload = serde_json::to_vec(payload)
            .map_err(|error| EventEmitError::Encode(error.to_string()))?;
        self.kernel
            .dispatch_event(self.name.event_type(), 1, 0, 0, payload)
            .map_err(EventEmitError::Dispatch)
    }
}

pub trait TypedResponseMatch<Response> {}

impl<Response> TypedResponseMatch<Response> for Response {}
impl<Response> TypedResponseMatch<Response> for Project<Response> {}
impl<Response> TypedResponseMatch<Response> for Exact<Response> {}

pub struct TypedSdkClient<'host, 'runtime, I, Request, Response>
where
    I: ComponentInterface,
{
    inner: SdkClient<'host, 'runtime, I>,
    marker: PhantomData<fn(Request) -> Response>,
}

impl<'host, 'runtime, I, Request, Response> Clone
    for TypedSdkClient<'host, 'runtime, I, Request, Response>
where
    I: ComponentInterface,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            marker: PhantomData,
        }
    }
}

impl<'host, 'runtime, I, Request, Response> TypedSdkClient<'host, 'runtime, I, Request, Response>
where
    I: ComponentInterface,
{
    #[must_use]
    pub fn new(host: &'host PluginHost<'runtime>, component: ComponentId) -> Self {
        Self {
            inner: SdkClient::new(host, component),
            marker: PhantomData,
        }
    }

    pub fn invoke<Matched>(&self, request: &Request) -> Result<Matched, ComponentInvocationError>
    where
        Matched: TypedResponseMatch<Response>,
        for<'value> PhenixValue: From<&'value Request>,
        for<'value> Matched: TryFrom<&'value PhenixValue, Error = ValueError>,
    {
        self.inner.invoke(request)
    }

    pub fn invoke_value(
        &self,
        request: &PhenixValue,
    ) -> Result<PhenixValue, ComponentInvocationError> {
        self.inner.invoke_value(request)
    }
}

pub fn dispatch_projected_provider<Request, Response, E, F>(
    host: &PluginHost<'_>,
    interface: &InterfaceId,
    input: &[u8],
    handler: F,
) -> Result<Vec<u8>, String>
where
    for<'value> Request: TryFrom<Project<&'value PhenixValue>, Error = ValueError>,
    for<'value> PhenixValue: From<&'value Response>,
    for<'value> PhenixValue: From<&'value E>,
    F: FnOnce(Request) -> Result<Response, E>,
{
    let kernel = PluginContext::new(host, (), (), ()).kernel;
    let request = kernel
        .decode_projected::<Request>(interface, input)
        .map_err(|error| error.to_string())?;
    encode_result_runtime(host, &handler(request))
}

pub fn dispatch_exact_provider<Request, Response, E, F>(
    host: &PluginHost<'_>,
    interface: &InterfaceId,
    input: &[u8],
    handler: F,
) -> Result<Vec<u8>, String>
where
    for<'value> Request: TryFrom<Exact<&'value PhenixValue>, Error = ValueError>,
    for<'value> PhenixValue: From<&'value Response>,
    for<'value> PhenixValue: From<&'value E>,
    F: FnOnce(Request) -> Result<Response, E>,
{
    let kernel = PluginContext::new(host, (), (), ()).kernel;
    let request = kernel
        .decode_exact::<Request>(interface, input)
        .map_err(|error| error.to_string())?;
    encode_result_runtime(host, &handler(request))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListenerDeclaration {
    pub local_name: &'static str,
    pub event: &'static str,
    pub projection: ListenerProjection,
}

pub fn listener_subscription<T, E, F>(
    events: &Arc<EventBus>,
    owner: PluginId,
    local_name: &'static str,
    event: &'static str,
    projection: ListenerProjection,
    maximum_authority: Authority,
    handler: F,
) -> EventSubscription
where
    for<'value> T: TryFrom<Project<&'value PhenixValue>, Error = ValueError>,
    for<'value> T: TryFrom<Exact<&'value PhenixValue>, Error = ValueError>,
    E: Display,
    F: Fn(T) -> Result<(), E> + Send + Sync + 'static,
{
    listener_subscription_with_authority(
        events,
        owner,
        ListenerDeclaration {
            local_name,
            event,
            projection,
        },
        Authority::default(),
        maximum_authority,
        handler,
    )
}

pub fn listener_subscription_with_authority<T, E, F>(
    events: &Arc<EventBus>,
    owner: PluginId,
    declaration: ListenerDeclaration,
    required_authority: Authority,
    maximum_authority: Authority,
    handler: F,
) -> EventSubscription
where
    for<'value> T: TryFrom<Project<&'value PhenixValue>, Error = ValueError>,
    for<'value> T: TryFrom<Exact<&'value PhenixValue>, Error = ValueError>,
    E: Display,
    F: Fn(T) -> Result<(), E> + Send + Sync + 'static,
{
    let ListenerDeclaration {
        local_name,
        event,
        projection,
    } = declaration;
    let subscription = SubscriptionId::parse(format!("{}/listener/{local_name}", owner.as_str()))
        .expect("generated listener subscription id is valid");
    let event_type = EventTypeId::parse(event).expect("validated static listener event type");
    let diagnostic_events = Arc::downgrade(events);
    let diagnostic_owner = owner.clone();

    EventSubscription {
        spec: SubscriptionSpec {
            id: subscription,
            owner,
            event_type,
            event_version: 1,
            dependencies: Vec::new(),
            failure_policy: EventFailurePolicy::Warn,
            required_authority,
            maximum_authority,
            kernel_policy_revision: 0,
        },
        handler: Arc::new(move |envelope: &EventEnvelope, authority: &Authority| {
            let value = serde_json::from_slice::<PhenixValue>(&envelope.payload)
                .map_err(|error| format!("listener payload decoding failed: {error}"))?;
            let payload = match projection {
                ListenerProjection::Project => T::try_from(Project(&value)),
                ListenerProjection::Exact => T::try_from(Exact(&value)),
            };
            let payload = payload.map_err(|error| {
                report_listener_mismatch(
                    &diagnostic_events,
                    &diagnostic_owner,
                    local_name,
                    envelope,
                    authority,
                    &error,
                );
                error.to_string()
            })?;
            handler(payload).map_err(|error| error.to_string())
        }),
    }
}

fn report_listener_mismatch(
    events: &Weak<EventBus>,
    owner: &PluginId,
    listener: &str,
    source: &EventEnvelope,
    authority: &Authority,
    error: &ValueError,
) {
    let Some(events) = events.upgrade() else {
        return;
    };
    let Ok(payload) = serde_json::to_vec(&serde_json::json!({
        "event": source.event_type.as_str(),
        "listener": listener,
        "direction": "listener",
        "error": error.to_string(),
    })) else {
        return;
    };
    let diagnostic = EventEnvelope {
        event_type: EventTypeId::parse(STRUCTURAL_MISMATCH_EVENT)
            .expect("static structural mismatch event type is valid"),
        version: STRUCTURAL_MISMATCH_EVENT_VERSION,
        emitter: owner.clone(),
        causality_id: source.causality_id,
        kernel_policy_revision: source.kernel_policy_revision,
        payload,
    };
    let _ = events.dispatch(&diagnostic, authority);
}
