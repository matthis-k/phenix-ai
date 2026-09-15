use crate::{
    ArtifactRevision, Authority, ComponentGraphError, ComponentId, EventError, PluginExecution,
    PluginId, PluginManifest, ResourceNamespace, RuntimeId, ServiceId, ServiceRole,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt::{self, Display, Formatter},
    sync::atomic::{AtomicU64, Ordering},
};

pub const EMBEDDED_RUNTIME: &str = "embedded";
pub const RUNTIME_PROVIDER_SERVICE_PREFIX: &str = "phenix.kernel.runtime-provider/";
const RUNTIME_PROVIDER_SERVICE_VERSION: &str = "@1";

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct RuntimeBinding {
    pub guest: PluginId,
    pub runtime: RuntimeId,
    pub provider: PluginId,
    pub artifact_revision: ArtifactRevision,
}

#[must_use]
pub fn runtime_provider_service(runtime: &RuntimeId) -> ServiceId {
    ServiceId::parse(format!(
        "{RUNTIME_PROVIDER_SERVICE_PREFIX}{}{RUNTIME_PROVIDER_SERVICE_VERSION}",
        runtime.as_str()
    ))
    .expect("runtime provider service id is derived from a validated runtime id")
}

#[must_use]
pub fn runtime_provider_runtime(service: &ServiceId) -> Option<RuntimeId> {
    let runtime = service
        .as_str()
        .strip_prefix(RUNTIME_PROVIDER_SERVICE_PREFIX)?
        .strip_suffix(RUNTIME_PROVIDER_SERVICE_VERSION)?;
    RuntimeId::parse(runtime).ok()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelError {
    DuplicatePlugin(PluginId),
    UnknownDependency {
        plugin: PluginId,
        dependency: PluginId,
    },
    DependencyCycle(PluginId),
    DuplicateResourceNamespace(ResourceNamespace),
    DuplicateServiceContribution {
        plugin: PluginId,
        service: ServiceId,
    },
    ResourceOnlyService(PluginId),
    UnknownPlugin(PluginId),
    NoEligibleProvider(ServiceId),
    BoundProviderUnavailable {
        service: ServiceId,
        plugin: PluginId,
    },
    DuplicateLayerPolicy {
        service: ServiceId,
        plugin: PluginId,
    },
    RequiredLayerUnavailable {
        service: ServiceId,
        plugin: PluginId,
    },
    ContinuationUnavailable,
    ContinuationAlreadyUsed(ServiceId),
    CausalServiceReentry(ServiceId),
    ServiceDenied {
        plugin: PluginId,
        service: ServiceId,
        message: String,
    },
    ServiceCancelled {
        plugin: PluginId,
        service: ServiceId,
    },
    PluginNotActive(PluginId),
    HostOperationDenied {
        plugin: PluginId,
        operation: String,
    },
    Persistence {
        plugin: PluginId,
        message: String,
    },
    EmbeddedFactoryMissing(PluginId),
    WrongExecutionKind(PluginId),
    ComponentGraph(ComponentGraphError),
    RuntimeProviderUnavailable(RuntimeId),
    DuplicateRuntimeProvider {
        runtime: RuntimeId,
        first: PluginId,
        second: PluginId,
    },
    ReservedRuntimeProvider(PluginId),
    RuntimeProviderNotExecutable {
        runtime: RuntimeId,
        provider: PluginId,
    },
    RuntimeProviderContractUnavailable {
        runtime: RuntimeId,
        provider: PluginId,
    },
    RuntimePrepare {
        plugin: PluginId,
        runtime: RuntimeId,
        message: String,
    },
    PluginStart {
        plugin: PluginId,
        message: String,
    },
    ListenerBinding {
        plugin: PluginId,
        component: ComponentId,
        method: String,
        message: String,
    },
    EventTopology(EventError),
    ResolvedGenerationMissing,
    PluginStop {
        plugin: PluginId,
        message: String,
    },
    PartiallyActiveRuntime,
    ServiceInvoke {
        plugin: PluginId,
        service: ServiceId,
        message: String,
    },
}

impl Display for KernelError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicatePlugin(plugin) => write!(f, "duplicate plugin: {plugin}"),
            Self::UnknownDependency { plugin, dependency } => {
                write!(f, "plugin {plugin} depends on unknown plugin {dependency}")
            }
            Self::DependencyCycle(plugin) => write!(f, "plugin dependency cycle includes {plugin}"),
            Self::DuplicateResourceNamespace(namespace) => {
                write!(f, "resource namespace has multiple owners: {namespace}")
            }
            Self::DuplicateServiceContribution { plugin, service } => write!(
                f,
                "plugin {plugin} contributes service {service} more than once"
            ),
            Self::ResourceOnlyService(plugin) => {
                write!(
                    f,
                    "resource-only plugin cannot provide executable services: {plugin}"
                )
            }
            Self::UnknownPlugin(plugin) => write!(f, "unknown plugin: {plugin}"),
            Self::NoEligibleProvider(service) => {
                write!(f, "no eligible provider for service {service}")
            }
            Self::BoundProviderUnavailable { service, plugin } => write!(
                f,
                "bound provider {plugin} is unavailable for service {service}"
            ),
            Self::DuplicateLayerPolicy { service, plugin } => write!(
                f,
                "layer policy lists plugin {plugin} more than once for service {service}"
            ),
            Self::RequiredLayerUnavailable { service, plugin } => write!(
                f,
                "required layer {plugin} is unavailable for service {service}"
            ),
            Self::ContinuationUnavailable => {
                f.write_str("service continuation is unavailable outside a layer")
            }
            Self::ContinuationAlreadyUsed(service) => {
                write!(f, "service continuation was already consumed for {service}")
            }
            Self::CausalServiceReentry(service) => {
                write!(f, "causal same-service re-entry is denied for {service}")
            }
            Self::ServiceDenied {
                plugin,
                service,
                message,
            } => write!(f, "plugin {plugin} denied service {service}: {message}"),
            Self::ServiceCancelled { plugin, service } => {
                write!(f, "plugin {plugin} cancelled service {service}")
            }
            Self::PluginNotActive(plugin) => write!(f, "plugin is not active: {plugin}"),
            Self::HostOperationDenied { plugin, operation } => {
                write!(
                    f,
                    "plugin {plugin} is not allowed to perform host operation {operation}"
                )
            }
            Self::Persistence { plugin, message } => {
                write!(f, "plugin {plugin} persistence operation failed: {message}")
            }
            Self::EmbeddedFactoryMissing(plugin) => {
                write!(f, "embedded plugin has no registered factory: {plugin}")
            }
            Self::WrongExecutionKind(plugin) => write!(
                f,
                "plugin execution kind does not match requested host: {plugin}"
            ),
            Self::ComponentGraph(error) => write!(f, "component graph resolution failed: {error}"),
            Self::RuntimeProviderUnavailable(runtime) => {
                write!(f, "runtime provider is unavailable: {runtime}")
            }
            Self::DuplicateRuntimeProvider {
                runtime,
                first,
                second,
            } => write!(
                f,
                "runtime {runtime} has multiple providers: {first} and {second}"
            ),
            Self::ReservedRuntimeProvider(plugin) => {
                write!(
                    f,
                    "plugin {plugin} cannot provide the Core-owned embedded runtime"
                )
            }
            Self::RuntimeProviderNotExecutable { runtime, provider } => write!(
                f,
                "runtime provider {provider} for {runtime} is not executable"
            ),
            Self::RuntimeProviderContractUnavailable { runtime, provider } => write!(
                f,
                "plugin {provider} does not expose the runtime-provider contract for {runtime}"
            ),
            Self::RuntimePrepare {
                plugin,
                runtime,
                message,
            } => write!(
                f,
                "runtime {runtime} failed to prepare plugin {plugin}: {message}"
            ),
            Self::PluginStart { plugin, message } => {
                write!(f, "plugin {plugin} failed to start: {message}")
            }
            Self::ListenerBinding {
                plugin,
                component,
                method,
                message,
            } => write!(
                f,
                "plugin {plugin} failed to bind listener {component}/{method}: {message}"
            ),
            Self::EventTopology(error) => write!(f, "event topology activation failed: {error}"),
            Self::ResolvedGenerationMissing => {
                f.write_str("listener topology requires an active resolved generation")
            }
            Self::PluginStop { plugin, message } => {
                write!(f, "plugin {plugin} failed to stop: {message}")
            }
            Self::PartiallyActiveRuntime => f.write_str(
                "development reconciliation requires a fully active or fully inactive runtime",
            ),
            Self::ServiceInvoke {
                plugin,
                service,
                message,
            } => write!(f, "plugin {plugin} failed service {service}: {message}"),
        }
    }
}

impl Error for KernelError {}

impl From<ComponentGraphError> for KernelError {
    fn from(error: ComponentGraphError) -> Self {
        Self::ComponentGraph(error)
    }
}

impl From<EventError> for KernelError {
    fn from(error: EventError) -> Self {
        Self::EventTopology(error)
    }
}

static NEXT_POLICY_IDENTITY: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct KernelPolicyIdentity(u64);

impl KernelPolicyIdentity {
    fn fresh() -> Self {
        Self(NEXT_POLICY_IDENTITY.fetch_add(1, Ordering::Relaxed))
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderBinding {
    pub service: ServiceId,
    pub plugin: PluginId,
    pub priority: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedServiceChain {
    pub policy_identity: KernelPolicyIdentity,
    pub service: ServiceId,
    pub layers: Vec<ProviderBinding>,
    pub terminal: ProviderBinding,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayerPolicy {
    pub plugin: PluginId,
    pub priority: i32,
    pub required: bool,
    pub enabled: bool,
}

#[derive(Clone, Debug)]
pub struct KernelConfig {
    manifests: BTreeMap<PluginId, PluginManifest>,
    activation_order: Vec<PluginId>,
    namespace_owners: BTreeMap<ResourceNamespace, PluginId>,
    layer_policies: BTreeMap<ServiceId, Vec<LayerPolicy>>,
    runtime_providers: BTreeMap<RuntimeId, PluginId>,
    runtime_bindings: BTreeMap<PluginId, RuntimeBinding>,
    policy_identity: KernelPolicyIdentity,
}

impl KernelConfig {
    pub fn new(manifests: impl IntoIterator<Item = PluginManifest>) -> Result<Self, KernelError> {
        let mut indexed = BTreeMap::new();
        for manifest in manifests {
            let id = manifest.id.clone();
            if indexed.insert(id.clone(), manifest).is_some() {
                return Err(KernelError::DuplicatePlugin(id));
            }
        }

        let namespace_owners = validate_namespaces(&indexed)?;
        validate_contributions(&indexed)?;
        let runtime_providers = resolve_runtime_providers(&indexed)?;
        let runtime_bindings = resolve_runtime_bindings(&indexed, &runtime_providers)?;
        let activation_order = dependency_order(&indexed, &runtime_bindings)?;

        Ok(Self {
            manifests: indexed,
            activation_order,
            namespace_owners,
            layer_policies: BTreeMap::new(),
            runtime_providers,
            runtime_bindings,
            policy_identity: KernelPolicyIdentity::fresh(),
        })
    }

    pub fn empty() -> Self {
        Self::new([]).expect("empty kernel configuration is valid")
    }

    pub fn manifest(&self, plugin: &PluginId) -> Option<&PluginManifest> {
        self.manifests.get(plugin)
    }

    pub fn manifests(&self) -> impl Iterator<Item = &PluginManifest> {
        self.manifests.values()
    }

    pub fn activation_order(&self) -> &[PluginId] {
        &self.activation_order
    }

    pub fn runtime_provider(&self, runtime: &RuntimeId) -> Option<&PluginId> {
        self.runtime_providers.get(runtime)
    }

    pub fn runtime_binding(&self, plugin: &PluginId) -> Option<&RuntimeBinding> {
        self.runtime_bindings.get(plugin)
    }

    pub fn runtime_bindings(&self) -> impl Iterator<Item = &RuntimeBinding> {
        self.runtime_bindings.values()
    }

    pub fn resource_owner(&self, namespace: &ResourceNamespace) -> Option<&PluginId> {
        self.namespace_owners.get(namespace)
    }

    pub fn with_layer_policy(
        mut self,
        service: ServiceId,
        layers: Vec<LayerPolicy>,
    ) -> Result<Self, KernelError> {
        let mut seen = BTreeSet::new();
        for layer in &layers {
            if !seen.insert(layer.plugin.clone()) {
                return Err(KernelError::DuplicateLayerPolicy {
                    service,
                    plugin: layer.plugin.clone(),
                });
            }
        }
        self.layer_policies.insert(service, layers);
        self.policy_identity = KernelPolicyIdentity::fresh();
        Ok(self)
    }

    pub fn policy_identity(&self) -> KernelPolicyIdentity {
        self.policy_identity
    }

    pub fn layer_policy(&self, service: &ServiceId) -> &[LayerPolicy] {
        self.layer_policies
            .get(service)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn resolve_chain(
        &self,
        service: &ServiceId,
        caller_authority: &Authority,
        binding: Option<&PluginId>,
    ) -> Result<ResolvedServiceChain, KernelError> {
        let mut terminals = Vec::new();

        for manifest in self.manifests.values() {
            for contribution in &manifest.services {
                if contribution.role != ServiceRole::Terminal
                    || &contribution.service != service
                    || !caller_authority.permits_all(&contribution.required_authority)
                {
                    continue;
                }
                if binding.is_none_or(|bound| bound == &manifest.id) {
                    terminals.push(ProviderBinding {
                        service: service.clone(),
                        plugin: manifest.id.clone(),
                        priority: contribution.priority,
                    });
                }
            }
        }

        let layers = self.resolve_layers(service, caller_authority)?;

        let order = |left: &ProviderBinding, right: &ProviderBinding| {
            right
                .priority
                .cmp(&left.priority)
                .then_with(|| left.plugin.cmp(&right.plugin))
        };
        terminals.sort_by(order);

        let terminal = if let Some(terminal) = terminals.into_iter().next() {
            terminal
        } else if let Some(plugin) = binding {
            return Err(KernelError::BoundProviderUnavailable {
                service: service.clone(),
                plugin: plugin.clone(),
            });
        } else {
            return Err(KernelError::NoEligibleProvider(service.clone()));
        };

        Ok(ResolvedServiceChain {
            policy_identity: self.policy_identity,
            service: service.clone(),
            layers,
            terminal,
        })
    }

    /// Resolve interposition around one exact graph-selected component endpoint.
    /// Terminal selection is already complete in the component graph; the service
    /// registry contributes only explicitly configured layers.
    pub fn resolve_component_chain(
        &self,
        service: &ServiceId,
        caller_authority: &Authority,
        terminal_plugin: &PluginId,
    ) -> Result<ResolvedServiceChain, KernelError> {
        let terminal_manifest = self
            .manifests
            .get(terminal_plugin)
            .ok_or_else(|| KernelError::UnknownPlugin(terminal_plugin.clone()))?;
        if matches!(terminal_manifest.execution, PluginExecution::ResourceOnly) {
            return Err(KernelError::WrongExecutionKind(terminal_plugin.clone()));
        }
        Ok(ResolvedServiceChain {
            policy_identity: self.policy_identity,
            service: service.clone(),
            layers: self.resolve_layers(service, caller_authority)?,
            terminal: ProviderBinding {
                service: service.clone(),
                plugin: terminal_plugin.clone(),
                priority: 0,
            },
        })
    }

    fn resolve_layers(
        &self,
        service: &ServiceId,
        caller_authority: &Authority,
    ) -> Result<Vec<ProviderBinding>, KernelError> {
        let mut layers = Vec::new();
        for policy in self.layer_policy(service) {
            if !policy.enabled {
                if policy.required {
                    return Err(KernelError::RequiredLayerUnavailable {
                        service: service.clone(),
                        plugin: policy.plugin.clone(),
                    });
                }
                continue;
            }
            let contribution = self.manifests.get(&policy.plugin).and_then(|manifest| {
                manifest.services.iter().find(|contribution| {
                    contribution.role == ServiceRole::Layer && &contribution.service == service
                })
            });
            match contribution {
                Some(contribution)
                    if caller_authority.permits_all(&contribution.required_authority) =>
                {
                    layers.push(ProviderBinding {
                        service: service.clone(),
                        plugin: policy.plugin.clone(),
                        priority: policy.priority,
                    });
                }
                _ if policy.required => {
                    return Err(KernelError::RequiredLayerUnavailable {
                        service: service.clone(),
                        plugin: policy.plugin.clone(),
                    });
                }
                _ => {}
            }
        }
        layers.sort_by(|left, right| {
            right
                .priority
                .cmp(&left.priority)
                .then_with(|| left.plugin.cmp(&right.plugin))
        });
        Ok(layers)
    }

    pub fn resolve_bound_chain(
        &self,
        service: &ServiceId,
        caller_authority: &Authority,
        binding: &PluginId,
    ) -> Result<ResolvedServiceChain, KernelError> {
        self.resolve_chain(service, caller_authority, Some(binding))
    }

    pub fn resolve(
        &self,
        service: &ServiceId,
        caller_authority: &Authority,
        binding: Option<&PluginId>,
    ) -> Result<ProviderBinding, KernelError> {
        Ok(self
            .resolve_chain(service, caller_authority, binding)?
            .terminal)
    }

    pub fn can_execute(&self, plugin: &PluginId) -> Result<bool, KernelError> {
        let manifest = self
            .manifests
            .get(plugin)
            .ok_or_else(|| KernelError::UnknownPlugin(plugin.clone()))?;
        Ok(!matches!(manifest.execution, PluginExecution::ResourceOnly))
    }
}

fn validate_namespaces(
    manifests: &BTreeMap<PluginId, PluginManifest>,
) -> Result<BTreeMap<ResourceNamespace, PluginId>, KernelError> {
    let mut owners = BTreeMap::new();
    for manifest in manifests.values() {
        for namespace in &manifest.resource_namespaces {
            if owners
                .insert(namespace.clone(), manifest.id.clone())
                .is_some()
            {
                return Err(KernelError::DuplicateResourceNamespace(namespace.clone()));
            }
        }
    }
    Ok(owners)
}

fn validate_contributions(
    manifests: &BTreeMap<PluginId, PluginManifest>,
) -> Result<(), KernelError> {
    for manifest in manifests.values() {
        if matches!(manifest.execution, PluginExecution::ResourceOnly)
            && !manifest.services.is_empty()
        {
            return Err(KernelError::ResourceOnlyService(manifest.id.clone()));
        }
        let mut seen = BTreeSet::new();
        for contribution in &manifest.services {
            if !seen.insert(contribution.service.clone()) {
                return Err(KernelError::DuplicateServiceContribution {
                    plugin: manifest.id.clone(),
                    service: contribution.service.clone(),
                });
            }
        }
    }
    Ok(())
}

fn resolve_runtime_providers(
    manifests: &BTreeMap<PluginId, PluginManifest>,
) -> Result<BTreeMap<RuntimeId, PluginId>, KernelError> {
    let mut providers = BTreeMap::new();
    for manifest in manifests.values() {
        for contribution in &manifest.services {
            let Some(runtime) = runtime_provider_runtime(&contribution.service) else {
                continue;
            };
            if runtime.as_str() == EMBEDDED_RUNTIME {
                return Err(KernelError::ReservedRuntimeProvider(manifest.id.clone()));
            }
            if contribution.role != ServiceRole::Terminal
                || matches!(manifest.execution, PluginExecution::ResourceOnly)
            {
                return Err(KernelError::RuntimeProviderNotExecutable {
                    runtime,
                    provider: manifest.id.clone(),
                });
            }
            if let Some(first) = providers.insert(runtime.clone(), manifest.id.clone()) {
                return Err(KernelError::DuplicateRuntimeProvider {
                    runtime,
                    first,
                    second: manifest.id.clone(),
                });
            }
        }
    }
    Ok(providers)
}

fn resolve_runtime_bindings(
    manifests: &BTreeMap<PluginId, PluginManifest>,
    providers: &BTreeMap<RuntimeId, PluginId>,
) -> Result<BTreeMap<PluginId, RuntimeBinding>, KernelError> {
    let mut bindings = BTreeMap::new();
    for manifest in manifests.values() {
        let PluginExecution::Runtime { runtime, artifact } = &manifest.execution else {
            continue;
        };
        if runtime.as_str() == EMBEDDED_RUNTIME {
            return Err(KernelError::RuntimeProviderUnavailable(runtime.clone()));
        }
        let provider = providers
            .get(runtime)
            .ok_or_else(|| KernelError::RuntimeProviderUnavailable(runtime.clone()))?
            .clone();
        bindings.insert(
            manifest.id.clone(),
            RuntimeBinding {
                guest: manifest.id.clone(),
                runtime: runtime.clone(),
                provider,
                artifact_revision: artifact.revision.clone(),
            },
        );
    }
    Ok(bindings)
}

fn dependency_order(
    manifests: &BTreeMap<PluginId, PluginManifest>,
    runtime_bindings: &BTreeMap<PluginId, RuntimeBinding>,
) -> Result<Vec<PluginId>, KernelError> {
    for manifest in manifests.values() {
        for dependency in &manifest.dependencies {
            if !manifests.contains_key(dependency) {
                return Err(KernelError::UnknownDependency {
                    plugin: manifest.id.clone(),
                    dependency: dependency.clone(),
                });
            }
        }
    }

    #[derive(Clone, Copy, Eq, PartialEq)]
    enum Visit {
        Visiting,
        Done,
    }

    fn visit(
        plugin: &PluginId,
        manifests: &BTreeMap<PluginId, PluginManifest>,
        runtime_bindings: &BTreeMap<PluginId, RuntimeBinding>,
        visits: &mut BTreeMap<PluginId, Visit>,
        order: &mut Vec<PluginId>,
    ) -> Result<(), KernelError> {
        match visits.get(plugin) {
            Some(Visit::Done) => return Ok(()),
            Some(Visit::Visiting) => return Err(KernelError::DependencyCycle(plugin.clone())),
            None => {}
        }
        visits.insert(plugin.clone(), Visit::Visiting);
        for dependency in &manifests[plugin].dependencies {
            visit(dependency, manifests, runtime_bindings, visits, order)?;
        }
        if let Some(binding) = runtime_bindings.get(plugin) {
            visit(
                &binding.provider,
                manifests,
                runtime_bindings,
                visits,
                order,
            )?;
        }
        visits.insert(plugin.clone(), Visit::Done);
        order.push(plugin.clone());
        Ok(())
    }

    let mut visits = BTreeMap::new();
    let mut order = Vec::new();
    for plugin in manifests.keys() {
        visit(plugin, manifests, runtime_bindings, &mut visits, &mut order)?;
    }
    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CapabilityId, ServiceContribution, ServiceRole};

    fn plugin(value: &str) -> PluginId {
        PluginId::parse(value).unwrap()
    }

    fn service(value: &str) -> ServiceId {
        ServiceId::parse(value).unwrap()
    }

    fn capability(value: &str) -> CapabilityId {
        CapabilityId::parse(value).unwrap()
    }

    fn manifest(id: &str, priority: i32, required: Authority) -> PluginManifest {
        PluginManifest {
            id: plugin(id),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: vec![ServiceContribution {
                role: crate::ServiceRole::Terminal,
                service: service("demo.service@1"),
                priority,
                required_authority: required,
            }],
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }

    #[test]
    fn provider_resolution_is_deterministic() {
        let config = KernelConfig::new([
            manifest("z-provider", 10, Authority::default()),
            manifest("a-provider", 10, Authority::default()),
            manifest("higher", 20, Authority::default()),
        ])
        .unwrap();

        let selected = config
            .resolve(&service("demo.service@1"), &Authority::default(), None)
            .unwrap();
        assert_eq!(selected.plugin, plugin("higher"));

        let equal_priority = KernelConfig::new([
            manifest("z-provider", 10, Authority::default()),
            manifest("a-provider", 10, Authority::default()),
        ])
        .unwrap();
        assert_eq!(
            equal_priority
                .resolve(&service("demo.service@1"), &Authority::default(), None)
                .unwrap()
                .plugin,
            plugin("a-provider")
        );
    }

    #[test]
    fn service_chain_orders_layers_before_one_terminal() {
        let mut lower = manifest("z-layer", 10, Authority::default());
        lower.services[0].role = ServiceRole::Layer;
        let mut equal = manifest("a-layer", 10, Authority::default());
        equal.services[0].role = ServiceRole::Layer;
        let mut higher = manifest("higher-layer", 20, Authority::default());
        higher.services[0].role = ServiceRole::Layer;
        let terminal = manifest("terminal", 1, Authority::default());
        let config = KernelConfig::new([lower, equal, higher, terminal])
            .unwrap()
            .with_layer_policy(
                service("demo.service@1"),
                vec![
                    LayerPolicy {
                        plugin: plugin("z-layer"),
                        priority: 10,
                        required: false,
                        enabled: true,
                    },
                    LayerPolicy {
                        plugin: plugin("a-layer"),
                        priority: 10,
                        required: false,
                        enabled: true,
                    },
                    LayerPolicy {
                        plugin: plugin("higher-layer"),
                        priority: 20,
                        required: false,
                        enabled: true,
                    },
                ],
            )
            .unwrap();

        let chain = config
            .resolve_chain(&service("demo.service@1"), &Authority::default(), None)
            .unwrap();
        assert_eq!(
            chain
                .layers
                .iter()
                .map(|binding| binding.plugin.clone())
                .collect::<Vec<_>>(),
            vec![plugin("higher-layer"), plugin("a-layer"), plugin("z-layer")]
        );
        assert_eq!(chain.terminal.plugin, plugin("terminal"));
        assert_eq!(
            config
                .resolve(&service("demo.service@1"), &Authority::default(), None)
                .unwrap()
                .plugin,
            plugin("terminal")
        );
    }

    #[test]
    fn explicit_terminal_binding_preserves_layers() {
        let mut layer = manifest("layer", 100, Authority::default());
        layer.services[0].role = ServiceRole::Layer;
        let preferred = manifest("preferred", 1, Authority::default());
        let alternate = manifest("alternate", 50, Authority::default());
        let config = KernelConfig::new([layer, preferred, alternate])
            .unwrap()
            .with_layer_policy(
                service("demo.service@1"),
                vec![LayerPolicy {
                    plugin: plugin("layer"),
                    priority: 100,
                    required: false,
                    enabled: true,
                }],
            )
            .unwrap();
        let chain = config
            .resolve_chain(
                &service("demo.service@1"),
                &Authority::default(),
                Some(&plugin("preferred")),
            )
            .unwrap();
        assert_eq!(chain.layers.len(), 1);
        assert_eq!(chain.layers[0].plugin, plugin("layer"));
        assert_eq!(chain.terminal.plugin, plugin("preferred"));
    }

    #[test]
    fn unauthorized_layers_are_excluded_before_chain_resolution() {
        let read = capability("fs.read");
        let write = capability("fs.write");
        let mut allowed = manifest("allowed-layer", 1, Authority::new([read.clone()]));
        allowed.services[0].role = ServiceRole::Layer;
        let mut forbidden = manifest("forbidden-layer", 100, Authority::new([write]));
        forbidden.services[0].role = ServiceRole::Layer;
        let terminal = manifest("terminal", 1, Authority::default());
        let config = KernelConfig::new([allowed, forbidden, terminal])
            .unwrap()
            .with_layer_policy(
                service("demo.service@1"),
                vec![
                    LayerPolicy {
                        plugin: plugin("forbidden-layer"),
                        priority: 100,
                        required: false,
                        enabled: true,
                    },
                    LayerPolicy {
                        plugin: plugin("allowed-layer"),
                        priority: 1,
                        required: false,
                        enabled: true,
                    },
                ],
            )
            .unwrap();
        let chain = config
            .resolve_chain(&service("demo.service@1"), &Authority::new([read]), None)
            .unwrap();
        assert_eq!(chain.layers.len(), 1);
        assert_eq!(chain.layers[0].plugin, plugin("allowed-layer"));
    }

    #[test]
    fn unconfigured_layer_is_not_self_enabled() {
        let mut layer = manifest("layer", 100, Authority::default());
        layer.services[0].role = ServiceRole::Layer;
        let terminal = manifest("terminal", 1, Authority::default());
        let config = KernelConfig::new([layer, terminal]).unwrap();
        let chain = config
            .resolve_chain(&service("demo.service@1"), &Authority::default(), None)
            .unwrap();
        assert!(chain.layers.is_empty());
    }

    #[test]
    fn optional_missing_layer_is_skipped_but_required_missing_layer_fails_closed() {
        let terminal = manifest("terminal", 1, Authority::default());
        let optional = KernelConfig::new([terminal.clone()])
            .unwrap()
            .with_layer_policy(
                service("demo.service@1"),
                vec![LayerPolicy {
                    plugin: plugin("missing"),
                    priority: 1,
                    required: false,
                    enabled: true,
                }],
            )
            .unwrap();
        assert!(optional
            .resolve_chain(&service("demo.service@1"), &Authority::default(), None)
            .unwrap()
            .layers
            .is_empty());

        let required = KernelConfig::new([terminal])
            .unwrap()
            .with_layer_policy(
                service("demo.service@1"),
                vec![LayerPolicy {
                    plugin: plugin("missing"),
                    priority: 1,
                    required: true,
                    enabled: true,
                }],
            )
            .unwrap();
        assert!(matches!(
            required.resolve_chain(&service("demo.service@1"), &Authority::default(), None),
            Err(KernelError::RequiredLayerUnavailable { .. })
        ));
    }

    #[test]
    fn required_unauthorized_or_disabled_layer_fails_closed() {
        let write = capability("fs.write");
        let mut layer = manifest("layer", 100, Authority::new([write]));
        layer.services[0].role = ServiceRole::Layer;
        let terminal = manifest("terminal", 1, Authority::default());
        let config = KernelConfig::new([layer, terminal])
            .unwrap()
            .with_layer_policy(
                service("demo.service@1"),
                vec![LayerPolicy {
                    plugin: plugin("layer"),
                    priority: 1,
                    required: true,
                    enabled: true,
                }],
            )
            .unwrap();
        assert!(matches!(
            config.resolve_chain(&service("demo.service@1"), &Authority::default(), None),
            Err(KernelError::RequiredLayerUnavailable { .. })
        ));
    }

    #[test]
    fn authority_filters_before_priority() {
        let read = capability("fs.read");
        let write = capability("fs.write");
        let config = KernelConfig::new([
            manifest("readable", 1, Authority::new([read.clone()])),
            manifest("forbidden", 100, Authority::new([write])),
        ])
        .unwrap();

        let selected = config
            .resolve(&service("demo.service@1"), &Authority::new([read]), None)
            .unwrap();
        assert_eq!(selected.plugin, plugin("readable"));
    }

    #[test]
    fn provider_grant_does_not_become_caller_requirement() {
        let read = capability("fs.read");
        let network = capability("network.read");
        let mut provider = manifest("provider", 1, Authority::new([read.clone()]));
        provider.maximum_authority = Authority::new([network]);
        let config = KernelConfig::new([provider]).unwrap();

        assert_eq!(
            config
                .resolve(&service("demo.service@1"), &Authority::new([read]), None)
                .unwrap()
                .plugin,
            plugin("provider")
        );
    }

    #[test]
    fn dependencies_are_order_independent_and_cycles_fail() {
        let mut a = PluginManifest::resource_only(plugin("a"));
        a.dependencies.push(plugin("b"));
        let b = PluginManifest::resource_only(plugin("b"));
        let config = KernelConfig::new([a.clone(), b]).unwrap();
        assert_eq!(config.activation_order(), &[plugin("b"), plugin("a")]);

        let mut b = PluginManifest::resource_only(plugin("b"));
        b.dependencies.push(plugin("a"));
        assert!(matches!(
            KernelConfig::new([a, b]),
            Err(KernelError::DependencyCycle(_))
        ));
    }

    #[test]
    fn resource_namespace_has_one_owner() {
        let namespace = ResourceNamespace::parse("demo.data").unwrap();
        let mut a = PluginManifest::resource_only(plugin("a"));
        a.resource_namespaces.push(namespace.clone());
        let mut b = PluginManifest::resource_only(plugin("b"));
        b.resource_namespaces.push(namespace.clone());

        assert_eq!(
            KernelConfig::new([a, b]).unwrap_err(),
            KernelError::DuplicateResourceNamespace(namespace)
        );
    }

    #[test]
    fn resource_only_plugins_cannot_execute_or_contribute_services() {
        let id = plugin("resources");
        let config = KernelConfig::new([PluginManifest::resource_only(id.clone())]).unwrap();
        assert!(!config.can_execute(&id).unwrap());

        let mut invalid = PluginManifest::resource_only(id.clone());
        invalid.services.push(ServiceContribution {
            role: crate::ServiceRole::Terminal,
            service: service("demo.service@1"),
            priority: 0,
            required_authority: Authority::default(),
        });
        assert_eq!(
            KernelConfig::new([invalid]).unwrap_err(),
            KernelError::ResourceOnlyService(id)
        );
    }
}
