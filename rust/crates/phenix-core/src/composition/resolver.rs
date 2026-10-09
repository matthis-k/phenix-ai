use crate::{
    Authority, CompiledWorkflow, ComponentEntryTrigger, ComponentGraphError, ComponentManifest,
    ComponentProcessArgument, CompositionMetadataError, ConfigContribution, ConfigMergeError,
    ConfigurationFrontendId, ConfigurationFrontendMetadata, DurableSchemaRegistration,
    EntryTriggerKind, FrontendConfigContribution, FrontendConfigError, GenerationId, InterfaceId,
    KernelConfig, KernelError, LayerPolicy, PermissionId, PersistenceBackendFeature, PluginId,
    PluginManifest, ProviderCompositionPolicy, ResolvedComponentGraph, ResolvedConfigContributions,
    ResolvedDispatchTopology, ResourceNamespace, ServiceId, ServiceRole, SkillResourceMetadata,
    WorkflowCompileError, WorkflowDeclaration,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt::{self, Display, Formatter},
};

#[derive(Clone, Debug)]
enum GenerationTopologyIdentity {
    Bootstrap,
    Resolved(GenerationId),
}

/// One coherent runtime topology.
///
/// Bootstrap is an explicit state: configuration exists before a resolved
/// semantic generation is installed. In the resolved state the generation
/// identity, configuration, component graph, and resources always move
/// together as one value.
#[derive(Clone, Debug)]
pub struct GenerationTopology {
    identity: GenerationTopologyIdentity,
    config: KernelConfig,
    component_graph: ResolvedComponentGraph,
    dispatch_topology: ResolvedDispatchTopology,
    resources: Vec<SkillResourceMetadata>,
    entry_triggers: Vec<ComponentEntryTrigger>,
    workflows: BTreeMap<(crate::ComponentId, String), CompiledWorkflow>,
}

impl GenerationTopology {
    pub(crate) fn bootstrap(config: KernelConfig) -> Self {
        let dispatch_topology = config.resolved_dispatch_topology();
        Self {
            identity: GenerationTopologyIdentity::Bootstrap,
            config,
            component_graph: ResolvedComponentGraph::empty(),
            dispatch_topology,
            resources: Vec::new(),
            entry_triggers: Vec::new(),
            workflows: BTreeMap::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn bootstrap_with_component_graph(
        config: KernelConfig,
        component_graph: ResolvedComponentGraph,
    ) -> Self {
        let dispatch_topology = config
            .resolved_dispatch_topology()
            .with_component_graph(&component_graph);
        Self {
            identity: GenerationTopologyIdentity::Bootstrap,
            config,
            component_graph,
            dispatch_topology,
            resources: Vec::new(),
            entry_triggers: Vec::new(),
            workflows: BTreeMap::new(),
        }
    }

    fn resolved(
        id: GenerationId,
        config: KernelConfig,
        component_graph: ResolvedComponentGraph,
        resources: Vec<SkillResourceMetadata>,
        entry_triggers: Vec<ComponentEntryTrigger>,
    ) -> Self {
        let dispatch_topology = config
            .resolved_dispatch_topology()
            .with_component_graph(&component_graph);
        Self {
            identity: GenerationTopologyIdentity::Resolved(id),
            config,
            component_graph,
            dispatch_topology,
            resources,
            entry_triggers,
            workflows: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn generation(&self) -> Option<&GenerationId> {
        match &self.identity {
            GenerationTopologyIdentity::Bootstrap => None,
            GenerationTopologyIdentity::Resolved(id) => Some(id),
        }
    }

    pub(crate) fn resolved_generation(&self) -> &GenerationId {
        self.generation()
            .expect("resolved generation topology has an identity")
    }

    #[must_use]
    pub fn config(&self) -> &KernelConfig {
        &self.config
    }

    #[must_use]
    pub fn component_graph(&self) -> &ResolvedComponentGraph {
        &self.component_graph
    }

    #[must_use]
    pub fn dispatch_topology(&self) -> &ResolvedDispatchTopology {
        &self.dispatch_topology
    }

    #[must_use]
    pub fn resources(&self) -> &[SkillResourceMetadata] {
        &self.resources
    }

    #[must_use]
    pub fn entry_triggers(&self) -> &[ComponentEntryTrigger] {
        &self.entry_triggers
    }
    pub fn workflow(&self, owner: &crate::ComponentId, name: &str) -> Option<&CompiledWorkflow> {
        self.workflows.get(&(owner.clone(), name.to_owned()))
    }

    fn incorporate_semantic_metadata<T: Serialize>(&mut self, metadata: &T) {
        let id = match &mut self.identity {
            GenerationTopologyIdentity::Bootstrap => None,
            GenerationTopologyIdentity::Resolved(id) => Some(id),
        }
        .expect("bootstrap runtime generation cannot absorb resolved semantic metadata");
        let bytes = serde_json::to_vec(&(id.as_str(), metadata))
            .expect("resolved composition metadata is serializable");
        *id = GenerationId::from(format!("sha256:{:x}", Sha256::digest(bytes)));
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GenerationResolutionError {
    ComponentGraph(ComponentGraphError),
    Kernel(KernelError),
    CompositionMetadata {
        resource: String,
        error: CompositionMetadataError,
    },
    ConfigurationMerge(ConfigMergeError),
    DuplicateConfigurationFrontend(ConfigurationFrontendId),
    UnknownConfigurationFrontend(ConfigurationFrontendId),
    ConfigurationFrontend {
        frontend: ConfigurationFrontendId,
        error: FrontendConfigError,
    },
    DuplicateResource(String),
    MissingWorkflowOwner(crate::ComponentId),
    InvalidWorkflowName(crate::ComponentId),
    WorkflowAlreadyBound,
    FrameSchemasAlreadyBound,
    MissingFrameWorkflow {
        owner: crate::ComponentId,
        name: String,
    },
    DuplicateFrameSchema {
        owner: crate::ComponentId,
        name: String,
    },
    InvalidFrameSchema {
        owner: crate::ComponentId,
        name: String,
        error: crate::WorkflowFrameError,
    },
    DuplicateWorkflow {
        owner: crate::ComponentId,
        name: String,
    },
    InvalidWorkflow {
        owner: crate::ComponentId,
        name: String,
        error: WorkflowCompileError,
    },
    DuplicateDurableSchema(ResourceNamespace),
    UndeclaredDurableSchema {
        plugin: PluginId,
        namespace: ResourceNamespace,
    },
    DurableSchemaOwnerMismatch {
        plugin: PluginId,
        namespace: ResourceNamespace,
        owner: PluginId,
    },
    MissingResourceDependency {
        resource: String,
        dependency: String,
    },
    ResourceConflict {
        resource: String,
        conflict: String,
    },
    ResourceInterfaceUnavailable {
        resource: String,
        interface: InterfaceId,
    },
    ResourceAuthorityDenied {
        resource: String,
        capability: PermissionId,
    },
    MissingEntryTriggerTarget {
        component: crate::ComponentId,
        interface: InterfaceId,
    },
    EntryTriggerAuthorityDenied {
        component: crate::ComponentId,
        interface: InterfaceId,
    },
    DuplicateToolCallTrigger(crate::CallableId),
    MissingProcessArgumentTarget {
        component: crate::ComponentId,
        interface: InterfaceId,
    },
    ProcessArgumentAuthorityDenied {
        component: crate::ComponentId,
        interface: InterfaceId,
    },
    InvalidProcessArgument(String),
    DuplicateProcessArgument(String),
    DuplicateLayerPolicy {
        service: ServiceId,
        plugin: PluginId,
    },
    RequiredLayerUnavailable {
        service: ServiceId,
        plugin: PluginId,
    },
}

impl Display for GenerationResolutionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ComponentGraph(error) => Display::fmt(error, f),
            Self::Kernel(error) => Display::fmt(error, f),
            Self::CompositionMetadata { resource, error } => {
                write!(f, "resource {resource} metadata is invalid: {error:?}")
            }
            Self::ConfigurationMerge(error) => write!(f, "configuration merge failed: {error:?}"),
            Self::DuplicateConfigurationFrontend(frontend) => {
                write!(f, "duplicate configuration frontend metadata: {frontend}")
            }
            Self::UnknownConfigurationFrontend(frontend) => {
                write!(f, "unknown configuration frontend: {frontend}")
            }
            Self::ConfigurationFrontend { frontend, error } => {
                write!(
                    f,
                    "configuration frontend {frontend} rejected contribution: {error:?}"
                )
            }
            Self::MissingWorkflowOwner(owner) => {
                write!(f, "workflow owner {owner} is absent from resolved graph")
            }
            Self::InvalidWorkflowName(owner) => {
                write!(f, "workflow owner {owner} has empty workflow name")
            }
            Self::WorkflowAlreadyBound => {
                write!(
                    f,
                    "workflow declarations already belong to this resolved generation"
                )
            }
            Self::FrameSchemasAlreadyBound => {
                write!(f, "frame schemas already belong to this resolved generation")
            }
            Self::MissingFrameWorkflow { owner, name } => {
                write!(f, "frame schema targets an unselected workflow {owner}:{name}")
            }
            Self::DuplicateFrameSchema { owner, name } => {
                write!(f, "duplicate frame schema for workflow {owner}:{name}")
            }
            Self::InvalidFrameSchema { owner, name, error } => {
                write!(f, "invalid frame schema for workflow {owner}:{name}: {error}")
            }
            Self::DuplicateWorkflow { owner, name } => {
                write!(f, "duplicate workflow {owner}:{name}")
            }
            Self::InvalidWorkflow { owner, name, error } => {
                write!(f, "workflow {owner}:{name} is invalid: {error:?}")
            }
            Self::DuplicateResource(resource) => {
                write!(f, "duplicate skill/resource metadata: {resource}")
            }
            Self::DuplicateDurableSchema(namespace) => {
                write!(f, "duplicate durable schema declaration: {namespace}")
            }
            Self::UndeclaredDurableSchema { plugin, namespace } => write!(
                f,
                "plugin {plugin} declares durable schema {namespace} without owning that namespace"
            ),
            Self::DurableSchemaOwnerMismatch {
                plugin,
                namespace,
                owner,
            } => write!(
                f,
                "plugin {plugin} declares durable schema {namespace}, but the namespace belongs to {owner}"
            ),
            Self::MissingResourceDependency {
                resource,
                dependency,
            } => write!(
                f,
                "resource {resource} requires missing resource {dependency}"
            ),
            Self::ResourceConflict { resource, conflict } => {
                write!(
                    f,
                    "resource {resource} conflicts with selected resource {conflict}"
                )
            }
            Self::ResourceInterfaceUnavailable {
                resource,
                interface,
            } => write!(
                f,
                "resource {resource} requires unavailable interface {interface}"
            ),
            Self::ResourceAuthorityDenied {
                resource,
                capability,
            } => write!(
                f,
                "resource {resource} requires denied capability {capability}"
            ),
            Self::MissingEntryTriggerTarget {
                component,
                interface,
            } => {
                write!(
                    f,
                    "entry trigger targets missing export {component}:{interface}"
                )
            }
            Self::EntryTriggerAuthorityDenied {
                component,
                interface,
            } => {
                write!(
                    f,
                    "entry trigger authority exceeds runtime/component authority for {component}:{interface}"
                )
            }
            Self::DuplicateToolCallTrigger(callable) => {
                write!(f, "duplicate tool-call trigger id: {callable}")
            }
            Self::MissingProcessArgumentTarget {
                component,
                interface,
            } => {
                write!(
                    f,
                    "process argument targets missing export {component}:{interface}"
                )
            }
            Self::ProcessArgumentAuthorityDenied {
                component,
                interface,
            } => {
                write!(
                    f,
                    "process argument authority exceeds runtime/component authority for {component}:{interface}"
                )
            }
            Self::InvalidProcessArgument(argument) => {
                write!(f, "invalid process argument trigger: {argument}")
            }
            Self::DuplicateProcessArgument(argument) => {
                write!(f, "duplicate process argument trigger: {argument}")
            }
            Self::DuplicateLayerPolicy { service, plugin } => {
                write!(
                    f,
                    "duplicate layer policy for {plugin} on service {service}"
                )
            }
            Self::RequiredLayerUnavailable { service, plugin } => {
                write!(
                    f,
                    "required layer {plugin} is unavailable for service {service}"
                )
            }
        }
    }
}

impl Error for GenerationResolutionError {}

impl From<ComponentGraphError> for GenerationResolutionError {
    fn from(error: ComponentGraphError) -> Self {
        Self::ComponentGraph(error)
    }
}

impl From<ConfigMergeError> for GenerationResolutionError {
    fn from(error: ConfigMergeError) -> Self {
        Self::ConfigurationMerge(error)
    }
}

impl From<KernelError> for GenerationResolutionError {
    fn from(error: KernelError) -> Self {
        Self::Kernel(error)
    }
}

#[derive(Clone, Debug)]
pub struct ResolvedGeneration {
    runtime: GenerationTopology,
    plugins: Vec<PluginManifest>,
    components: Vec<ComponentManifest>,
    entry_triggers: Vec<ComponentEntryTrigger>,
    process_arguments: Vec<ComponentProcessArgument>,
    workflows: Vec<WorkflowDeclaration>,
    durable_schemas: Vec<DurableSchemaRegistration>,
    configuration: ResolvedConfigContributions,
    layer_policies: BTreeMap<ServiceId, Vec<LayerPolicy>>,
    provider_policy: ProviderCompositionPolicy,
    authority_ceiling: Authority,
}

struct ResolutionInputs {
    durable_schemas: Vec<DurableSchemaRegistration>,
    resources: Vec<SkillResourceMetadata>,
    entry_triggers: Vec<ComponentEntryTrigger>,
    process_arguments: Vec<ComponentProcessArgument>,
    contributions: Vec<ConfigContribution>,
    layer_policies: BTreeMap<ServiceId, Vec<LayerPolicy>>,
    provider_policy: ProviderCompositionPolicy,
}

impl ResolutionInputs {
    fn new(
        durable_schemas: impl IntoIterator<Item = DurableSchemaRegistration>,
        resources: impl IntoIterator<Item = SkillResourceMetadata>,
        entry_triggers: impl IntoIterator<Item = ComponentEntryTrigger>,
        contributions: impl IntoIterator<Item = ConfigContribution>,
        layer_policies: BTreeMap<ServiceId, Vec<LayerPolicy>>,
        provider_policy: ProviderCompositionPolicy,
    ) -> Self {
        Self {
            durable_schemas: durable_schemas.into_iter().collect(),
            resources: resources.into_iter().collect(),
            entry_triggers: entry_triggers.into_iter().collect(),
            process_arguments: Vec::new(),
            contributions: contributions.into_iter().collect(),
            layer_policies,
            provider_policy,
        }
    }

    fn with_process_arguments(
        mut self,
        process_arguments: impl IntoIterator<Item = ComponentProcessArgument>,
    ) -> Self {
        self.process_arguments = process_arguments.into_iter().collect();
        self
    }
}

impl ResolvedGeneration {
    pub fn resolve(
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        contributions: impl IntoIterator<Item = ConfigContribution>,
        authority_ceiling: &Authority,
    ) -> Result<Self, GenerationResolutionError> {
        Self::resolve_with_inputs(
            plugin_manifests,
            component_manifests,
            ResolutionInputs::new(
                [],
                [],
                [],
                contributions,
                BTreeMap::new(),
                ProviderCompositionPolicy::default(),
            ),
            authority_ceiling,
        )
    }

    pub fn resolve_with_durable_schemas(
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        durable_schemas: impl IntoIterator<Item = DurableSchemaRegistration>,
        contributions: impl IntoIterator<Item = ConfigContribution>,
        authority_ceiling: &Authority,
    ) -> Result<Self, GenerationResolutionError> {
        Self::resolve_with_inputs(
            plugin_manifests,
            component_manifests,
            ResolutionInputs::new(
                durable_schemas,
                [],
                [],
                contributions,
                BTreeMap::new(),
                ProviderCompositionPolicy::default(),
            ),
            authority_ceiling,
        )
    }

    pub fn resolve_with_provider_policy(
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        contributions: impl IntoIterator<Item = ConfigContribution>,
        provider_policy: ProviderCompositionPolicy,
        authority_ceiling: &Authority,
    ) -> Result<Self, GenerationResolutionError> {
        Self::resolve_with_inputs(
            plugin_manifests,
            component_manifests,
            ResolutionInputs::new([], [], [], contributions, BTreeMap::new(), provider_policy),
            authority_ceiling,
        )
    }

    pub fn resolve_with_resources(
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        resources: impl IntoIterator<Item = SkillResourceMetadata>,
        contributions: impl IntoIterator<Item = ConfigContribution>,
        authority_ceiling: &Authority,
    ) -> Result<Self, GenerationResolutionError> {
        Self::resolve_with_inputs(
            plugin_manifests,
            component_manifests,
            ResolutionInputs::new(
                [],
                resources,
                [],
                contributions,
                BTreeMap::new(),
                ProviderCompositionPolicy::default(),
            ),
            authority_ceiling,
        )
    }

    pub fn resolve_with_layer_policies(
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        contributions: impl IntoIterator<Item = ConfigContribution>,
        layer_policies: BTreeMap<ServiceId, Vec<LayerPolicy>>,
        authority_ceiling: &Authority,
    ) -> Result<Self, GenerationResolutionError> {
        Self::resolve_with_inputs(
            plugin_manifests,
            component_manifests,
            ResolutionInputs::new(
                [],
                [],
                [],
                contributions,
                layer_policies,
                ProviderCompositionPolicy::default(),
            ),
            authority_ceiling,
        )
    }

    pub fn resolve_with_durable_schemas_and_layer_policies(
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        durable_schemas: impl IntoIterator<Item = DurableSchemaRegistration>,
        contributions: impl IntoIterator<Item = ConfigContribution>,
        layer_policies: BTreeMap<ServiceId, Vec<LayerPolicy>>,
        authority_ceiling: &Authority,
    ) -> Result<Self, GenerationResolutionError> {
        Self::resolve_with_inputs(
            plugin_manifests,
            component_manifests,
            ResolutionInputs::new(
                durable_schemas,
                [],
                [],
                contributions,
                layer_policies,
                ProviderCompositionPolicy::default(),
            ),
            authority_ceiling,
        )
    }

    pub fn resolve_with_durable_schemas_layer_policies_and_entry_triggers(
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        durable_schemas: impl IntoIterator<Item = DurableSchemaRegistration>,
        entry_triggers: impl IntoIterator<Item = ComponentEntryTrigger>,
        contributions: impl IntoIterator<Item = ConfigContribution>,
        layer_policies: BTreeMap<ServiceId, Vec<LayerPolicy>>,
        authority_ceiling: &Authority,
    ) -> Result<Self, GenerationResolutionError> {
        Self::resolve_with_inputs(
            plugin_manifests,
            component_manifests,
            ResolutionInputs::new(
                durable_schemas,
                [],
                entry_triggers,
                contributions,
                layer_policies,
                ProviderCompositionPolicy::default(),
            ),
            authority_ceiling,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn resolve_with_durable_schemas_layer_policies_entry_triggers_and_process_arguments(
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        durable_schemas: impl IntoIterator<Item = DurableSchemaRegistration>,
        entry_triggers: impl IntoIterator<Item = ComponentEntryTrigger>,
        process_arguments: impl IntoIterator<Item = ComponentProcessArgument>,
        contributions: impl IntoIterator<Item = ConfigContribution>,
        layer_policies: BTreeMap<ServiceId, Vec<LayerPolicy>>,
        authority_ceiling: &Authority,
    ) -> Result<Self, GenerationResolutionError> {
        Self::resolve_with_composition_policies(
            plugin_manifests,
            component_manifests,
            durable_schemas,
            entry_triggers,
            process_arguments,
            contributions,
            layer_policies,
            ProviderCompositionPolicy::default(),
            authority_ceiling,
        )
    }

    /// Resolve all runtime metadata with explicit provider and service Layer policies.
    ///
    /// Both policies are canonical Phenix composition inputs; configuration
    /// frontends may supply them independently of deployment tooling.
    #[allow(clippy::too_many_arguments)]
    pub fn resolve_with_composition_policies(
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        durable_schemas: impl IntoIterator<Item = DurableSchemaRegistration>,
        entry_triggers: impl IntoIterator<Item = ComponentEntryTrigger>,
        process_arguments: impl IntoIterator<Item = ComponentProcessArgument>,
        contributions: impl IntoIterator<Item = ConfigContribution>,
        layer_policies: BTreeMap<ServiceId, Vec<LayerPolicy>>,
        provider_policy: ProviderCompositionPolicy,
        authority_ceiling: &Authority,
    ) -> Result<Self, GenerationResolutionError> {
        Self::resolve_with_inputs(
            plugin_manifests,
            component_manifests,
            ResolutionInputs::new(
                durable_schemas,
                [],
                entry_triggers,
                contributions,
                layer_policies,
                provider_policy,
            )
            .with_process_arguments(process_arguments),
            authority_ceiling,
        )
    }

    pub fn resolve_with_resources_and_layer_policies(
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        resources: impl IntoIterator<Item = SkillResourceMetadata>,
        contributions: impl IntoIterator<Item = ConfigContribution>,
        layer_policies: BTreeMap<ServiceId, Vec<LayerPolicy>>,
        authority_ceiling: &Authority,
    ) -> Result<Self, GenerationResolutionError> {
        Self::resolve_with_inputs(
            plugin_manifests,
            component_manifests,
            ResolutionInputs::new(
                [],
                resources,
                [],
                contributions,
                layer_policies,
                ProviderCompositionPolicy::default(),
            ),
            authority_ceiling,
        )
    }

    fn resolve_with_inputs(
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        mut inputs: ResolutionInputs,
        authority_ceiling: &Authority,
    ) -> Result<Self, GenerationResolutionError> {
        let mut plugins: Vec<_> = plugin_manifests.into_iter().collect();
        plugins.sort_by(|left, right| left.id.cmp(&right.id));
        let mut components: Vec<_> = component_manifests.into_iter().collect();
        components.sort_by(|left, right| left.id.cmp(&right.id));
        let mut entry_triggers = inputs.entry_triggers;
        entry_triggers.sort_by(entry_trigger_order);
        validate_entry_triggers(&components, &entry_triggers, authority_ceiling)?;
        let mut process_arguments = inputs.process_arguments;
        process_arguments.sort_by(process_argument_order);
        validate_process_arguments(&components, &process_arguments, authority_ceiling)?;
        let resources = resolve_resources(inputs.resources, &components, authority_ceiling)?;
        validate_layer_policies(&plugins, &inputs.layer_policies, authority_ceiling)?;
        for layers in inputs.layer_policies.values_mut() {
            layers.sort_by(|left, right| {
                right
                    .priority
                    .cmp(&left.priority)
                    .then_with(|| left.plugin.cmp(&right.plugin))
            });
        }
        let configuration =
            ResolvedConfigContributions::try_resolve(inputs.contributions, authority_ceiling)?;
        let mut kernel_config = KernelConfig::new(plugins.clone())?;
        for (service, layers) in &inputs.layer_policies {
            kernel_config = kernel_config.with_layer_policy(service.clone(), layers.clone())?;
        }
        let durable_schemas = resolve_durable_schemas(inputs.durable_schemas, &kernel_config)?;
        let component_graph = ResolvedComponentGraph::compile_with_provider_policy(
            plugins.clone(),
            components.clone(),
            authority_ceiling,
            &inputs.provider_policy,
        )?;
        let generation = SemanticGeneration {
            plugins: &plugins,
            components: &components,
            entry_triggers: &entry_triggers,
            process_arguments: &process_arguments,
            durable_schemas: durable_schema_payload(&durable_schemas),
            resources: &resources,
            configuration: configuration.semantic_payload(),
            layer_policies: layer_policy_payload(&inputs.layer_policies),
            provider_policy: &inputs.provider_policy,
            authority_ceiling,
        }
        .identity();

        Ok(Self {
            runtime: GenerationTopology::resolved(
                generation,
                kernel_config,
                component_graph,
                resources,
                entry_triggers.clone(),
            ),
            plugins,
            components,
            entry_triggers,
            process_arguments,
            workflows: Vec::new(),
            durable_schemas,
            configuration,
            layer_policies: inputs.layer_policies,
            provider_policy: inputs.provider_policy,
            authority_ceiling: authority_ceiling.clone(),
        })
    }

    pub fn resolve_frontends(
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        frontend_metadata: impl IntoIterator<Item = ConfigurationFrontendMetadata>,
        contributions: impl IntoIterator<Item = (ConfigurationFrontendId, FrontendConfigContribution)>,
        authority_ceiling: &Authority,
    ) -> Result<Self, GenerationResolutionError> {
        let mut frontends = BTreeMap::new();
        for metadata in frontend_metadata {
            let id = metadata.id.clone();
            if frontends.insert(id.clone(), metadata).is_some() {
                return Err(GenerationResolutionError::DuplicateConfigurationFrontend(
                    id,
                ));
            }
        }

        let lowered = contributions
            .into_iter()
            .map(|(frontend, contribution)| {
                let metadata = frontends.get(&frontend).ok_or_else(|| {
                    GenerationResolutionError::UnknownConfigurationFrontend(frontend.clone())
                })?;
                contribution
                    .lower(metadata, authority_ceiling)
                    .map_err(|error| GenerationResolutionError::ConfigurationFrontend {
                        frontend: frontend.clone(),
                        error,
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Self::resolve(
            plugin_manifests,
            component_manifests,
            lowered,
            authority_ceiling,
        )
    }

    pub fn generation(&self) -> &GenerationId {
        self.runtime.resolved_generation()
    }

    #[must_use]
    pub fn generation_topology(&self) -> &GenerationTopology {
        &self.runtime
    }

    pub fn plugins(&self) -> &[PluginManifest] {
        &self.plugins
    }

    pub fn components(&self) -> &[ComponentManifest] {
        &self.components
    }

    pub fn entry_triggers(&self) -> &[ComponentEntryTrigger] {
        &self.entry_triggers
    }

    pub fn process_arguments(&self) -> &[ComponentProcessArgument] {
        &self.process_arguments
    }

    pub fn workflows(&self) -> &[WorkflowDeclaration] {
        &self.workflows
    }

    /// Bind workflow nodes to the canonical resolved component imports.
    pub fn with_workflows(
        mut self,
        declarations: impl IntoIterator<Item = WorkflowDeclaration>,
    ) -> Result<Self, GenerationResolutionError> {
        let mut declarations: Vec<_> = declarations.into_iter().collect();
        declarations.sort_by(|left, right| {
            left.owner
                .cmp(&right.owner)
                .then_with(|| left.name.cmp(&right.name))
        });
        // Generation metadata is immutable once workflows are attached.
        // A new selection must re-resolve the candidate generation.
        if !self.workflows.is_empty() {
            return if self.workflows == declarations {
                Ok(self)
            } else {
                Err(GenerationResolutionError::WorkflowAlreadyBound)
            };
        }
        if declarations.is_empty() {
            return Ok(self);
        }
        // Reject conflicting authorship before inspecting a provider graph.
        // Two declarations for one owner/name are ambiguous even when one is
        // malformed. Their input enumeration must not decide which failure
        // wins or which selected import is inspected first.
        for pair in declarations.windows(2) {
            if pair[0].owner == pair[1].owner && pair[0].name == pair[1].name {
                return Err(GenerationResolutionError::DuplicateWorkflow {
                    owner: pair[0].owner.clone(),
                    name: pair[0].name.clone(),
                });
            }
        }
        // Resolve inclusions from the complete selected declaration set, never
        // from a runtime provider search. Same-component reuse is the only
        // permitted ownership scope in this first lowering form.
        let selected_topologies: BTreeMap<_, _> = declarations
            .iter()
            .map(|declaration| {
                (
                    (declaration.owner.clone(), declaration.name.clone()),
                    declaration.topology.clone(),
                )
            })
            .collect();
        let mut compiled = BTreeMap::new();
        for declaration in &declarations {
            if declaration.name.trim().is_empty() {
                return Err(GenerationResolutionError::InvalidWorkflowName(
                    declaration.owner.clone(),
                ));
            }
            if self
                .component_graph()
                .component(&declaration.owner)
                .is_none()
            {
                return Err(GenerationResolutionError::MissingWorkflowOwner(
                    declaration.owner.clone(),
                ));
            }
            let topology = crate::WorkflowTopology::inline_selected(
                &declaration.owner,
                &declaration.name,
                &selected_topologies,
            )
            .map_err(|error| GenerationResolutionError::InvalidWorkflow {
                owner: declaration.owner.clone(),
                name: declaration.name.clone(),
                error,
            })?;
            let workflow = topology
                .compile_for_component(self.component_graph(), &declaration.owner)
                .map_err(|error| GenerationResolutionError::InvalidWorkflow {
                    owner: declaration.owner.clone(),
                    name: declaration.name.clone(),
                    error,
                })?;
            let key = (declaration.owner.clone(), declaration.name.clone());
            if compiled.insert(key.clone(), workflow).is_some() {
                return Err(GenerationResolutionError::DuplicateWorkflow {
                    owner: key.0,
                    name: key.1,
                });
            }
        }
        // A compiler semantic revision changes the meaning of identical
        // authored plan bytes. Version the canonical execution contract in
        // every pinned generation, not just the plugin-provided declarations.
        const INVOKE_EXIT_LOWERING_REVISION: u32 = 2;
        self.runtime.incorporate_semantic_metadata(&(
            "phenix.workflow-ir",
            INVOKE_EXIT_LOWERING_REVISION,
            &declarations,
        ));
        self.runtime.workflows = compiled;
        self.workflows = declarations;
        Ok(self)
    }

    /// Bind frame types to existing selected workflows as part of a candidate.
    ///
    /// All ownership, schema, and duplicate checks happen before publishing
    /// any new compiled binding or changing generation identity. Legacy plans
    /// remain frame-free unless their owner explicitly declares a schema.
    pub fn with_workflow_frame_schemas(
        mut self,
        declarations: impl IntoIterator<Item = crate::WorkflowFrameDeclaration>,
    ) -> Result<Self, GenerationResolutionError> {
        let mut declarations: Vec<_> = declarations.into_iter().collect();
        declarations.sort_by(|left, right| {
            left.owner.cmp(&right.owner).then_with(|| left.name.cmp(&right.name))
        });
        for pair in declarations.windows(2) {
            if pair[0].owner == pair[1].owner && pair[0].name == pair[1].name {
                return Err(GenerationResolutionError::DuplicateFrameSchema {
                    owner: pair[0].owner.clone(),
                    name: pair[0].name.clone(),
                });
            }
        }
        let mut already_bound = 0;
        for declaration in &declarations {
            declaration.schema.validate().map_err(|error| {
                GenerationResolutionError::InvalidFrameSchema {
                    owner: declaration.owner.clone(),
                    name: declaration.name.clone(),
                    error,
                }
            })?;
            let compiled = self
                .runtime
                .workflow(&declaration.owner, &declaration.name)
                .ok_or_else(|| GenerationResolutionError::MissingFrameWorkflow {
                    owner: declaration.owner.clone(),
                    name: declaration.name.clone(),
                })?;
            if let Some(existing) = compiled.frame_schema() {
                if existing != &declaration.schema {
                    return Err(GenerationResolutionError::FrameSchemasAlreadyBound);
                }
                already_bound += 1;
            }
        }
        if already_bound == declarations.len() {
            return Ok(self);
        }
        if already_bound != 0 {
            return Err(GenerationResolutionError::FrameSchemasAlreadyBound);
        }
        const FRAME_CONTRACT_REVISION: u32 = 1;
        self.runtime.incorporate_semantic_metadata(
            &(FRAME_CONTRACT_REVISION, &declarations),
        );
        for declaration in declarations {
            self.runtime
                .workflows
                .get_mut(&(declaration.owner, declaration.name))
                .expect("selected schema target was validated")
                .bind_frame_schema(declaration.schema);
        }
        Ok(self)
    }

    pub fn durable_schemas(&self) -> &[DurableSchemaRegistration] {
        &self.durable_schemas
    }

    pub fn resources(&self) -> &[SkillResourceMetadata] {
        self.runtime.resources()
    }

    pub fn configuration(&self) -> &ResolvedConfigContributions {
        &self.configuration
    }

    pub fn kernel_config(&self) -> &KernelConfig {
        self.runtime.config()
    }

    pub fn component_graph(&self) -> &ResolvedComponentGraph {
        self.runtime.component_graph()
    }

    pub fn layer_policies(&self) -> &BTreeMap<ServiceId, Vec<LayerPolicy>> {
        &self.layer_policies
    }

    pub fn provider_policy(&self) -> &ProviderCompositionPolicy {
        &self.provider_policy
    }

    pub fn authority_ceiling(&self) -> &Authority {
        &self.authority_ceiling
    }

    pub(crate) fn incorporate_semantic_metadata<T: Serialize>(&mut self, metadata: &T) {
        self.runtime.incorporate_semantic_metadata(metadata);
    }

    /// Re-resolve a sibling harness that shares this harness's resources,
    /// configuration, layer policies, and provider policy but uses a complete
    /// desired plugin and component set. Kernel plugin management uses this to
    /// turn a desired plugin set into one atomic candidate generation.
    pub(crate) fn with_plugin_set(
        &self,
        plugins: Vec<PluginManifest>,
        components: Vec<ComponentManifest>,
        entry_triggers: Vec<ComponentEntryTrigger>,
        process_arguments: Vec<ComponentProcessArgument>,
        authority_ceiling: &Authority,
    ) -> Result<Self, GenerationResolutionError> {
        let mut plugins = plugins;
        plugins.sort_by(|left, right| left.id.cmp(&right.id));
        let mut components = components;
        components.sort_by(|left, right| left.id.cmp(&right.id));
        let mut entry_triggers = entry_triggers;
        entry_triggers.sort_by(entry_trigger_order);
        validate_entry_triggers(&components, &entry_triggers, authority_ceiling)?;
        let mut process_arguments = process_arguments;
        process_arguments.sort_by(process_argument_order);
        validate_process_arguments(&components, &process_arguments, authority_ceiling)?;
        let mut kernel_config = KernelConfig::new(plugins.clone())?;
        for (service, layers) in &self.layer_policies {
            kernel_config = kernel_config.with_layer_policy(service.clone(), layers.clone())?;
        }
        let durable_schemas =
            resolve_durable_schemas(self.durable_schemas.clone(), &kernel_config)?;
        let component_graph = ResolvedComponentGraph::compile_with_provider_policy(
            plugins.clone(),
            components.clone(),
            authority_ceiling,
            &self.provider_policy,
        )?;
        let generation = SemanticGeneration {
            plugins: &plugins,
            components: &components,
            entry_triggers: &entry_triggers,
            process_arguments: &process_arguments,
            durable_schemas: durable_schema_payload(&durable_schemas),
            resources: self.resources(),
            configuration: self.configuration.semantic_payload(),
            layer_policies: layer_policy_payload(&self.layer_policies),
            provider_policy: &self.provider_policy,
            authority_ceiling,
        }
        .identity();
        // Workflow definitions belong to their originating plugin revision.
        // Reusing a component ID does not transfer authorship of its topology.
        // Until artifact contributions can be reselected, carry a declaration
        // forward only when both the owning component and plugin manifest are
        // unchanged. A changed owner must publish its own new declaration.
        let retained_workflows = self
            .workflows
            .iter()
            .filter(|declaration| {
                let Some(previous) = self
                    .components
                    .iter()
                    .find(|component| component.id == declaration.owner)
                else {
                    return false;
                };
                if !components.iter().any(|component| component == previous) {
                    return false;
                }
                let Some(owner) = self
                    .plugins
                    .iter()
                    .find(|plugin| plugin.id == previous.owner)
                else {
                    return false;
                };
                plugins.iter().any(|plugin| plugin == owner)
            })
            .cloned()
            .collect::<Vec<_>>();
        Self {
            runtime: GenerationTopology::resolved(
                generation,
                kernel_config,
                component_graph,
                self.resources().to_vec(),
                entry_triggers.clone(),
            ),
            plugins,
            components,
            entry_triggers,
            process_arguments,
            workflows: Vec::new(),
            durable_schemas,
            configuration: self.configuration.clone(),
            layer_policies: self.layer_policies.clone(),
            provider_policy: self.provider_policy.clone(),
            authority_ceiling: authority_ceiling.clone(),
        }
        .with_workflows(retained_workflows)
    }
}

#[derive(Serialize)]
struct SemanticGeneration<'a> {
    plugins: &'a [PluginManifest],
    components: &'a [ComponentManifest],
    entry_triggers: &'a [ComponentEntryTrigger],
    process_arguments: &'a [ComponentProcessArgument],
    durable_schemas: serde_json::Value,
    resources: &'a [SkillResourceMetadata],
    configuration: serde_json::Value,
    layer_policies: serde_json::Value,
    provider_policy: &'a ProviderCompositionPolicy,
    authority_ceiling: &'a Authority,
}

impl SemanticGeneration<'_> {
    fn identity(&self) -> GenerationId {
        let encoded = serde_json::to_vec(self).expect("resolved generation metadata serializes");
        let digest = Sha256::digest(encoded);
        let mut identity = String::with_capacity(digest.len() * 2);
        for byte in digest {
            use std::fmt::Write as _;
            write!(&mut identity, "{byte:02x}").expect("writing to String cannot fail");
        }
        GenerationId::from(identity)
    }
}

fn resolve_durable_schemas(
    durable_schemas: impl IntoIterator<Item = DurableSchemaRegistration>,
    kernel_config: &KernelConfig,
) -> Result<Vec<DurableSchemaRegistration>, GenerationResolutionError> {
    let mut resolved = BTreeMap::new();
    for mut registration in durable_schemas {
        let namespace = registration.schema.namespace.clone();
        let Some(owner) = kernel_config.resource_owner(&namespace) else {
            return Err(GenerationResolutionError::UndeclaredDurableSchema {
                plugin: registration.owner,
                namespace,
            });
        };
        if owner != &registration.owner {
            return Err(GenerationResolutionError::DurableSchemaOwnerMismatch {
                plugin: registration.owner,
                namespace,
                owner: owner.clone(),
            });
        }
        registration
            .migrations
            .sort_by_key(|migration| (migration.from_version, migration.to_version));
        if resolved.insert(namespace.clone(), registration).is_some() {
            return Err(GenerationResolutionError::DuplicateDurableSchema(namespace));
        }
    }
    Ok(resolved.into_values().collect())
}

fn durable_schema_payload(durable_schemas: &[DurableSchemaRegistration]) -> serde_json::Value {
    serde_json::Value::Array(
        durable_schemas
            .iter()
            .map(|registration| {
                serde_json::json!({
                    "owner": registration.owner.as_str(),
                    "namespace": registration.schema.namespace.as_str(),
                    "version": registration.schema.version,
                    "required_features": registration
                        .schema
                        .required_features
                        .iter()
                        .map(|feature| backend_feature_name(*feature))
                        .collect::<Vec<_>>(),
                    "migrations": registration.migrations.iter().map(|migration| serde_json::json!({
                        "from_version": migration.from_version,
                        "to_version": migration.to_version,
                        "operations": migration.operations,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

fn backend_feature_name(feature: PersistenceBackendFeature) -> &'static str {
    match feature {
        PersistenceBackendFeature::Transactions => "transactions",
        PersistenceBackendFeature::UniqueKeys => "unique_keys",
        PersistenceBackendFeature::ForeignKeys => "foreign_keys",
        PersistenceBackendFeature::OrderedAppend => "ordered_append",
        PersistenceBackendFeature::IndexedRange => "indexed_range",
        PersistenceBackendFeature::Migrations => "migrations",
    }
}

fn resolve_resources(
    resources: impl IntoIterator<Item = SkillResourceMetadata>,
    components: &[ComponentManifest],
    authority_ceiling: &Authority,
) -> Result<Vec<SkillResourceMetadata>, GenerationResolutionError> {
    let mut selected = BTreeMap::new();
    for resource in resources {
        resource.validate_pre_activation().map_err(|error| {
            GenerationResolutionError::CompositionMetadata {
                resource: resource.identity.clone(),
                error,
            }
        })?;
        let identity = resource.identity.clone();
        if selected.insert(identity.clone(), resource).is_some() {
            return Err(GenerationResolutionError::DuplicateResource(identity));
        }
    }

    let exported_interfaces: BTreeSet<_> = components
        .iter()
        .flat_map(|component| {
            component
                .exports
                .iter()
                .map(|export| export.interface.clone())
        })
        .collect();
    for resource in selected.values() {
        for dependency in &resource.dependencies {
            if !selected.contains_key(dependency) {
                return Err(GenerationResolutionError::MissingResourceDependency {
                    resource: resource.identity.clone(),
                    dependency: dependency.clone(),
                });
            }
        }
        if let Some(conflict) = resource
            .conflicts
            .iter()
            .find(|conflict| selected.contains_key(*conflict))
        {
            return Err(GenerationResolutionError::ResourceConflict {
                resource: resource.identity.clone(),
                conflict: conflict.clone(),
            });
        }
        if let Some(interface) = resource
            .required_interfaces
            .iter()
            .find(|interface| !exported_interfaces.contains(*interface))
        {
            return Err(GenerationResolutionError::ResourceInterfaceUnavailable {
                resource: resource.identity.clone(),
                interface: interface.clone(),
            });
        }
        if let Some(capability) = resource
            .required_capabilities
            .iter()
            .find(|capability| !authority_ceiling.permits(capability))
        {
            return Err(GenerationResolutionError::ResourceAuthorityDenied {
                resource: resource.identity.clone(),
                capability: capability.clone(),
            });
        }
    }

    Ok(selected.into_values().collect())
}

fn layer_policy_payload(
    layer_policies: &BTreeMap<ServiceId, Vec<LayerPolicy>>,
) -> serde_json::Value {
    serde_json::Value::Array(
        layer_policies
            .iter()
            .map(|(service, layers)| {
                serde_json::json!({
                    "service": service.as_str(),
                    "layers": layers.iter().map(|layer| serde_json::json!({
                        "plugin": layer.plugin.as_str(),
                        "priority": layer.priority,
                        "required": layer.required,
                        "enabled": layer.enabled,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

fn entry_trigger_order(
    left: &ComponentEntryTrigger,
    right: &ComponentEntryTrigger,
) -> std::cmp::Ordering {
    left.component
        .cmp(&right.component)
        .then_with(|| left.interface.cmp(&right.interface))
        .then_with(|| match (&left.trigger, &right.trigger) {
            (
                EntryTriggerKind::ToolCall {
                    callable_id: left, ..
                },
                EntryTriggerKind::ToolCall {
                    callable_id: right, ..
                },
            ) => left.cmp(right),
        })
}

fn validate_entry_triggers(
    components: &[ComponentManifest],
    triggers: &[ComponentEntryTrigger],
    authority_ceiling: &Authority,
) -> Result<(), GenerationResolutionError> {
    let mut tool_ids = BTreeSet::new();
    for trigger in triggers {
        let Some(component) = components
            .iter()
            .find(|component| component.id == trigger.component)
        else {
            return Err(GenerationResolutionError::MissingEntryTriggerTarget {
                component: trigger.component.clone(),
                interface: trigger.interface.clone(),
            });
        };
        let Some(export) = component
            .exports
            .iter()
            .find(|export| export.interface == trigger.interface)
        else {
            return Err(GenerationResolutionError::MissingEntryTriggerTarget {
                component: trigger.component.clone(),
                interface: trigger.interface.clone(),
            });
        };
        if !authority_ceiling.permits_all(&trigger.required_authority)
            || !component
                .maximum_authority
                .permits_all(&trigger.required_authority)
            || !component
                .maximum_authority
                .permits_all(&export.required_authority)
            || !trigger
                .required_authority
                .permits_all(&export.required_authority)
        {
            return Err(GenerationResolutionError::EntryTriggerAuthorityDenied {
                component: trigger.component.clone(),
                interface: trigger.interface.clone(),
            });
        }
        match &trigger.trigger {
            EntryTriggerKind::ToolCall { callable_id, .. } => {
                if !tool_ids.insert(callable_id.clone()) {
                    return Err(GenerationResolutionError::DuplicateToolCallTrigger(
                        callable_id.clone(),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn process_argument_order(
    left: &ComponentProcessArgument,
    right: &ComponentProcessArgument,
) -> std::cmp::Ordering {
    left.component
        .cmp(&right.component)
        .then_with(|| left.interface.cmp(&right.interface))
        .then_with(|| left.name.cmp(&right.name))
}

fn validate_process_arguments(
    components: &[ComponentManifest],
    arguments: &[ComponentProcessArgument],
    authority_ceiling: &Authority,
) -> Result<(), GenerationResolutionError> {
    let mut names = BTreeSet::new();
    for argument in arguments {
        let Some(component) = components
            .iter()
            .find(|component| component.id == argument.component)
        else {
            return Err(GenerationResolutionError::MissingProcessArgumentTarget {
                component: argument.component.clone(),
                interface: argument.interface.clone(),
            });
        };
        let Some(export) = component
            .exports
            .iter()
            .find(|export| export.interface == argument.interface)
        else {
            return Err(GenerationResolutionError::MissingProcessArgumentTarget {
                component: argument.component.clone(),
                interface: argument.interface.clone(),
            });
        };
        if !authority_ceiling.permits_all(&argument.required_authority)
            || !component
                .maximum_authority
                .permits_all(&argument.required_authority)
            || !component
                .maximum_authority
                .permits_all(&export.required_authority)
            || !argument
                .required_authority
                .permits_all(&export.required_authority)
        {
            return Err(GenerationResolutionError::ProcessArgumentAuthorityDenied {
                component: argument.component.clone(),
                interface: argument.interface.clone(),
            });
        }

        let Some(body) = argument.name.strip_prefix("--") else {
            return Err(GenerationResolutionError::InvalidProcessArgument(
                argument.name.clone(),
            ));
        };
        let mut chars = body.chars();
        let valid = chars
            .next()
            .is_some_and(|first| first.is_ascii_alphanumeric())
            && chars.all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            });
        if !valid {
            return Err(GenerationResolutionError::InvalidProcessArgument(
                argument.name.clone(),
            ));
        }
        if !names.insert(argument.name.clone()) {
            return Err(GenerationResolutionError::DuplicateProcessArgument(
                argument.name.clone(),
            ));
        }
    }
    Ok(())
}

fn validate_layer_policies(
    plugins: &[PluginManifest],
    layer_policies: &BTreeMap<ServiceId, Vec<LayerPolicy>>,
    authority_ceiling: &Authority,
) -> Result<(), GenerationResolutionError> {
    for (service, layers) in layer_policies {
        let mut seen = BTreeSet::new();
        for layer in layers {
            if !seen.insert(layer.plugin.clone()) {
                return Err(GenerationResolutionError::DuplicateLayerPolicy {
                    service: service.clone(),
                    plugin: layer.plugin.clone(),
                });
            }
            if !(layer.enabled && layer.required) {
                continue;
            }
            let available = plugins.iter().any(|manifest| {
                manifest.id == layer.plugin
                    && manifest.services.iter().any(|contribution| {
                        contribution.service == *service
                            && contribution.role == ServiceRole::Layer
                            && authority_ceiling.permits_all(&contribution.required_authority)
                    })
            });
            if !available {
                return Err(GenerationResolutionError::RequiredLayerUnavailable {
                    service: service.clone(),
                    plugin: layer.plugin.clone(),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ComponentId;
    use crate::{
        CompatibilityMetadata, ComponentExport, ComponentImport, ConfigContributionSource,
        ConfigNamespace, ConfigSourceClass, DurableSchema, InterfaceId, PermissionId,
        PluginExecution, PluginId, ReloadPolicy,
    };
    use std::collections::BTreeSet;

    fn plugin(value: &str) -> PluginId {
        PluginId::parse(value).unwrap()
    }

    fn component(value: &str) -> ComponentId {
        ComponentId::parse(value).unwrap()
    }

    fn interface(value: &str) -> InterfaceId {
        InterfaceId::parse(value).unwrap()
    }

    fn capability(value: &str) -> PermissionId {
        PermissionId::parse(value).unwrap()
    }

    fn frontend(value: &str) -> ConfigurationFrontendId {
        ConfigurationFrontendId::parse(value).unwrap()
    }

    fn owner(id: &str, authority: Authority) -> PluginManifest {
        PluginManifest {
            id: plugin(id),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: authority,
        }
    }

    fn provider(authority: Authority) -> ComponentManifest {
        ComponentManifest {
            listeners: Vec::new(),
            id: component("provider"),
            owner: plugin("provider-owner"),
            imports: Vec::new(),
            exports: vec![ComponentExport {
                interface: interface("fixture.echo@1"),
                schema: Default::default(),
                priority: 1,
                required_authority: authority.clone(),
            }],
            maximum_authority: authority,
        }
    }

    fn consumer(authority: Authority) -> ComponentManifest {
        ComponentManifest {
            listeners: Vec::new(),
            id: component("consumer"),
            owner: plugin("consumer-owner"),
            imports: vec![ComponentImport {
                interface: interface("fixture.echo@1"),
                schema: Default::default(),
                required: true,
                authority: authority.clone(),
            }],
            exports: Vec::new(),
            maximum_authority: authority,
        }
    }

    fn resource(id: &str, content: &str) -> SkillResourceMetadata {
        SkillResourceMetadata {
            identity: id.into(),
            version: 1,
            content_identity: content.into(),
            dependencies: BTreeSet::new(),
            conflicts: BTreeSet::new(),
            triggers: BTreeSet::new(),
            scope: "execution".into(),
            priority: 0,
            required_tools: BTreeSet::new(),
            required_interfaces: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
            compatibility: CompatibilityMetadata {
                minimum_kernel_version: 1,
                maximum_kernel_version: None,
            },
            invalidation_targets: BTreeSet::from(["skill-index".into()]),
            reload_policy: ReloadPolicy::Restart,
        }
    }

    fn contribution(frontend_id: &str, source: &str, revision: &str) -> ConfigContribution {
        ConfigContribution {
            source: ConfigContributionSource {
                frontend: frontend(frontend_id),
                source_identity: source.into(),
                source_revision: revision.into(),
            },
            namespace: ConfigNamespace::parse("acme.engineering@1").unwrap(),
            contract_version: 1,
            precedence: 10,
            value: serde_json::json!({"review":"strict"}).into(),
            requested_authority: Authority::default(),
        }
    }

    fn frontend_metadata(id: &str, authority: Authority) -> ConfigurationFrontendMetadata {
        ConfigurationFrontendMetadata {
            id: frontend(id),
            version: 1,
            accepted_source_kinds: BTreeSet::from(["inline".into()]),
            exposed_namespaces: BTreeSet::from([
                ConfigNamespace::parse("acme.engineering@1").unwrap()
            ]),
            watch: true,
            required_authority: authority,
        }
    }

    fn frontend_contribution(id: &str) -> (ConfigurationFrontendId, FrontendConfigContribution) {
        (
            frontend(id),
            FrontendConfigContribution {
                source_kind: "inline".into(),
                source_identity: format!("{id}:fixture"),
                source_revision: "rev-1".into(),
                source_class: ConfigSourceClass::Materialized,
                namespace: ConfigNamespace::parse("acme.engineering@1").unwrap(),
                contract_version: 1,
                precedence: 10,
                value: serde_json::json!({"review":"strict"}).into(),
                requested_authority: Authority::default(),
            },
        )
    }

    #[test]
    fn equivalent_frontends_and_registration_order_resolve_to_one_semantic_generation() {
        let authority = Authority::new([capability("fixture.use")]);
        let first = ResolvedGeneration::resolve(
            [
                owner("provider-owner", authority.clone()),
                owner("consumer-owner", authority.clone()),
            ],
            [provider(authority.clone()), consumer(authority.clone())],
            [contribution("phenix-config-nix", "flake:acme", "a")],
            &authority,
        )
        .unwrap();
        let second = ResolvedGeneration::resolve(
            [
                owner("consumer-owner", authority.clone()),
                owner("provider-owner", authority.clone()),
            ],
            [consumer(authority.clone()), provider(authority.clone())],
            [contribution("phenix-config-lua", "file:phenix.lua", "b")],
            &authority,
        )
        .unwrap();

        assert_eq!(first.generation(), second.generation());
        assert_ne!(
            first.configuration().entries()[0].attributions,
            second.configuration().entries()[0].attributions
        );
    }

    #[test]
    fn configuration_conflicts_are_rejected_by_the_canonical_resolver() {
        let mut left = contribution("phenix-config-nix", "flake:one", "a");
        let mut right = contribution("phenix-config-lua", "file:two.lua", "b");
        left.value = serde_json::json!({"mode":"strict"}).into();
        right.value = serde_json::json!({"mode":"relaxed"}).into();

        assert_eq!(
            ResolvedGeneration::resolve([], [], [left, right], &Authority::default()).unwrap_err(),
            GenerationResolutionError::ConfigurationMerge(
                ConfigMergeError::ConflictingContributions {
                    namespace: ConfigNamespace::parse("acme.engineering@1").unwrap(),
                    contract_version: 1,
                    precedence: 10,
                }
            )
        );
    }

    #[test]
    fn frontend_metadata_is_enforced_before_canonical_resolution() {
        let read = capability("config.read");
        let metadata = frontend_metadata("phenix-config-lua", Authority::new([read.clone()]));
        let denied = ResolvedGeneration::resolve_frontends(
            [],
            [],
            [metadata.clone()],
            [frontend_contribution("phenix-config-lua")],
            &Authority::default(),
        )
        .unwrap_err();
        assert_eq!(
            denied,
            GenerationResolutionError::ConfigurationFrontend {
                frontend: frontend("phenix-config-lua"),
                error: FrontendConfigError::SourceAuthorityDenied,
            }
        );

        let resolved = ResolvedGeneration::resolve_frontends(
            [],
            [],
            [metadata],
            [frontend_contribution("phenix-config-lua")],
            &Authority::new([read]),
        )
        .unwrap();
        assert_eq!(
            resolved.configuration().entries()[0].attributions[0]
                .source
                .frontend
                .as_str(),
            "phenix-config-lua"
        );
    }

    #[test]
    fn equivalent_validated_frontends_share_one_semantic_generation() {
        let first = ResolvedGeneration::resolve_frontends(
            [],
            [],
            [frontend_metadata("phenix-config-nix", Authority::default())],
            [frontend_contribution("phenix-config-nix")],
            &Authority::default(),
        )
        .unwrap();
        let second = ResolvedGeneration::resolve_frontends(
            [],
            [],
            [frontend_metadata("phenix-config-lua", Authority::default())],
            [frontend_contribution("phenix-config-lua")],
            &Authority::default(),
        )
        .unwrap();

        assert_eq!(first.generation(), second.generation());
        assert_ne!(
            first.configuration().entries()[0].attributions,
            second.configuration().entries()[0].attributions
        );
    }

    #[test]
    fn resolved_generation_projects_one_generation_topology() {
        let resolved = ResolvedGeneration::resolve_with_resources(
            [],
            [],
            [resource("review", "sha256:one")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let runtime = resolved.generation_topology();

        assert_eq!(runtime.generation(), Some(resolved.generation()));
        assert!(std::ptr::eq(runtime.config(), resolved.kernel_config()));
        assert!(std::ptr::eq(
            runtime.component_graph(),
            resolved.component_graph()
        ));
        assert_eq!(runtime.resources(), resolved.resources());
    }

    #[test]
    fn resource_metadata_is_part_of_resolution_and_generation_identity() {
        let baseline = ResolvedGeneration::resolve_with_resources(
            [],
            [],
            [resource("review", "sha256:one")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let changed = ResolvedGeneration::resolve_with_resources(
            [],
            [],
            [resource("review", "sha256:two")],
            [],
            &Authority::default(),
        )
        .unwrap();

        assert_eq!(baseline.resources()[0].identity, "review");
        assert_ne!(baseline.generation(), changed.generation());
    }

    #[test]
    fn durable_schema_metadata_is_resolved_before_activation_and_changes_generation() {
        let owner_id = plugin("durable-owner");
        let namespace = ResourceNamespace::parse("durable.state").unwrap();
        let mut manifest = owner("durable-owner", Authority::default());
        manifest.resource_namespaces.push(namespace.clone());
        let baseline = ResolvedGeneration::resolve_with_durable_schemas(
            [manifest.clone()],
            [],
            [DurableSchemaRegistration::new(
                owner_id.clone(),
                DurableSchema::new(namespace.clone(), 1),
            )],
            [],
            &Authority::default(),
        )
        .unwrap();
        let changed = ResolvedGeneration::resolve_with_durable_schemas(
            [manifest],
            [],
            [DurableSchemaRegistration::new(
                owner_id,
                DurableSchema::requiring(
                    namespace.clone(),
                    2,
                    [PersistenceBackendFeature::Transactions],
                ),
            )],
            [],
            &Authority::default(),
        )
        .unwrap();

        assert_eq!(baseline.durable_schemas()[0].schema.version, 1);
        assert_ne!(baseline.generation(), changed.generation());
    }

    #[test]
    fn durable_schema_owner_is_validated_during_resolution() {
        let namespace = ResourceNamespace::parse("durable.state").unwrap();
        let mut manifest = owner("actual-owner", Authority::default());
        manifest.resource_namespaces.push(namespace.clone());
        let error = ResolvedGeneration::resolve_with_durable_schemas(
            [manifest],
            [],
            [DurableSchemaRegistration::new(
                plugin("wrong-owner"),
                DurableSchema::new(namespace.clone(), 1),
            )],
            [],
            &Authority::default(),
        )
        .unwrap_err();

        assert_eq!(
            error,
            GenerationResolutionError::DurableSchemaOwnerMismatch {
                plugin: plugin("wrong-owner"),
                namespace,
                owner: plugin("actual-owner"),
            }
        );
    }

    #[test]
    fn resource_dependencies_conflicts_interfaces_and_authority_fail_before_activation() {
        let mut missing_dependency = resource("review", "sha256:one");
        missing_dependency.dependencies.insert("tools".into());
        assert_eq!(
            ResolvedGeneration::resolve_with_resources(
                [],
                [],
                [missing_dependency],
                [],
                &Authority::default(),
            )
            .unwrap_err(),
            GenerationResolutionError::MissingResourceDependency {
                resource: "review".into(),
                dependency: "tools".into(),
            }
        );

        let mut review = resource("review", "sha256:one");
        review.conflicts.insert("tools".into());
        assert_eq!(
            ResolvedGeneration::resolve_with_resources(
                [],
                [],
                [review, resource("tools", "sha256:tools")],
                [],
                &Authority::default(),
            )
            .unwrap_err(),
            GenerationResolutionError::ResourceConflict {
                resource: "review".into(),
                conflict: "tools".into(),
            }
        );

        let required_interface = interface("fixture.resource@1");
        let mut needs_interface = resource("review", "sha256:one");
        needs_interface
            .required_interfaces
            .insert(required_interface.clone());
        assert_eq!(
            ResolvedGeneration::resolve_with_resources(
                [],
                [],
                [needs_interface],
                [],
                &Authority::default(),
            )
            .unwrap_err(),
            GenerationResolutionError::ResourceInterfaceUnavailable {
                resource: "review".into(),
                interface: required_interface,
            }
        );

        let required_capability = capability("workspace.read");
        let mut needs_authority = resource("review", "sha256:one");
        needs_authority
            .required_capabilities
            .insert(required_capability.clone());
        assert_eq!(
            ResolvedGeneration::resolve_with_resources(
                [],
                [],
                [needs_authority],
                [],
                &Authority::default(),
            )
            .unwrap_err(),
            GenerationResolutionError::ResourceAuthorityDenied {
                resource: "review".into(),
                capability: required_capability,
            }
        );
    }

    #[test]
    fn semantic_change_creates_a_new_generation() {
        let authority = Authority::default();
        let baseline = ResolvedGeneration::resolve(
            [owner("consumer-owner", Authority::default())],
            [ComponentManifest {
                listeners: Vec::new(),
                id: component("consumer"),
                owner: plugin("consumer-owner"),
                imports: Vec::new(),
                exports: Vec::new(),
                maximum_authority: Authority::default(),
            }],
            [contribution("phenix-config-nix", "flake:acme", "a")],
            &authority,
        )
        .unwrap();
        let mut changed = contribution("phenix-config-nix", "flake:acme", "b");
        changed.value = serde_json::json!({"review":"relaxed"}).into();
        let changed = ResolvedGeneration::resolve(
            [owner("consumer-owner", Authority::default())],
            [ComponentManifest {
                listeners: Vec::new(),
                id: component("consumer"),
                owner: plugin("consumer-owner"),
                imports: Vec::new(),
                exports: Vec::new(),
                maximum_authority: Authority::default(),
            }],
            [changed],
            &authority,
        )
        .unwrap();

        assert_ne!(baseline.generation(), changed.generation());
    }

    #[test]
    fn provider_policy_is_part_of_canonical_resolution_and_reconciliation() {
        let provider_interface = interface("fixture.echo@1");
        let provider_a = ComponentManifest {
            listeners: Vec::new(),
            id: component("provider-a"),
            owner: plugin("provider-a-owner"),
            imports: Vec::new(),
            exports: vec![ComponentExport {
                interface: provider_interface.clone(),
                schema: Default::default(),
                priority: 100,
                required_authority: Authority::default(),
            }],
            maximum_authority: Authority::default(),
        };
        let provider_b = ComponentManifest {
            id: component("provider-b"),
            owner: plugin("provider-b-owner"),
            ..provider_a.clone()
        };
        let consumer = consumer(Authority::default());
        let plugins = vec![
            owner("consumer-owner", Authority::default()),
            owner("provider-a-owner", Authority::default()),
            owner("provider-b-owner", Authority::default()),
        ];
        let components = vec![consumer.clone(), provider_a, provider_b];
        let baseline = ResolvedGeneration::resolve(
            plugins.clone(),
            components.clone(),
            [],
            &Authority::default(),
        )
        .unwrap();
        let policy = ProviderCompositionPolicy::new()
            .with_explicit_binding(provider_interface.clone(), component("provider-b"));
        let resolved = ResolvedGeneration::resolve_with_provider_policy(
            plugins,
            components,
            [],
            policy.clone(),
            &Authority::default(),
        )
        .unwrap();

        assert_eq!(resolved.provider_policy(), &policy);
        assert_ne!(baseline.generation(), resolved.generation());
        assert_eq!(
            resolved
                .component_graph()
                .provider_plan(&consumer.id, &provider_interface)
                .unwrap()
                .unwrap()
                .primary()
                .exporter(),
            &component("provider-b")
        );

        let sibling = resolved
            .with_plugin_set(
                resolved.plugins().to_vec(),
                resolved.components().to_vec(),
                resolved.entry_triggers().to_vec(),
                resolved.process_arguments().to_vec(),
                &Authority::default(),
            )
            .unwrap();
        assert_eq!(sibling.provider_policy(), &policy);
        assert_eq!(sibling.generation(), resolved.generation());
    }

    #[test]
    fn hard_import_cycles_are_rejected_with_the_concrete_path() {
        let left_interface = interface("fixture.left@1");
        let right_interface = interface("fixture.right@1");
        let components = [
            ComponentManifest {
                listeners: Vec::new(),
                id: component("left"),
                owner: plugin("left-owner"),
                imports: vec![ComponentImport {
                    interface: right_interface.clone(),
                    schema: Default::default(),
                    required: true,
                    authority: Authority::default(),
                }],
                exports: vec![ComponentExport {
                    interface: left_interface.clone(),
                    schema: Default::default(),
                    priority: 1,
                    required_authority: Authority::default(),
                }],
                maximum_authority: Authority::default(),
            },
            ComponentManifest {
                listeners: Vec::new(),
                id: component("right"),
                owner: plugin("right-owner"),
                imports: vec![ComponentImport {
                    interface: left_interface,
                    schema: Default::default(),
                    required: true,
                    authority: Authority::default(),
                }],
                exports: vec![ComponentExport {
                    interface: right_interface,
                    schema: Default::default(),
                    priority: 1,
                    required_authority: Authority::default(),
                }],
                maximum_authority: Authority::default(),
            },
        ];
        let error = ResolvedGeneration::resolve(
            [
                owner("left-owner", Authority::default()),
                owner("right-owner", Authority::default()),
            ],
            components,
            [],
            &Authority::default(),
        )
        .unwrap_err();

        assert_eq!(
            error,
            GenerationResolutionError::ComponentGraph(ComponentGraphError::RequiredImportCycle {
                path: vec![component("left"), component("right"), component("left")]
            })
        );
        assert_eq!(
            error.to_string(),
            "required component import cycle: left -> right -> left"
        );
    }

    #[test]
    fn duplicate_layer_policy_is_rejected_before_graph_resolution() {
        let service = ServiceId::parse("fixture.layered@1").unwrap();
        let layer = PluginId::parse("layer").unwrap();
        let policy = LayerPolicy {
            plugin: layer.clone(),
            priority: 10,
            required: false,
            enabled: true,
        };
        let error = ResolvedGeneration::resolve_with_layer_policies(
            [],
            [],
            [],
            BTreeMap::from([(service.clone(), vec![policy.clone(), policy])]),
            &Authority::default(),
        )
        .unwrap_err();
        assert_eq!(
            error,
            GenerationResolutionError::DuplicateLayerPolicy {
                service,
                plugin: layer,
            }
        );
    }

    #[test]
    fn required_layer_must_fit_the_resolved_authority_ceiling() {
        let service = ServiceId::parse("fixture.layered@1").unwrap();
        let layer = PluginId::parse("layer").unwrap();
        let layer_authority = Authority::new([capability("fixture.layer")]);
        let manifest = PluginManifest {
            id: layer.clone(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: vec![crate::ServiceContribution {
                role: ServiceRole::Layer,
                service: service.clone(),
                priority: 10,
                required_authority: layer_authority.clone(),
            }],
            resource_namespaces: Vec::new(),
            maximum_authority: layer_authority,
        };
        let error = ResolvedGeneration::resolve_with_layer_policies(
            [manifest],
            [],
            [],
            BTreeMap::from([(
                service.clone(),
                vec![LayerPolicy {
                    plugin: layer.clone(),
                    priority: 10,
                    required: true,
                    enabled: true,
                }],
            )]),
            &Authority::default(),
        )
        .unwrap_err();
        assert_eq!(
            error,
            GenerationResolutionError::RequiredLayerUnavailable {
                service,
                plugin: layer,
            }
        );
    }

    #[test]
    fn required_layer_must_be_declared_for_the_same_service() {
        let service = ServiceId::parse("fixture.layered@1").unwrap();
        let layer = PluginId::parse("layer").unwrap();
        let error = ResolvedGeneration::resolve_with_layer_policies(
            [],
            [],
            [],
            BTreeMap::from([(
                service.clone(),
                vec![LayerPolicy {
                    plugin: layer.clone(),
                    priority: 10,
                    required: true,
                    enabled: true,
                }],
            )]),
            &Authority::default(),
        )
        .unwrap_err();
        assert_eq!(
            error,
            GenerationResolutionError::RequiredLayerUnavailable {
                service,
                plugin: layer,
            }
        );
    }
}

#[cfg(test)]
mod entry_trigger_tests {
    use super::*;
    use crate::{CallableId, ComponentExport, ComponentId, InterfaceSchema, PluginExecution};

    fn plugin() -> PluginManifest {
        PluginManifest {
            id: PluginId::parse("fixture.trigger").unwrap(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::new([PermissionId::parse("workspace.shell").unwrap()]),
        }
    }

    fn component() -> ComponentManifest {
        let authority = Authority::new([PermissionId::parse("workspace.shell").unwrap()]);
        ComponentManifest {
            id: ComponentId::parse("fixture.trigger.component").unwrap(),
            owner: plugin().id,
            imports: Vec::new(),
            exports: vec![ComponentExport {
                interface: InterfaceId::parse("fixture.trigger.shell@1").unwrap(),
                schema: InterfaceSchema::of::<String, String>(),
                priority: 100,
                required_authority: authority.clone(),
            }],
            listeners: Vec::new(),
            maximum_authority: authority,
        }
    }

    fn trigger(id: &str) -> ComponentEntryTrigger {
        ComponentEntryTrigger {
            component: ComponentId::parse("fixture.trigger.component").unwrap(),
            interface: InterfaceId::parse("fixture.trigger.shell@1").unwrap(),
            trigger: EntryTriggerKind::ToolCall {
                callable_id: CallableId::parse(id).unwrap(),
                description: "fixture".into(),
            },
            required_authority: Authority::new([PermissionId::parse("workspace.shell").unwrap()]),
        }
    }

    fn process_argument(name: &str) -> ComponentProcessArgument {
        ComponentProcessArgument {
            component: ComponentId::parse("fixture.trigger.component").unwrap(),
            interface: InterfaceId::parse("fixture.trigger.shell@1").unwrap(),
            name: name.into(),
            takes_value: true,
            description: "fixture process argument".into(),
            required_authority: Authority::new([PermissionId::parse("workspace.shell").unwrap()]),
        }
    }

    fn resolve_process_arguments(
        arguments: impl IntoIterator<Item = ComponentProcessArgument>,
    ) -> Result<ResolvedGeneration, GenerationResolutionError> {
        ResolvedGeneration::resolve_with_durable_schemas_layer_policies_entry_triggers_and_process_arguments(
            [plugin()],
            [component()],
            [],
            [],
            arguments,
            [],
            BTreeMap::new(),
            &Authority::new([PermissionId::parse("workspace.shell").unwrap()]),
        )
    }

    fn resolve(
        triggers: impl IntoIterator<Item = ComponentEntryTrigger>,
    ) -> Result<ResolvedGeneration, GenerationResolutionError> {
        ResolvedGeneration::resolve_with_durable_schemas_layer_policies_and_entry_triggers(
            [plugin()],
            [component()],
            [],
            triggers,
            [],
            BTreeMap::new(),
            &Authority::new([PermissionId::parse("workspace.shell").unwrap()]),
        )
    }

    #[test]
    fn entry_trigger_is_part_of_resolved_generation() {
        let first = resolve([trigger("bash")]).unwrap();
        let second = resolve([]).unwrap();

        assert_eq!(first.entry_triggers(), &[trigger("bash")]);
        assert_ne!(first.generation(), second.generation());
    }

    #[test]
    fn duplicate_tool_call_ids_fail_resolution() {
        let mut second = trigger("bash");
        second.component = ComponentId::parse("fixture.trigger.component").unwrap();

        assert!(matches!(
            resolve([trigger("bash"), second]),
            Err(GenerationResolutionError::DuplicateToolCallTrigger(id)) if id.as_str() == "bash"
        ));
    }

    #[test]
    fn process_arguments_are_resolved_metadata() {
        let resolved =
            resolve_process_arguments([process_argument("--plugin-handled-value")]).unwrap();
        assert_eq!(
            resolved.process_arguments(),
            &[process_argument("--plugin-handled-value")]
        );
    }

    #[test]
    fn duplicate_process_arguments_fail_resolution() {
        assert!(matches!(
            resolve_process_arguments([
                process_argument("--plugin-handled-value"),
                process_argument("--plugin-handled-value")
            ]),
            Err(GenerationResolutionError::DuplicateProcessArgument(argument))
                if argument == "--plugin-handled-value"
        ));
    }

    #[test]
    fn malformed_process_arguments_fail_resolution() {
        assert!(matches!(
            resolve_process_arguments([process_argument("plugin-handled-value")]),
            Err(GenerationResolutionError::InvalidProcessArgument(argument))
                if argument == "plugin-handled-value"
        ));
    }

    #[test]
    fn trigger_must_target_a_resolved_export() {
        let mut missing = trigger("bash");
        missing.interface = InterfaceId::parse("fixture.trigger.missing@1").unwrap();

        assert!(matches!(
            resolve([missing]),
            Err(GenerationResolutionError::MissingEntryTriggerTarget { .. })
        ));
    }

    #[test]
    fn trigger_authority_must_cover_target_export() {
        let mut underpowered = trigger("bash");
        underpowered.required_authority = Authority::default();

        assert!(matches!(
            resolve([underpowered]),
            Err(GenerationResolutionError::EntryTriggerAuthorityDenied { .. })
        ));
    }
}
