use crate::{
    Authority, ComponentExport, ComponentId, ComponentListener, ComponentManifest, EventBus,
    EventEnvelope, EventError, EventHandler, EventSubscription, InterfaceCompatibility,
    InterfaceId, InterfaceSchemaMismatch, PluginExecution, PluginId, PluginManifest,
    ProviderCompositionPolicy, ProviderSelectionReason, SubscriptionSpec,
};
use std::sync::Arc;
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt::{self, Display, Formatter},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ComponentGraphError {
    DuplicatePlugin(PluginId),
    DuplicateComponent(ComponentId),
    UnknownOwningPlugin {
        component: ComponentId,
        plugin: PluginId,
    },
    ResourceOnlyComponentOwner {
        component: ComponentId,
        plugin: PluginId,
    },
    DuplicateImport {
        component: ComponentId,
        interface: InterfaceId,
    },
    DuplicateExport {
        component: ComponentId,
        interface: InterfaceId,
    },
    MissingRequiredImport {
        component: ComponentId,
        interface: InterfaceId,
    },
    IncompatibleRequiredImport {
        component: ComponentId,
        interface: InterfaceId,
        exporter: ComponentId,
        mismatch: Box<InterfaceSchemaMismatch>,
    },
    ImportNotDeclared {
        component: ComponentId,
        interface: InterfaceId,
    },
    RequiredImportCycle {
        path: Vec<ComponentId>,
    },
    ListenerTopology(EventError),
    UnknownComponent(ComponentId),
}

impl Display for ComponentGraphError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicatePlugin(plugin) => write!(f, "duplicate plugin trust owner: {plugin}"),
            Self::DuplicateComponent(component) => write!(f, "duplicate component: {component}"),
            Self::UnknownOwningPlugin { component, plugin } => {
                write!(
                    f,
                    "component {component} has unknown owning plugin {plugin}"
                )
            }
            Self::ResourceOnlyComponentOwner { component, plugin } => write!(
                f,
                "executable component {component} cannot belong to resource-only plugin {plugin}"
            ),
            Self::DuplicateImport {
                component,
                interface,
            } => write!(
                f,
                "component {component} imports interface {interface} more than once"
            ),
            Self::DuplicateExport {
                component,
                interface,
            } => write!(
                f,
                "component {component} exports interface {interface} more than once"
            ),
            Self::MissingRequiredImport {
                component,
                interface,
            } => write!(
                f,
                "component {component} has unresolved required import {interface}"
            ),
            Self::IncompatibleRequiredImport {
                component,
                interface,
                exporter,
                mismatch,
            } => write!(
                f,
                "component {component} import {interface} is structurally incompatible with exporter {exporter}: {mismatch}"
            ),
            Self::ImportNotDeclared {
                component,
                interface,
            } => write!(
                f,
                "component {component} has no capability for undeclared import {interface}"
            ),
            Self::RequiredImportCycle { path } => {
                let rendered = path
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" -> ");
                write!(f, "required component import cycle: {rendered}")
            }
            Self::ListenerTopology(error) => write!(f, "invalid listener topology: {error}"),
            Self::UnknownComponent(component) => write!(f, "unknown component: {component}"),
        }
    }
}

impl Error for ComponentGraphError {}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedImportHandle {
    importer: ComponentId,
    interface: InterfaceId,
    exporter: ComponentId,
    owning_plugin: PluginId,
    execution: PluginExecution,
    effective_authority: Authority,
}

impl ResolvedImportHandle {
    pub fn importer(&self) -> &ComponentId {
        &self.importer
    }

    pub fn interface(&self) -> &InterfaceId {
        &self.interface
    }

    pub fn exporter(&self) -> &ComponentId {
        &self.exporter
    }

    pub fn owning_plugin(&self) -> &PluginId {
        &self.owning_plugin
    }

    pub fn execution(&self) -> &PluginExecution {
        &self.execution
    }

    pub fn effective_authority(&self) -> &Authority {
        &self.effective_authority
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedProviderPlan {
    primary: ResolvedImportHandle,
    fallbacks: Vec<ResolvedImportHandle>,
    selection_reason: ProviderSelectionReason,
}

impl ResolvedProviderPlan {
    pub fn primary(&self) -> &ResolvedImportHandle {
        &self.primary
    }

    pub fn fallbacks(&self) -> &[ResolvedImportHandle] {
        &self.fallbacks
    }

    pub fn selection_reason(&self) -> ProviderSelectionReason {
        self.selection_reason
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedImport {
    pub interface: InterfaceId,
    pub required: bool,
    pub binding: Option<ResolvedImportHandle>,
    pub fallbacks: Vec<ResolvedImportHandle>,
    pub selection_reason: Option<ProviderSelectionReason>,
}

impl ResolvedImport {
    fn provider_plan(&self) -> Option<ResolvedProviderPlan> {
        self.binding.as_ref().map(|primary| ResolvedProviderPlan {
            primary: primary.clone(),
            fallbacks: self.fallbacks.clone(),
            selection_reason: self
                .selection_reason
                .expect("resolved provider binding has a selection reason"),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedComponent {
    pub id: ComponentId,
    pub owning_plugin: PluginId,
    pub execution: PluginExecution,
    pub imports: Vec<ResolvedImport>,
    pub maximum_authority: Authority,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedListener {
    pub component: ComponentId,
    pub owning_plugin: PluginId,
    pub declaration: ComponentListener,
    pub maximum_authority: Authority,
}

impl ResolvedListener {
    pub fn subscription_spec(&self, kernel_policy_revision: u64) -> SubscriptionSpec {
        SubscriptionSpec {
            id: self.declaration.id.clone(),
            owner: self.owning_plugin.clone(),
            event_type: self.declaration.event.clone(),
            event_version: self.declaration.event_version,
            dependencies: self.declaration.dependencies.clone(),
            failure_policy: self.declaration.failure_policy,
            required_authority: self.declaration.required_authority.clone(),
            maximum_authority: self.maximum_authority.clone(),
            kernel_policy_revision,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedComponentGraph {
    components: BTreeMap<ComponentId, ResolvedComponent>,
    listeners: Vec<ResolvedListener>,
}

struct EligibleProvider<'a> {
    component: &'a ComponentManifest,
    effective_authority: Authority,
    priority: i32,
    explicit: bool,
}

impl ResolvedComponentGraph {
    pub fn empty() -> Self {
        Self {
            components: BTreeMap::new(),
            listeners: Vec::new(),
        }
    }

    pub fn compile(
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        harness_authority: &Authority,
    ) -> Result<Self, ComponentGraphError> {
        Self::compile_inner(
            plugin_manifests,
            component_manifests,
            harness_authority,
            None,
        )
    }

    /// Resolve component imports with product-owned provider policy while using
    /// the same structural, authority, and topology resolver as the baseline path.
    pub fn compile_with_provider_policy(
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        harness_authority: &Authority,
        policy: &ProviderCompositionPolicy,
    ) -> Result<Self, ComponentGraphError> {
        Self::compile_inner(
            plugin_manifests,
            component_manifests,
            harness_authority,
            Some(policy),
        )
    }

    fn compile_inner(
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        harness_authority: &Authority,
        provider_policy: Option<&ProviderCompositionPolicy>,
    ) -> Result<Self, ComponentGraphError> {
        let mut plugins = BTreeMap::new();
        for manifest in plugin_manifests {
            let id = manifest.id.clone();
            if plugins.insert(id.clone(), manifest).is_some() {
                return Err(ComponentGraphError::DuplicatePlugin(id));
            }
        }

        let mut components = BTreeMap::new();
        for manifest in component_manifests {
            validate_component_manifest(&manifest)?;
            let owner = plugins.get(&manifest.owner).ok_or_else(|| {
                ComponentGraphError::UnknownOwningPlugin {
                    component: manifest.id.clone(),
                    plugin: manifest.owner.clone(),
                }
            })?;
            if matches!(owner.execution, PluginExecution::ResourceOnly) {
                return Err(ComponentGraphError::ResourceOnlyComponentOwner {
                    component: manifest.id.clone(),
                    plugin: manifest.owner.clone(),
                });
            }
            let id = manifest.id.clone();
            if components.insert(id.clone(), manifest).is_some() {
                return Err(ComponentGraphError::DuplicateComponent(id));
            }
        }

        let mut exporters: BTreeMap<InterfaceId, Vec<(&ComponentManifest, &ComponentExport)>> =
            BTreeMap::new();
        for manifest in components.values() {
            for export in &manifest.exports {
                exporters
                    .entry(export.interface.clone())
                    .or_default()
                    .push((manifest, export));
            }
        }
        for candidates in exporters.values_mut() {
            candidates.sort_by(|(left, _), (right, _)| left.id.cmp(&right.id));
        }

        let mut resolved = BTreeMap::new();
        let mut listeners = Vec::new();
        for manifest in components.values() {
            let owner = &plugins[&manifest.owner];
            let component_authority = harness_authority
                .attenuate(&owner.maximum_authority)
                .attenuate(&manifest.maximum_authority);
            let mut imports = Vec::with_capacity(manifest.imports.len());
            for import in &manifest.imports {
                let explicit =
                    provider_policy.and_then(|policy| policy.explicit_binding(&import.interface));
                let mut eligible = Vec::new();
                let mut incompatible = None;
                if let Some(candidates) = exporters.get(&import.interface) {
                    for (candidate, export) in candidates {
                        if provider_policy.is_some_and(|policy| {
                            !policy.provider_enabled(&import.interface, &candidate.id)
                        }) {
                            continue;
                        }
                        let exporter_owner = &plugins[&candidate.owner];
                        let effective_authority = component_authority
                            .attenuate(&import.authority)
                            .attenuate(&exporter_owner.maximum_authority)
                            .attenuate(&candidate.maximum_authority);
                        if !effective_authority.permits_all(&export.required_authority) {
                            continue;
                        }
                        match import.schema.accepts_provider(&export.schema) {
                            InterfaceCompatibility::Exact | InterfaceCompatibility::Compatible => {
                                let priority = provider_policy.map_or(export.priority, |policy| {
                                    policy.effective_priority(&import.interface, &candidate.id)
                                });
                                eligible.push(EligibleProvider {
                                    component: candidate,
                                    effective_authority,
                                    priority,
                                    explicit: explicit.is_some_and(|id| id == &candidate.id),
                                });
                            }
                            InterfaceCompatibility::Incompatible(mismatch) => {
                                incompatible
                                    .get_or_insert_with(|| (candidate.id.clone(), mismatch));
                            }
                        }
                    }
                }

                eligible.sort_by(|left, right| {
                    right
                        .explicit
                        .cmp(&left.explicit)
                        .then_with(|| right.priority.cmp(&left.priority))
                        .then_with(|| left.component.id.cmp(&right.component.id))
                });

                let selection_reason = eligible.first().map(|primary| {
                    if primary.explicit {
                        ProviderSelectionReason::ExplicitBinding
                    } else {
                        let priority_changed_order = eligible
                            .get(1)
                            .is_some_and(|next| next.priority != primary.priority);
                        let policy_has_priority = provider_policy.is_some_and(|policy| {
                            eligible.iter().any(|candidate| {
                                policy.has_priority(&import.interface, &candidate.component.id)
                            })
                        });
                        if priority_changed_order || policy_has_priority {
                            ProviderSelectionReason::Priority
                        } else {
                            ProviderSelectionReason::StableIdentity
                        }
                    }
                });

                let mut handles = eligible
                    .into_iter()
                    .map(|candidate| {
                        let exporter_owner = &plugins[&candidate.component.owner];
                        ResolvedImportHandle {
                            importer: manifest.id.clone(),
                            interface: import.interface.clone(),
                            exporter: candidate.component.id.clone(),
                            owning_plugin: candidate.component.owner.clone(),
                            execution: exporter_owner.execution.clone(),
                            effective_authority: candidate.effective_authority,
                        }
                    })
                    .collect::<Vec<_>>();

                let binding = if handles.is_empty() {
                    if import.required {
                        if let Some((exporter, mismatch)) = incompatible {
                            return Err(ComponentGraphError::IncompatibleRequiredImport {
                                component: manifest.id.clone(),
                                interface: import.interface.clone(),
                                exporter,
                                mismatch: Box::new(mismatch),
                            });
                        }
                        return Err(ComponentGraphError::MissingRequiredImport {
                            component: manifest.id.clone(),
                            interface: import.interface.clone(),
                        });
                    }
                    None
                } else {
                    Some(handles.remove(0))
                };
                let fallbacks = if provider_policy
                    .is_some_and(|policy| policy.fallback_enabled(&import.interface))
                {
                    handles
                } else {
                    Vec::new()
                };
                imports.push(ResolvedImport {
                    interface: import.interface.clone(),
                    required: import.required,
                    binding,
                    fallbacks,
                    selection_reason,
                });
            }
            resolved.insert(
                manifest.id.clone(),
                ResolvedComponent {
                    id: manifest.id.clone(),
                    owning_plugin: manifest.owner.clone(),
                    execution: owner.execution.clone(),
                    imports,
                    maximum_authority: component_authority.clone(),
                },
            );
            listeners.extend(manifest.listeners.iter().cloned().map(|declaration| {
                ResolvedListener {
                    component: manifest.id.clone(),
                    owning_plugin: manifest.owner.clone(),
                    declaration,
                    maximum_authority: component_authority.clone(),
                }
            }));
        }

        validate_required_import_dag(&resolved)?;
        let noop: Arc<dyn EventHandler> =
            Arc::new(|_: &EventEnvelope, _: &Authority| -> Result<(), String> { Ok(()) });
        EventBus::validate_subscriptions(listeners.iter().map(|listener| EventSubscription {
            spec: listener.subscription_spec(0),
            handler: Arc::clone(&noop),
        }))
        .map_err(ComponentGraphError::ListenerTopology)?;

        Ok(Self {
            components: resolved,
            listeners,
        })
    }

    pub fn components(&self) -> impl Iterator<Item = &ResolvedComponent> {
        self.components.values()
    }

    pub fn component(&self, component: &ComponentId) -> Option<&ResolvedComponent> {
        self.components.get(component)
    }

    pub fn listeners(&self) -> impl Iterator<Item = &ResolvedListener> {
        self.listeners.iter()
    }

    fn resolved_import(
        &self,
        component: &ComponentId,
        interface: &InterfaceId,
    ) -> Result<&ResolvedImport, ComponentGraphError> {
        let resolved = self
            .components
            .get(component)
            .ok_or_else(|| ComponentGraphError::UnknownComponent(component.clone()))?;
        resolved
            .imports
            .iter()
            .find(|import| &import.interface == interface)
            .ok_or_else(|| ComponentGraphError::ImportNotDeclared {
                component: component.clone(),
                interface: interface.clone(),
            })
    }

    pub fn import_handle(
        &self,
        component: &ComponentId,
        interface: &InterfaceId,
    ) -> Result<Option<&ResolvedImportHandle>, ComponentGraphError> {
        Ok(self.resolved_import(component, interface)?.binding.as_ref())
    }

    pub fn provider_plan(
        &self,
        component: &ComponentId,
        interface: &InterfaceId,
    ) -> Result<Option<ResolvedProviderPlan>, ComponentGraphError> {
        Ok(self.resolved_import(component, interface)?.provider_plan())
    }
}

fn validate_required_import_dag(
    components: &BTreeMap<ComponentId, ResolvedComponent>,
) -> Result<(), ComponentGraphError> {
    fn visit(
        component: &ComponentId,
        components: &BTreeMap<ComponentId, ResolvedComponent>,
        visiting: &mut Vec<ComponentId>,
        visited: &mut BTreeSet<ComponentId>,
    ) -> Result<(), ComponentGraphError> {
        if let Some(start) = visiting.iter().position(|candidate| candidate == component) {
            let mut path = visiting[start..].to_vec();
            path.push(component.clone());
            return Err(ComponentGraphError::RequiredImportCycle { path });
        }
        if visited.contains(component) {
            return Ok(());
        }

        visiting.push(component.clone());
        if let Some(resolved) = components.get(component) {
            for import in &resolved.imports {
                if !import.required {
                    continue;
                }
                if let Some(binding) = &import.binding {
                    visit(binding.exporter(), components, visiting, visited)?;
                }
                for fallback in &import.fallbacks {
                    visit(fallback.exporter(), components, visiting, visited)?;
                }
            }
        }
        visiting.pop();
        visited.insert(component.clone());
        Ok(())
    }

    let mut visiting = Vec::new();
    let mut visited = BTreeSet::new();
    for component in components.keys() {
        visit(component, components, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn validate_component_manifest(manifest: &ComponentManifest) -> Result<(), ComponentGraphError> {
    let mut imports = BTreeSet::new();
    for import in &manifest.imports {
        if !imports.insert(import.interface.clone()) {
            return Err(ComponentGraphError::DuplicateImport {
                component: manifest.id.clone(),
                interface: import.interface.clone(),
            });
        }
    }
    let mut exports = BTreeSet::new();
    for export in &manifest.exports {
        if !exports.insert(export.interface.clone()) {
            return Err(ComponentGraphError::DuplicateExport {
                component: manifest.id.clone(),
                interface: export.interface.clone(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CapabilityId, ComponentExport, ComponentImport};

    fn component(value: &str) -> ComponentId {
        ComponentId::parse(value).unwrap()
    }

    fn interface(value: &str) -> InterfaceId {
        InterfaceId::parse(value).unwrap()
    }

    fn plugin(value: &str) -> PluginId {
        PluginId::parse(value).unwrap()
    }

    fn capability(value: &str) -> CapabilityId {
        CapabilityId::parse(value).unwrap()
    }

    fn plugin_manifest(id: &str, authority: Authority) -> PluginManifest {
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

    fn exporter(id: &str, priority: i32, authority: Authority) -> ComponentManifest {
        ComponentManifest {
            listeners: Vec::new(),
            id: component(id),
            owner: plugin(&format!("plugin-{id}")),
            imports: Vec::new(),
            exports: vec![ComponentExport {
                interface: interface("phenix.demo@1"),
                schema: Default::default(),
                priority,
                required_authority: Authority::default(),
            }],
            maximum_authority: authority,
        }
    }

    fn importer(required: bool, authority: Authority) -> ComponentManifest {
        ComponentManifest {
            listeners: Vec::new(),
            id: component("consumer"),
            owner: plugin("plugin-consumer"),
            imports: vec![ComponentImport {
                interface: interface("phenix.demo@1"),
                schema: Default::default(),
                required,
                authority: authority.clone(),
            }],
            exports: Vec::new(),
            maximum_authority: authority,
        }
    }

    fn owners(authority: Authority, exporters: &[&str]) -> Vec<PluginManifest> {
        let mut plugins = vec![plugin_manifest("plugin-consumer", authority.clone())];
        plugins.extend(
            exporters
                .iter()
                .map(|id| plugin_manifest(&format!("plugin-{id}"), authority.clone())),
        );
        plugins
    }

    #[test]
    fn required_import_must_resolve_before_component_use() {
        let error = ResolvedComponentGraph::compile(
            owners(Authority::default(), &[]),
            [importer(true, Authority::default())],
            &Authority::default(),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            ComponentGraphError::MissingRequiredImport { .. }
        ));
    }

    #[test]
    fn provider_selection_is_deterministic_and_registration_order_independent() {
        let authority = Authority::new([capability("demo.use")]);
        let first = ResolvedComponentGraph::compile(
            owners(authority.clone(), &["z-provider", "a-provider"]),
            [
                exporter("z-provider", 10, authority.clone()),
                importer(true, authority.clone()),
                exporter("a-provider", 10, authority.clone()),
            ],
            &authority,
        )
        .unwrap();
        let second = ResolvedComponentGraph::compile(
            owners(authority.clone(), &["a-provider", "z-provider"]),
            [
                exporter("a-provider", 10, authority.clone()),
                exporter("z-provider", 10, authority.clone()),
                importer(true, authority.clone()),
            ],
            &authority,
        )
        .unwrap();
        let selected = |graph: &ResolvedComponentGraph| {
            graph
                .import_handle(&component("consumer"), &interface("phenix.demo@1"))
                .unwrap()
                .unwrap()
                .exporter()
                .clone()
        };
        assert_eq!(selected(&first), component("a-provider"));
        assert_eq!(selected(&first), selected(&second));
    }

    #[test]
    fn product_policy_owns_effective_priority_and_fallback_plan() {
        let authority = Authority::default();
        let demo = interface("phenix.demo@1");
        let components = [
            exporter("a-provider", -100, authority.clone()),
            exporter("z-provider", 100, authority.clone()),
            importer(true, authority.clone()),
        ];
        let plugins = owners(authority.clone(), &["a-provider", "z-provider"]);

        let default = ResolvedComponentGraph::compile_with_provider_policy(
            plugins.clone(),
            components.clone(),
            &authority,
            &ProviderCompositionPolicy::default(),
        )
        .unwrap();
        assert_eq!(
            default
                .import_handle(&component("consumer"), &demo)
                .unwrap()
                .unwrap()
                .exporter(),
            &component("a-provider")
        );
        assert_eq!(
            default
                .provider_plan(&component("consumer"), &demo)
                .unwrap()
                .unwrap()
                .selection_reason(),
            ProviderSelectionReason::StableIdentity
        );

        let policy = ProviderCompositionPolicy::new()
            .with_priority(demo.clone(), component("z-provider"), 10)
            .with_interface_fallback(demo.clone())
            .with_fallback_enabled(demo.clone());
        let preferred = ResolvedComponentGraph::compile_with_provider_policy(
            plugins, components, &authority, &policy,
        )
        .unwrap();
        let plan = preferred
            .provider_plan(&component("consumer"), &demo)
            .unwrap()
            .unwrap();
        assert_eq!(plan.primary().exporter(), &component("z-provider"));
        assert_eq!(plan.fallbacks().len(), 1);
        assert_eq!(plan.fallbacks()[0].exporter(), &component("a-provider"));
        assert_eq!(plan.selection_reason(), ProviderSelectionReason::Priority);
    }

    #[test]
    fn explicit_binding_only_wins_when_eligible() {
        let read = capability("fs.read");
        let network = capability("network.read");
        let caller = Authority::new([read.clone()]);
        let broad = Authority::new([read.clone(), network.clone()]);
        let mut authorized = exporter("a-authorized", 0, broad.clone());
        authorized.exports[0].required_authority = Authority::new([read]);
        let mut unauthorized = exporter("z-unauthorized", 0, broad.clone());
        unauthorized.exports[0].required_authority = Authority::new([network]);
        let policy = ProviderCompositionPolicy::new()
            .with_explicit_binding(interface("phenix.demo@1"), component("z-unauthorized"));
        let graph = ResolvedComponentGraph::compile_with_provider_policy(
            vec![
                plugin_manifest("plugin-consumer", caller.clone()),
                plugin_manifest("plugin-a-authorized", broad.clone()),
                plugin_manifest("plugin-z-unauthorized", broad),
            ],
            [authorized, unauthorized, importer(true, caller.clone())],
            &caller,
            &policy,
        )
        .unwrap();

        assert_eq!(
            graph
                .import_handle(&component("consumer"), &interface("phenix.demo@1"))
                .unwrap()
                .unwrap()
                .exporter(),
            &component("a-authorized")
        );
    }

    #[test]
    fn provider_selection_skips_exports_whose_required_authority_cannot_be_granted() {
        let read = capability("fs.read");
        let network = capability("network.read");
        let importer_authority = Authority::new([read.clone()]);
        let provider_authority = Authority::new([read.clone(), network.clone()]);
        let mut high = exporter("high-provider", 20, provider_authority.clone());
        high.exports[0].required_authority = Authority::new([network]);
        let mut low = exporter("low-provider", 10, provider_authority.clone());
        low.exports[0].required_authority = Authority::new([read.clone()]);
        let graph = ResolvedComponentGraph::compile(
            vec![
                plugin_manifest("plugin-consumer", importer_authority.clone()),
                plugin_manifest("plugin-high-provider", provider_authority.clone()),
                plugin_manifest("plugin-low-provider", provider_authority),
            ],
            [high, low, importer(true, importer_authority.clone())],
            &importer_authority,
        )
        .unwrap();

        let handle = graph
            .import_handle(&component("consumer"), &interface("phenix.demo@1"))
            .unwrap()
            .unwrap();
        assert_eq!(handle.exporter(), &component("low-provider"));
        assert!(handle.effective_authority().permits(&read));
    }

    #[test]
    fn required_import_with_only_authority_incompatible_exports_fails_before_activation() {
        let read = capability("fs.read");
        let network = capability("network.read");
        let importer_authority = Authority::new([read]);
        let provider_authority = Authority::new([network.clone()]);
        let mut provider = exporter("provider", 10, provider_authority.clone());
        provider.exports[0].required_authority = provider_authority.clone();
        let graph = ResolvedComponentGraph::compile(
            vec![
                plugin_manifest("plugin-consumer", importer_authority.clone()),
                plugin_manifest("plugin-provider", provider_authority),
            ],
            [provider, importer(true, importer_authority.clone())],
            &Authority::new([capability("fs.read"), network]),
        );

        assert!(matches!(
            graph,
            Err(ComponentGraphError::MissingRequiredImport {
                component: missing_component,
                interface: missing_interface,
            }) if missing_component == component("consumer")
                && missing_interface == interface("phenix.demo@1")
        ));
    }

    #[test]
    fn binding_keeps_granted_authority_beyond_the_export_minimum() {
        let read = capability("fs.read");
        let authority = Authority::new([read.clone()]);
        let graph = ResolvedComponentGraph::compile(
            owners(authority.clone(), &["provider"]),
            [
                exporter("provider", 10, authority.clone()),
                importer(true, authority.clone()),
            ],
            &authority,
        )
        .unwrap();

        let handle = graph
            .import_handle(&component("consumer"), &interface("phenix.demo@1"))
            .unwrap()
            .unwrap();
        assert!(handle.effective_authority().permits(&read));
    }

    #[test]
    fn import_handle_authority_is_attenuated_at_plugin_and_component_boundaries() {
        let read = capability("fs.read");
        let write = capability("fs.write");
        let network = capability("network.read");
        let plugins = vec![
            plugin_manifest(
                "plugin-consumer",
                Authority::new([read.clone(), write.clone()]),
            ),
            plugin_manifest(
                "plugin-provider",
                Authority::new([read.clone(), network.clone()]),
            ),
        ];
        let graph = ResolvedComponentGraph::compile(
            plugins,
            [
                exporter("provider", 1, Authority::new([read.clone()])),
                importer(true, Authority::new([read.clone(), write.clone()])),
            ],
            &Authority::new([read.clone(), write, network]),
        )
        .unwrap();
        let handle = graph
            .import_handle(&component("consumer"), &interface("phenix.demo@1"))
            .unwrap()
            .unwrap();
        assert!(handle.effective_authority().permits(&read));
        assert!(!handle
            .effective_authority()
            .permits(&capability("fs.write")));
        assert!(!handle
            .effective_authority()
            .permits(&capability("network.read")));
    }

    #[test]
    fn optional_and_undeclared_imports_are_distinct() {
        let optional = ResolvedComponentGraph::compile(
            owners(Authority::default(), &[]),
            [importer(false, Authority::default())],
            &Authority::default(),
        )
        .unwrap();
        assert!(optional
            .import_handle(&component("consumer"), &interface("phenix.demo@1"))
            .unwrap()
            .is_none());
        assert!(matches!(
            optional.import_handle(&component("consumer"), &interface("other@1")),
            Err(ComponentGraphError::ImportNotDeclared { .. })
        ));
    }

    #[test]
    fn required_import_cycles_report_the_concrete_component_path() {
        let authority = Authority::default();
        let interface_a = interface("phenix.a@1");
        let interface_b = interface("phenix.b@1");
        let component_a = ComponentManifest {
            listeners: Vec::new(),
            id: component("component-a"),
            owner: plugin("plugin-a"),
            imports: vec![ComponentImport {
                interface: interface_b.clone(),
                schema: Default::default(),
                required: true,
                authority: authority.clone(),
            }],
            exports: vec![ComponentExport {
                interface: interface_a.clone(),
                schema: Default::default(),
                priority: 0,
                required_authority: authority.clone(),
            }],
            maximum_authority: authority.clone(),
        };
        let component_b = ComponentManifest {
            listeners: Vec::new(),
            id: component("component-b"),
            owner: plugin("plugin-b"),
            imports: vec![ComponentImport {
                interface: interface_a,
                schema: Default::default(),
                required: true,
                authority: authority.clone(),
            }],
            exports: vec![ComponentExport {
                interface: interface_b,
                schema: Default::default(),
                priority: 0,
                required_authority: authority.clone(),
            }],
            maximum_authority: authority.clone(),
        };

        let error = ResolvedComponentGraph::compile(
            [
                plugin_manifest("plugin-a", authority.clone()),
                plugin_manifest("plugin-b", authority.clone()),
            ],
            [component_a, component_b],
            &authority,
        )
        .unwrap_err();

        assert_eq!(
            error,
            ComponentGraphError::RequiredImportCycle {
                path: vec![
                    component("component-a"),
                    component("component-b"),
                    component("component-a"),
                ],
            }
        );
        assert_eq!(
            error.to_string(),
            "required component import cycle: component-a -> component-b -> component-a"
        );
    }

    #[test]
    fn component_owner_is_a_real_plugin_trust_boundary() {
        let component = ComponentManifest {
            listeners: Vec::new(),
            id: component("orphan"),
            owner: plugin("missing-plugin"),
            imports: Vec::new(),
            exports: Vec::new(),
            maximum_authority: Authority::default(),
        };
        assert!(matches!(
            ResolvedComponentGraph::compile([], [component], &Authority::default()),
            Err(ComponentGraphError::UnknownOwningPlugin { .. })
        ));
    }
}

#[cfg(test)]
mod interface_schema_binding_tests {
    use super::*;
    use crate::{InterfaceSchema, Key, PhenixSchema};

    fn key(value: &str) -> Key {
        Key::parse(value).unwrap()
    }

    fn table(fields: &[(&str, PhenixSchema)]) -> PhenixSchema {
        PhenixSchema::Table(
            fields
                .iter()
                .map(|(name, schema)| (key(name), schema.clone()))
                .collect(),
        )
    }

    fn plugin(id: &str) -> PluginManifest {
        PluginManifest {
            id: PluginId::parse(id).unwrap(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }

    fn interface() -> InterfaceId {
        InterfaceId::parse("fixture.schema@1").unwrap()
    }

    fn consumer(schema: InterfaceSchema) -> ComponentManifest {
        ComponentManifest {
            listeners: Vec::new(),
            id: ComponentId::parse("consumer").unwrap(),
            owner: PluginId::parse("consumer-owner").unwrap(),
            imports: vec![crate::ComponentImport {
                interface: interface(),
                schema,
                required: true,
                authority: Authority::default(),
            }],
            exports: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }

    fn provider(id: &str, priority: i32, schema: InterfaceSchema) -> ComponentManifest {
        ComponentManifest {
            listeners: Vec::new(),
            id: ComponentId::parse(id).unwrap(),
            owner: PluginId::parse(format!("{id}-owner")).unwrap(),
            imports: Vec::new(),
            exports: vec![crate::ComponentExport {
                interface: interface(),
                schema,
                priority,
                required_authority: Authority::default(),
            }],
            maximum_authority: Authority::default(),
        }
    }

    #[test]
    fn graph_accepts_directionally_compatible_independent_schemas() {
        let consumer_schema = InterfaceSchema::new(
            table(&[
                ("name", PhenixSchema::String),
                ("detail", PhenixSchema::U64),
            ]),
            table(&[("value", PhenixSchema::String)]),
        );
        let provider_schema = InterfaceSchema::new(
            table(&[("name", PhenixSchema::String)]),
            table(&[
                ("value", PhenixSchema::String),
                ("internal", PhenixSchema::U64),
            ]),
        );
        let graph = ResolvedComponentGraph::compile(
            [plugin("consumer-owner"), plugin("provider-owner")],
            [
                consumer(consumer_schema),
                provider("provider", 10, provider_schema),
            ],
            &Authority::default(),
        )
        .unwrap();

        assert_eq!(
            graph
                .import_handle(&ComponentId::parse("consumer").unwrap(), &interface())
                .unwrap()
                .unwrap()
                .exporter(),
            &ComponentId::parse("provider").unwrap()
        );
    }

    #[test]
    fn graph_skips_incompatible_provider_for_lower_priority_compatible_provider() {
        let consumer_schema = InterfaceSchema::new(
            table(&[("name", PhenixSchema::String)]),
            table(&[("value", PhenixSchema::String)]),
        );
        let incompatible = InterfaceSchema::new(
            table(&[("name", PhenixSchema::String)]),
            table(&[("wrong", PhenixSchema::String)]),
        );
        let compatible = consumer_schema.clone();
        let graph = ResolvedComponentGraph::compile(
            [
                plugin("consumer-owner"),
                plugin("high-owner"),
                plugin("low-owner"),
            ],
            [
                consumer(consumer_schema),
                provider("high", 100, incompatible),
                provider("low", 10, compatible),
            ],
            &Authority::default(),
        )
        .unwrap();

        assert_eq!(
            graph
                .import_handle(&ComponentId::parse("consumer").unwrap(), &interface())
                .unwrap()
                .unwrap()
                .exporter(),
            &ComponentId::parse("low").unwrap()
        );
    }

    #[test]
    fn required_import_reports_structural_incompatibility_before_activation() {
        let consumer_schema = InterfaceSchema::new(
            table(&[("name", PhenixSchema::String)]),
            table(&[("value", PhenixSchema::String)]),
        );
        let provider_schema = InterfaceSchema::new(
            table(&[("name", PhenixSchema::String)]),
            table(&[("wrong", PhenixSchema::String)]),
        );
        let error = ResolvedComponentGraph::compile(
            [plugin("consumer-owner"), plugin("provider-owner")],
            [
                consumer(consumer_schema),
                provider("provider", 10, provider_schema),
            ],
            &Authority::default(),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            ComponentGraphError::IncompatibleRequiredImport { exporter, .. }
                if exporter == ComponentId::parse("provider").unwrap()
        ));
    }
}
