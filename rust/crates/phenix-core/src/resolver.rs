use crate::{
    Authority, BackendFeature, CapabilityId, ComponentGraphError, ComponentManifest,
    CompositionMetadataError, ConfigContribution, ConfigMergeError, ConfigurationFrontendId,
    ConfigurationFrontendMetadata, DurableSchemaRegistration, FrontendConfigContribution,
    FrontendConfigError, InterfaceId, KernelConfig, KernelError, LayerPolicy, PluginId,
    PluginManifest, ProviderCompositionPolicy, ResolvedComponentGraph, ResolvedConfigContributions,
    ResourceNamespace, ServiceId, ServiceRole, SkillResourceMetadata,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt::{self, Display, Formatter},
};

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct GraphGenerationId(String);

impl GraphGenerationId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn incorporate_semantic_metadata<T: Serialize>(&mut self, metadata: &T) {
        let bytes = serde_json::to_vec(&(self.as_str(), metadata))
            .expect("resolved composition metadata is serializable");
        self.0 = format!("sha256:{:x}", Sha256::digest(bytes));
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedHarnessError {
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
        capability: CapabilityId,
    },
    DuplicateLayerPolicy {
        service: ServiceId,
        plugin: PluginId,
    },
    RequiredLayerUnavailable {
        service: ServiceId,
        plugin: PluginId,
    },
}

impl Display for ResolvedHarnessError {
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

impl Error for ResolvedHarnessError {}

impl From<ComponentGraphError> for ResolvedHarnessError {
    fn from(error: ComponentGraphError) -> Self {
        Self::ComponentGraph(error)
    }
}

impl From<ConfigMergeError> for ResolvedHarnessError {
    fn from(error: ConfigMergeError) -> Self {
        Self::ConfigurationMerge(error)
    }
}

impl From<KernelError> for ResolvedHarnessError {
    fn from(error: KernelError) -> Self {
        Self::Kernel(error)
    }
}

#[derive(Clone, Debug)]
pub struct ResolvedHarness {
    generation: GraphGenerationId,
    plugins: Vec<PluginManifest>,
    components: Vec<ComponentManifest>,
    durable_schemas: Vec<DurableSchemaRegistration>,
    resources: Vec<SkillResourceMetadata>,
    configuration: ResolvedConfigContributions,
    kernel_config: KernelConfig,
    component_graph: ResolvedComponentGraph,
    layer_policies: BTreeMap<ServiceId, Vec<LayerPolicy>>,
    provider_policy: ProviderCompositionPolicy,
}

struct ResolutionInputs {
    durable_schemas: Vec<DurableSchemaRegistration>,
    resources: Vec<SkillResourceMetadata>,
    contributions: Vec<ConfigContribution>,
    layer_policies: BTreeMap<ServiceId, Vec<LayerPolicy>>,
    provider_policy: ProviderCompositionPolicy,
}

impl ResolutionInputs {
    fn new(
        durable_schemas: impl IntoIterator<Item = DurableSchemaRegistration>,
        resources: impl IntoIterator<Item = SkillResourceMetadata>,
        contributions: impl IntoIterator<Item = ConfigContribution>,
        layer_policies: BTreeMap<ServiceId, Vec<LayerPolicy>>,
        provider_policy: ProviderCompositionPolicy,
    ) -> Self {
        Self {
            durable_schemas: durable_schemas.into_iter().collect(),
            resources: resources.into_iter().collect(),
            contributions: contributions.into_iter().collect(),
            layer_policies,
            provider_policy,
        }
    }
}

impl ResolvedHarness {
    pub fn resolve(
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        contributions: impl IntoIterator<Item = ConfigContribution>,
        authority_ceiling: &Authority,
    ) -> Result<Self, ResolvedHarnessError> {
        Self::resolve_with_inputs(
            plugin_manifests,
            component_manifests,
            ResolutionInputs::new(
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
    ) -> Result<Self, ResolvedHarnessError> {
        Self::resolve_with_inputs(
            plugin_manifests,
            component_manifests,
            ResolutionInputs::new(
                durable_schemas,
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
    ) -> Result<Self, ResolvedHarnessError> {
        Self::resolve_with_inputs(
            plugin_manifests,
            component_manifests,
            ResolutionInputs::new([], [], contributions, BTreeMap::new(), provider_policy),
            authority_ceiling,
        )
    }

    pub fn resolve_with_resources(
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        resources: impl IntoIterator<Item = SkillResourceMetadata>,
        contributions: impl IntoIterator<Item = ConfigContribution>,
        authority_ceiling: &Authority,
    ) -> Result<Self, ResolvedHarnessError> {
        Self::resolve_with_inputs(
            plugin_manifests,
            component_manifests,
            ResolutionInputs::new(
                [],
                resources,
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
    ) -> Result<Self, ResolvedHarnessError> {
        Self::resolve_with_inputs(
            plugin_manifests,
            component_manifests,
            ResolutionInputs::new(
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
    ) -> Result<Self, ResolvedHarnessError> {
        Self::resolve_with_inputs(
            plugin_manifests,
            component_manifests,
            ResolutionInputs::new(
                durable_schemas,
                [],
                contributions,
                layer_policies,
                ProviderCompositionPolicy::default(),
            ),
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
    ) -> Result<Self, ResolvedHarnessError> {
        Self::resolve_with_inputs(
            plugin_manifests,
            component_manifests,
            ResolutionInputs::new(
                [],
                resources,
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
    ) -> Result<Self, ResolvedHarnessError> {
        let mut plugins: Vec<_> = plugin_manifests.into_iter().collect();
        plugins.sort_by(|left, right| left.id.cmp(&right.id));
        let mut components: Vec<_> = component_manifests.into_iter().collect();
        components.sort_by(|left, right| left.id.cmp(&right.id));
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
            durable_schemas: durable_schema_payload(&durable_schemas),
            resources: &resources,
            configuration: configuration.semantic_payload(),
            layer_policies: layer_policy_payload(&inputs.layer_policies),
            provider_policy: &inputs.provider_policy,
            authority_ceiling,
        }
        .identity();

        Ok(Self {
            generation,
            plugins,
            components,
            durable_schemas,
            resources,
            configuration,
            kernel_config,
            component_graph,
            layer_policies: inputs.layer_policies,
            provider_policy: inputs.provider_policy,
        })
    }

    pub fn resolve_frontends(
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        frontend_metadata: impl IntoIterator<Item = ConfigurationFrontendMetadata>,
        contributions: impl IntoIterator<Item = (ConfigurationFrontendId, FrontendConfigContribution)>,
        authority_ceiling: &Authority,
    ) -> Result<Self, ResolvedHarnessError> {
        let mut frontends = BTreeMap::new();
        for metadata in frontend_metadata {
            let id = metadata.id.clone();
            if frontends.insert(id.clone(), metadata).is_some() {
                return Err(ResolvedHarnessError::DuplicateConfigurationFrontend(id));
            }
        }

        let lowered = contributions
            .into_iter()
            .map(|(frontend, contribution)| {
                let metadata = frontends.get(&frontend).ok_or_else(|| {
                    ResolvedHarnessError::UnknownConfigurationFrontend(frontend.clone())
                })?;
                contribution
                    .lower(metadata, authority_ceiling)
                    .map_err(|error| ResolvedHarnessError::ConfigurationFrontend {
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

    pub fn generation(&self) -> &GraphGenerationId {
        &self.generation
    }

    pub fn plugins(&self) -> &[PluginManifest] {
        &self.plugins
    }

    pub fn components(&self) -> &[ComponentManifest] {
        &self.components
    }

    pub fn durable_schemas(&self) -> &[DurableSchemaRegistration] {
        &self.durable_schemas
    }

    pub fn resources(&self) -> &[SkillResourceMetadata] {
        &self.resources
    }

    pub fn configuration(&self) -> &ResolvedConfigContributions {
        &self.configuration
    }

    pub fn kernel_config(&self) -> &KernelConfig {
        &self.kernel_config
    }

    pub fn component_graph(&self) -> &ResolvedComponentGraph {
        &self.component_graph
    }

    pub fn layer_policies(&self) -> &BTreeMap<ServiceId, Vec<LayerPolicy>> {
        &self.layer_policies
    }

    pub fn provider_policy(&self) -> &ProviderCompositionPolicy {
        &self.provider_policy
    }

    pub(crate) fn incorporate_semantic_metadata<T: Serialize>(&mut self, metadata: &T) {
        self.generation.incorporate_semantic_metadata(metadata);
    }

    /// Re-resolve a sibling harness that shares this harness's resources,
    /// configuration, layer policies, and provider policy but uses a complete
    /// desired plugin and component set. Kernel plugin management uses this to
    /// turn a desired plugin set into one atomic candidate generation.
    pub(crate) fn with_plugin_set(
        &self,
        plugins: Vec<PluginManifest>,
        components: Vec<ComponentManifest>,
        authority_ceiling: &Authority,
    ) -> Result<Self, ResolvedHarnessError> {
        let mut plugins = plugins;
        plugins.sort_by(|left, right| left.id.cmp(&right.id));
        let mut components = components;
        components.sort_by(|left, right| left.id.cmp(&right.id));
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
            durable_schemas: durable_schema_payload(&durable_schemas),
            resources: &self.resources,
            configuration: self.configuration.semantic_payload(),
            layer_policies: layer_policy_payload(&self.layer_policies),
            provider_policy: &self.provider_policy,
            authority_ceiling,
        }
        .identity();
        Ok(Self {
            generation,
            plugins,
            components,
            durable_schemas,
            resources: self.resources.clone(),
            configuration: self.configuration.clone(),
            kernel_config,
            component_graph,
            layer_policies: self.layer_policies.clone(),
            provider_policy: self.provider_policy.clone(),
        })
    }
}

#[derive(Serialize)]
struct SemanticGeneration<'a> {
    plugins: &'a [PluginManifest],
    components: &'a [ComponentManifest],
    durable_schemas: serde_json::Value,
    resources: &'a [SkillResourceMetadata],
    configuration: serde_json::Value,
    layer_policies: serde_json::Value,
    provider_policy: &'a ProviderCompositionPolicy,
    authority_ceiling: &'a Authority,
}

impl SemanticGeneration<'_> {
    fn identity(&self) -> GraphGenerationId {
        let encoded = serde_json::to_vec(self).expect("resolved generation metadata serializes");
        let digest = Sha256::digest(encoded);
        let mut identity = String::with_capacity(digest.len() * 2);
        for byte in digest {
            use std::fmt::Write as _;
            write!(&mut identity, "{byte:02x}").expect("writing to String cannot fail");
        }
        GraphGenerationId(identity)
    }
}

fn resolve_durable_schemas(
    durable_schemas: impl IntoIterator<Item = DurableSchemaRegistration>,
    kernel_config: &KernelConfig,
) -> Result<Vec<DurableSchemaRegistration>, ResolvedHarnessError> {
    let mut resolved = BTreeMap::new();
    for mut registration in durable_schemas {
        let namespace = registration.schema.namespace.clone();
        let Some(owner) = kernel_config.resource_owner(&namespace) else {
            return Err(ResolvedHarnessError::UndeclaredDurableSchema {
                plugin: registration.owner,
                namespace,
            });
        };
        if owner != &registration.owner {
            return Err(ResolvedHarnessError::DurableSchemaOwnerMismatch {
                plugin: registration.owner,
                namespace,
                owner: owner.clone(),
            });
        }
        registration
            .migrations
            .sort_by_key(|migration| (migration.from_version, migration.to_version));
        if resolved.insert(namespace.clone(), registration).is_some() {
            return Err(ResolvedHarnessError::DuplicateDurableSchema(namespace));
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

fn backend_feature_name(feature: BackendFeature) -> &'static str {
    match feature {
        BackendFeature::Transactions => "transactions",
        BackendFeature::UniqueKeys => "unique_keys",
        BackendFeature::ForeignKeys => "foreign_keys",
        BackendFeature::OrderedAppend => "ordered_append",
        BackendFeature::IndexedRange => "indexed_range",
        BackendFeature::Migrations => "migrations",
    }
}

fn resolve_resources(
    resources: impl IntoIterator<Item = SkillResourceMetadata>,
    components: &[ComponentManifest],
    authority_ceiling: &Authority,
) -> Result<Vec<SkillResourceMetadata>, ResolvedHarnessError> {
    let mut selected = BTreeMap::new();
    for resource in resources {
        resource.validate_pre_activation().map_err(|error| {
            ResolvedHarnessError::CompositionMetadata {
                resource: resource.identity.clone(),
                error,
            }
        })?;
        let identity = resource.identity.clone();
        if selected.insert(identity.clone(), resource).is_some() {
            return Err(ResolvedHarnessError::DuplicateResource(identity));
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
                return Err(ResolvedHarnessError::MissingResourceDependency {
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
            return Err(ResolvedHarnessError::ResourceConflict {
                resource: resource.identity.clone(),
                conflict: conflict.clone(),
            });
        }
        if let Some(interface) = resource
            .required_interfaces
            .iter()
            .find(|interface| !exported_interfaces.contains(*interface))
        {
            return Err(ResolvedHarnessError::ResourceInterfaceUnavailable {
                resource: resource.identity.clone(),
                interface: interface.clone(),
            });
        }
        if let Some(capability) = resource
            .required_capabilities
            .iter()
            .find(|capability| !authority_ceiling.permits(capability))
        {
            return Err(ResolvedHarnessError::ResourceAuthorityDenied {
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

fn validate_layer_policies(
    plugins: &[PluginManifest],
    layer_policies: &BTreeMap<ServiceId, Vec<LayerPolicy>>,
    authority_ceiling: &Authority,
) -> Result<(), ResolvedHarnessError> {
    for (service, layers) in layer_policies {
        let mut seen = BTreeSet::new();
        for layer in layers {
            if !seen.insert(layer.plugin.clone()) {
                return Err(ResolvedHarnessError::DuplicateLayerPolicy {
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
                return Err(ResolvedHarnessError::RequiredLayerUnavailable {
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
        CapabilityId, CompatibilityMetadata, ComponentExport, ComponentImport,
        ConfigContributionSource, ConfigNamespace, ConfigSourceClass, DurableSchema, InterfaceId,
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

    fn capability(value: &str) -> CapabilityId {
        CapabilityId::parse(value).unwrap()
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
        let first = ResolvedHarness::resolve(
            [
                owner("provider-owner", authority.clone()),
                owner("consumer-owner", authority.clone()),
            ],
            [provider(authority.clone()), consumer(authority.clone())],
            [contribution("phenix-config-nix", "flake:acme", "a")],
            &authority,
        )
        .unwrap();
        let second = ResolvedHarness::resolve(
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
            ResolvedHarness::resolve([], [], [left, right], &Authority::default()).unwrap_err(),
            ResolvedHarnessError::ConfigurationMerge(ConfigMergeError::ConflictingContributions {
                namespace: ConfigNamespace::parse("acme.engineering@1").unwrap(),
                contract_version: 1,
                precedence: 10,
            })
        );
    }

    #[test]
    fn frontend_metadata_is_enforced_before_canonical_resolution() {
        let read = capability("config.read");
        let metadata = frontend_metadata("phenix-config-lua", Authority::new([read.clone()]));
        let denied = ResolvedHarness::resolve_frontends(
            [],
            [],
            [metadata.clone()],
            [frontend_contribution("phenix-config-lua")],
            &Authority::default(),
        )
        .unwrap_err();
        assert_eq!(
            denied,
            ResolvedHarnessError::ConfigurationFrontend {
                frontend: frontend("phenix-config-lua"),
                error: FrontendConfigError::SourceAuthorityDenied,
            }
        );

        let resolved = ResolvedHarness::resolve_frontends(
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
        let first = ResolvedHarness::resolve_frontends(
            [],
            [],
            [frontend_metadata("phenix-config-nix", Authority::default())],
            [frontend_contribution("phenix-config-nix")],
            &Authority::default(),
        )
        .unwrap();
        let second = ResolvedHarness::resolve_frontends(
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
    fn resource_metadata_is_part_of_resolution_and_generation_identity() {
        let baseline = ResolvedHarness::resolve_with_resources(
            [],
            [],
            [resource("review", "sha256:one")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let changed = ResolvedHarness::resolve_with_resources(
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
        let baseline = ResolvedHarness::resolve_with_durable_schemas(
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
        let changed = ResolvedHarness::resolve_with_durable_schemas(
            [manifest],
            [],
            [DurableSchemaRegistration::new(
                owner_id,
                DurableSchema::requiring(namespace.clone(), 2, [BackendFeature::Transactions]),
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
        let error = ResolvedHarness::resolve_with_durable_schemas(
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
            ResolvedHarnessError::DurableSchemaOwnerMismatch {
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
            ResolvedHarness::resolve_with_resources(
                [],
                [],
                [missing_dependency],
                [],
                &Authority::default(),
            )
            .unwrap_err(),
            ResolvedHarnessError::MissingResourceDependency {
                resource: "review".into(),
                dependency: "tools".into(),
            }
        );

        let mut review = resource("review", "sha256:one");
        review.conflicts.insert("tools".into());
        assert_eq!(
            ResolvedHarness::resolve_with_resources(
                [],
                [],
                [review, resource("tools", "sha256:tools")],
                [],
                &Authority::default(),
            )
            .unwrap_err(),
            ResolvedHarnessError::ResourceConflict {
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
            ResolvedHarness::resolve_with_resources(
                [],
                [],
                [needs_interface],
                [],
                &Authority::default(),
            )
            .unwrap_err(),
            ResolvedHarnessError::ResourceInterfaceUnavailable {
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
            ResolvedHarness::resolve_with_resources(
                [],
                [],
                [needs_authority],
                [],
                &Authority::default(),
            )
            .unwrap_err(),
            ResolvedHarnessError::ResourceAuthorityDenied {
                resource: "review".into(),
                capability: required_capability,
            }
        );
    }

    #[test]
    fn semantic_change_creates_a_new_generation() {
        let authority = Authority::default();
        let baseline = ResolvedHarness::resolve(
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
        let changed = ResolvedHarness::resolve(
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
        let baseline = ResolvedHarness::resolve(
            plugins.clone(),
            components.clone(),
            [],
            &Authority::default(),
        )
        .unwrap();
        let policy = ProviderCompositionPolicy::new()
            .with_explicit_binding(provider_interface.clone(), component("provider-b"));
        let resolved = ResolvedHarness::resolve_with_provider_policy(
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
        let error = ResolvedHarness::resolve(
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
            ResolvedHarnessError::ComponentGraph(ComponentGraphError::RequiredImportCycle {
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
        let error = ResolvedHarness::resolve_with_layer_policies(
            [],
            [],
            [],
            BTreeMap::from([(service.clone(), vec![policy.clone(), policy])]),
            &Authority::default(),
        )
        .unwrap_err();
        assert_eq!(
            error,
            ResolvedHarnessError::DuplicateLayerPolicy {
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
        let error = ResolvedHarness::resolve_with_layer_policies(
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
            ResolvedHarnessError::RequiredLayerUnavailable {
                service,
                plugin: layer,
            }
        );
    }

    #[test]
    fn required_layer_must_be_declared_for_the_same_service() {
        let service = ServiceId::parse("fixture.layered@1").unwrap();
        let layer = PluginId::parse("layer").unwrap();
        let error = ResolvedHarness::resolve_with_layer_policies(
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
            ResolvedHarnessError::RequiredLayerUnavailable {
                service,
                plugin: layer,
            }
        );
    }
}
