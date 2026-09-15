use crate::{
    Authority, ComponentId, ComponentManifest, ConfigContribution, ConfigurationFrontendId,
    ConfigurationFrontendMetadata, FrontendConfigContribution, GraphGenerationId, InterfaceId,
    LayerPolicy, PluginManifest, ResolvedHarness, ResolvedHarnessError, ServiceId,
    SkillResourceMetadata,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComponentChangeKind {
    Added,
    Removed,
    Reconfigured,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentChange {
    pub component: ComponentId,
    pub kind: ComponentChangeKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindingChange {
    pub importer: ComponentId,
    pub interface: InterfaceId,
    pub previous_provider: Option<ComponentId>,
    pub next_provider: Option<ComponentId>,
    pub authority_changed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterpositionChange {
    pub service: ServiceId,
    pub previous: Vec<LayerPolicy>,
    pub next: Vec<LayerPolicy>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceChangeKind {
    Added,
    Removed,
    ContentChanged,
    Reconfigured,
    Upgraded,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceChange {
    pub resource: String,
    pub kind: ResourceChangeKind,
    pub invalidation_targets: BTreeSet<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GraphDiff {
    pub components: Vec<ComponentChange>,
    pub bindings: Vec<BindingChange>,
    pub interposition: Vec<InterpositionChange>,
    pub resources: Vec<ResourceChange>,
}

impl GraphDiff {
    pub fn between(previous: &ResolvedHarness, next: &ResolvedHarness) -> Self {
        Self {
            components: component_changes(previous, next),
            bindings: binding_changes(previous, next),
            interposition: interposition_changes(previous, next),
            resources: resource_changes(previous, next),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.components.is_empty()
            && self.bindings.is_empty()
            && self.interposition.is_empty()
            && self.resources.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReconciliationAction {
    ActivateComponent(ComponentId),
    StopComponent(ComponentId),
    RestartComponent(ComponentId),
    RebindImport {
        importer: ComponentId,
        interface: InterfaceId,
        previous_provider: Option<ComponentId>,
        next_provider: Option<ComponentId>,
        authority_changed: bool,
    },
    ReconfigureInterposition {
        service: ServiceId,
        previous: Vec<LayerPolicy>,
        next: Vec<LayerPolicy>,
    },
    InvalidateResourceDerivedState {
        resource: String,
        targets: BTreeSet<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconciliationPreview {
    pub active_generation: GraphGenerationId,
    pub candidate_generation: GraphGenerationId,
    pub diff: GraphDiff,
    pub transition_plan: Vec<ReconciliationAction>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconciliationResult {
    pub previous_generation: GraphGenerationId,
    pub active_generation: GraphGenerationId,
    pub diff: GraphDiff,
    pub transition_plan: Vec<ReconciliationAction>,
}

#[derive(Clone, Debug)]
pub struct GraphReconciler {
    active: ResolvedHarness,
}

impl GraphReconciler {
    pub fn new(active: ResolvedHarness) -> Self {
        Self { active }
    }

    pub fn active(&self) -> &ResolvedHarness {
        &self.active
    }

    pub fn resolve_candidate(
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        contributions: impl IntoIterator<Item = ConfigContribution>,
        authority_ceiling: &Authority,
    ) -> Result<ResolvedHarness, ResolvedHarnessError> {
        ResolvedHarness::resolve(
            plugin_manifests,
            component_manifests,
            contributions,
            authority_ceiling,
        )
    }

    pub fn resolve_candidate_with_resources(
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        resources: impl IntoIterator<Item = SkillResourceMetadata>,
        contributions: impl IntoIterator<Item = ConfigContribution>,
        authority_ceiling: &Authority,
    ) -> Result<ResolvedHarness, ResolvedHarnessError> {
        ResolvedHarness::resolve_with_resources(
            plugin_manifests,
            component_manifests,
            resources,
            contributions,
            authority_ceiling,
        )
    }

    pub fn resolve_frontend_candidate(
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        frontend_metadata: impl IntoIterator<Item = ConfigurationFrontendMetadata>,
        contributions: impl IntoIterator<Item = (ConfigurationFrontendId, FrontendConfigContribution)>,
        authority_ceiling: &Authority,
    ) -> Result<ResolvedHarness, ResolvedHarnessError> {
        ResolvedHarness::resolve_frontends(
            plugin_manifests,
            component_manifests,
            frontend_metadata,
            contributions,
            authority_ceiling,
        )
    }

    pub fn preview_candidate(&self, candidate: &ResolvedHarness) -> ReconciliationPreview {
        let diff = GraphDiff::between(&self.active, candidate);
        let transition_plan = transition_plan(&self.active, candidate, &diff);
        ReconciliationPreview {
            active_generation: self.active.generation().clone(),
            candidate_generation: candidate.generation().clone(),
            diff,
            transition_plan,
        }
    }

    pub fn activate_candidate(&mut self, candidate: ResolvedHarness) -> ReconciliationResult {
        let preview = self.preview_candidate(&candidate);
        self.active = candidate;
        ReconciliationResult {
            previous_generation: preview.active_generation,
            active_generation: preview.candidate_generation,
            diff: preview.diff,
            transition_plan: preview.transition_plan,
        }
    }

    pub fn reconcile(
        &mut self,
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        contributions: impl IntoIterator<Item = ConfigContribution>,
        authority_ceiling: &Authority,
    ) -> Result<ReconciliationResult, ResolvedHarnessError> {
        let candidate = Self::resolve_candidate(
            plugin_manifests,
            component_manifests,
            contributions,
            authority_ceiling,
        )?;
        Ok(self.activate_candidate(candidate))
    }

    pub fn reconcile_with_resources(
        &mut self,
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        resources: impl IntoIterator<Item = SkillResourceMetadata>,
        contributions: impl IntoIterator<Item = ConfigContribution>,
        authority_ceiling: &Authority,
    ) -> Result<ReconciliationResult, ResolvedHarnessError> {
        let candidate = Self::resolve_candidate_with_resources(
            plugin_manifests,
            component_manifests,
            resources,
            contributions,
            authority_ceiling,
        )?;
        Ok(self.activate_candidate(candidate))
    }

    pub fn reconcile_frontends(
        &mut self,
        plugin_manifests: impl IntoIterator<Item = PluginManifest>,
        component_manifests: impl IntoIterator<Item = ComponentManifest>,
        frontend_metadata: impl IntoIterator<Item = ConfigurationFrontendMetadata>,
        contributions: impl IntoIterator<Item = (ConfigurationFrontendId, FrontendConfigContribution)>,
        authority_ceiling: &Authority,
    ) -> Result<ReconciliationResult, ResolvedHarnessError> {
        let candidate = Self::resolve_frontend_candidate(
            plugin_manifests,
            component_manifests,
            frontend_metadata,
            contributions,
            authority_ceiling,
        )?;
        Ok(self.activate_candidate(candidate))
    }
}

fn transition_plan(
    previous: &ResolvedHarness,
    next: &ResolvedHarness,
    diff: &GraphDiff,
) -> Vec<ReconciliationAction> {
    let mut actions = Vec::new();
    let mut restart = BTreeSet::new();

    for change in &diff.components {
        match change.kind {
            ComponentChangeKind::Added => {
                actions.push(ReconciliationAction::ActivateComponent(
                    change.component.clone(),
                ));
            }
            ComponentChangeKind::Removed => {
                actions.push(ReconciliationAction::StopComponent(
                    change.component.clone(),
                ));
            }
            ComponentChangeKind::Reconfigured => {
                restart.insert(change.component.clone());
                collect_required_dependents(previous, &change.component, &mut restart);
                collect_required_dependents(next, &change.component, &mut restart);
            }
        }
    }

    actions.extend(
        restart
            .into_iter()
            .map(ReconciliationAction::RestartComponent),
    );
    actions.extend(
        diff.bindings
            .iter()
            .map(|change| ReconciliationAction::RebindImport {
                importer: change.importer.clone(),
                interface: change.interface.clone(),
                previous_provider: change.previous_provider.clone(),
                next_provider: change.next_provider.clone(),
                authority_changed: change.authority_changed,
            }),
    );
    actions.extend(diff.interposition.iter().map(|change| {
        ReconciliationAction::ReconfigureInterposition {
            service: change.service.clone(),
            previous: change.previous.clone(),
            next: change.next.clone(),
        }
    }));
    actions.extend(
        diff.resources
            .iter()
            .filter(|change| !change.invalidation_targets.is_empty())
            .map(
                |change| ReconciliationAction::InvalidateResourceDerivedState {
                    resource: change.resource.clone(),
                    targets: change.invalidation_targets.clone(),
                },
            ),
    );
    actions
}

fn collect_required_dependents(
    harness: &ResolvedHarness,
    provider: &ComponentId,
    affected: &mut BTreeSet<ComponentId>,
) {
    let direct: Vec<_> = harness
        .component_graph()
        .components()
        .filter(|component| {
            component.imports.iter().any(|import| {
                import.required
                    && import
                        .binding
                        .as_ref()
                        .is_some_and(|binding| binding.exporter() == provider)
            })
        })
        .map(|component| component.id.clone())
        .collect();

    for dependent in direct {
        if affected.insert(dependent.clone()) {
            collect_required_dependents(harness, &dependent, affected);
        }
    }
}

fn component_changes(previous: &ResolvedHarness, next: &ResolvedHarness) -> Vec<ComponentChange> {
    let previous: BTreeMap<_, _> = previous
        .components()
        .iter()
        .map(|component| (&component.id, component))
        .collect();
    let next: BTreeMap<_, _> = next
        .components()
        .iter()
        .map(|component| (&component.id, component))
        .collect();
    let ids: BTreeSet<_> = previous.keys().chain(next.keys()).copied().collect();

    ids.into_iter()
        .filter_map(|id| match (previous.get(id), next.get(id)) {
            (None, Some(_)) => Some(ComponentChange {
                component: id.clone(),
                kind: ComponentChangeKind::Added,
            }),
            (Some(_), None) => Some(ComponentChange {
                component: id.clone(),
                kind: ComponentChangeKind::Removed,
            }),
            (Some(previous), Some(next)) if previous != next => Some(ComponentChange {
                component: id.clone(),
                kind: ComponentChangeKind::Reconfigured,
            }),
            _ => None,
        })
        .collect()
}

fn binding_changes(previous: &ResolvedHarness, next: &ResolvedHarness) -> Vec<BindingChange> {
    type BindingState = (Option<ComponentId>, Option<Authority>);
    type BindingKey = (ComponentId, InterfaceId);

    fn bindings(harness: &ResolvedHarness) -> BTreeMap<BindingKey, BindingState> {
        harness
            .component_graph()
            .components()
            .flat_map(|component| {
                component.imports.iter().map(move |import| {
                    let binding = import.binding.as_ref();
                    (
                        (component.id.clone(), import.interface.clone()),
                        (
                            binding.map(|binding| binding.exporter().clone()),
                            binding.map(|binding| binding.effective_authority().clone()),
                        ),
                    )
                })
            })
            .collect()
    }

    let previous = bindings(previous);
    let next = bindings(next);
    let keys: BTreeSet<_> = previous.keys().chain(next.keys()).cloned().collect();

    keys.into_iter()
        .filter_map(|(importer, interface)| {
            let previous_state = previous.get(&(importer.clone(), interface.clone()));
            let next_state = next.get(&(importer.clone(), interface.clone()));
            if previous_state == next_state {
                return None;
            }
            Some(BindingChange {
                importer,
                interface,
                previous_provider: previous_state.and_then(|state| state.0.clone()),
                next_provider: next_state.and_then(|state| state.0.clone()),
                authority_changed: previous_state.and_then(|state| state.1.as_ref())
                    != next_state.and_then(|state| state.1.as_ref()),
            })
        })
        .collect()
}

fn interposition_changes(
    previous: &ResolvedHarness,
    next: &ResolvedHarness,
) -> Vec<InterpositionChange> {
    let services: BTreeSet<_> = previous
        .layer_policies()
        .keys()
        .chain(next.layer_policies().keys())
        .cloned()
        .collect();

    services
        .into_iter()
        .filter_map(|service| {
            let previous_layers = previous
                .layer_policies()
                .get(&service)
                .cloned()
                .unwrap_or_default();
            let next_layers = next
                .layer_policies()
                .get(&service)
                .cloned()
                .unwrap_or_default();
            (previous_layers != next_layers).then_some(InterpositionChange {
                service,
                previous: previous_layers,
                next: next_layers,
            })
        })
        .collect()
}

fn resource_changes(previous: &ResolvedHarness, next: &ResolvedHarness) -> Vec<ResourceChange> {
    let previous: BTreeMap<_, _> = previous
        .resources()
        .iter()
        .map(|resource| (resource.identity.as_str(), resource))
        .collect();
    let next: BTreeMap<_, _> = next
        .resources()
        .iter()
        .map(|resource| (resource.identity.as_str(), resource))
        .collect();
    let ids: BTreeSet<_> = previous.keys().chain(next.keys()).copied().collect();

    ids.into_iter()
        .filter_map(|id| match (previous.get(id), next.get(id)) {
            (None, Some(resource)) => Some(ResourceChange {
                resource: id.to_owned(),
                kind: ResourceChangeKind::Added,
                invalidation_targets: resource.invalidation_targets.clone(),
            }),
            (Some(resource), None) => Some(ResourceChange {
                resource: id.to_owned(),
                kind: ResourceChangeKind::Removed,
                invalidation_targets: resource.invalidation_targets.clone(),
            }),
            (Some(previous), Some(next)) if previous != next => {
                let kind = if previous.content_identity != next.content_identity {
                    ResourceChangeKind::ContentChanged
                } else if previous.version != next.version {
                    ResourceChangeKind::Upgraded
                } else {
                    ResourceChangeKind::Reconfigured
                };
                Some(ResourceChange {
                    resource: id.to_owned(),
                    kind,
                    invalidation_targets: previous
                        .invalidation_targets
                        .union(&next.invalidation_targets)
                        .cloned()
                        .collect(),
                })
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CompatibilityMetadata, ComponentExport, ComponentGraphError, ComponentImport,
        ConfigNamespace, ConfigSourceClass, InterfaceId, PluginExecution, PluginId, ReloadPolicy,
    };

    fn plugin(value: &str) -> PluginId {
        PluginId::parse(value).unwrap()
    }

    fn component(value: &str) -> ComponentId {
        ComponentId::parse(value).unwrap()
    }

    fn frontend(value: &str) -> ConfigurationFrontendId {
        ConfigurationFrontendId::parse(value).unwrap()
    }

    fn interface() -> InterfaceId {
        InterfaceId::parse("fixture.echo@1").unwrap()
    }

    fn owner(value: &str) -> PluginManifest {
        PluginManifest {
            id: plugin(value),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }

    fn provider(id: &str, owner: &str, priority: i32) -> ComponentManifest {
        ComponentManifest {
            listeners: Vec::new(),
            id: component(id),
            owner: plugin(owner),
            imports: Vec::new(),
            exports: vec![ComponentExport {
                interface: interface(),
                schema: Default::default(),
                priority,
                required_authority: Authority::default(),
            }],
            maximum_authority: Authority::default(),
        }
    }

    fn consumer() -> ComponentManifest {
        ComponentManifest {
            listeners: Vec::new(),
            id: component("consumer"),
            owner: plugin("consumer-owner"),
            imports: vec![ComponentImport {
                interface: interface(),
                schema: Default::default(),
                required: true,
                authority: Authority::default(),
            }],
            exports: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }

    fn optional_component() -> ComponentManifest {
        ComponentManifest {
            listeners: Vec::new(),
            id: component("optional"),
            owner: plugin("optional-owner"),
            imports: Vec::new(),
            exports: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }

    fn resource(content_identity: &str) -> SkillResourceMetadata {
        SkillResourceMetadata {
            identity: "review-skill".into(),
            version: 1,
            content_identity: content_identity.into(),
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

    fn initial() -> ResolvedHarness {
        ResolvedHarness::resolve(
            [owner("consumer-owner"), owner("provider-a-owner")],
            [consumer(), provider("provider-a", "provider-a-owner", 10)],
            [],
            &Authority::default(),
        )
        .unwrap()
    }

    fn frontend_metadata() -> ConfigurationFrontendMetadata {
        ConfigurationFrontendMetadata {
            id: frontend("phenix-config-dev"),
            version: 1,
            accepted_source_kinds: BTreeSet::from(["inline".into()]),
            exposed_namespaces: BTreeSet::from([
                ConfigNamespace::parse("fixture.policy@1").unwrap()
            ]),
            watch: true,
            required_authority: Authority::default(),
        }
    }

    fn frontend_contribution(value: &str) -> (ConfigurationFrontendId, FrontendConfigContribution) {
        (
            frontend("phenix-config-dev"),
            FrontendConfigContribution {
                source_kind: "inline".into(),
                source_identity: "dev:fixture".into(),
                source_revision: format!("rev:{value}"),
                source_class: ConfigSourceClass::Materialized,
                namespace: ConfigNamespace::parse("fixture.policy@1").unwrap(),
                contract_version: 1,
                precedence: 10,
                value: serde_json::json!({"mode": value}).into(),
                requested_authority: Authority::default(),
            },
        )
    }

    #[test]
    fn candidate_transition_can_be_inspected_before_activation() {
        let active = initial();
        let active_generation = active.generation().clone();
        let candidate = ResolvedHarness::resolve(
            [owner("consumer-owner"), owner("provider-b-owner")],
            [consumer(), provider("provider-b", "provider-b-owner", 10)],
            [],
            &Authority::default(),
        )
        .unwrap();
        let candidate_generation = candidate.generation().clone();
        let mut reconciler = GraphReconciler::new(active);

        let preview = reconciler.preview_candidate(&candidate);

        assert_eq!(reconciler.active().generation(), &active_generation);
        assert_eq!(preview.active_generation, active_generation);
        assert_eq!(preview.candidate_generation, candidate_generation);
        assert!(!preview.diff.bindings.is_empty());
        assert!(!preview.transition_plan.is_empty());

        let result = reconciler.activate_candidate(candidate);
        assert_eq!(result.previous_generation, preview.active_generation);
        assert_eq!(result.active_generation, preview.candidate_generation);
        assert_eq!(result.diff, preview.diff);
        assert_eq!(result.transition_plan, preview.transition_plan);
    }

    #[test]
    fn invalid_candidate_leaves_the_active_generation_unchanged() {
        let initial = initial();
        let generation = initial.generation().clone();
        let mut reconciler = GraphReconciler::new(initial);

        let error = reconciler
            .reconcile(
                [owner("consumer-owner")],
                [consumer()],
                [],
                &Authority::default(),
            )
            .unwrap_err();

        assert!(matches!(
            error,
            ResolvedHarnessError::ComponentGraph(ComponentGraphError::MissingRequiredImport { .. })
        ));
        assert_eq!(reconciler.active().generation(), &generation);
    }

    #[test]
    fn invalid_frontend_candidate_leaves_the_active_generation_unchanged() {
        let initial = initial();
        let generation = initial.generation().clone();
        let mut reconciler = GraphReconciler::new(initial);
        let mut invalid = frontend_contribution("strict");
        invalid.1.source_kind = "undeclared".into();

        let error = reconciler
            .reconcile_frontends(
                [owner("consumer-owner"), owner("provider-a-owner")],
                [consumer(), provider("provider-a", "provider-a-owner", 10)],
                [frontend_metadata()],
                [invalid],
                &Authority::default(),
            )
            .unwrap_err();

        assert!(matches!(
            error,
            ResolvedHarnessError::ConfigurationFrontend { .. }
        ));
        assert_eq!(reconciler.active().generation(), &generation);
    }

    #[test]
    fn valid_frontend_candidate_uses_the_same_resolver_before_activation() {
        let initial = initial();
        let previous = initial.generation().clone();
        let mut reconciler = GraphReconciler::new(initial);

        let result = reconciler
            .reconcile_frontends(
                [owner("consumer-owner"), owner("provider-a-owner")],
                [consumer(), provider("provider-a", "provider-a-owner", 10)],
                [frontend_metadata()],
                [frontend_contribution("strict")],
                &Authority::default(),
            )
            .unwrap();

        assert_ne!(result.active_generation, previous);
        assert_eq!(reconciler.active().generation(), &result.active_generation);
        assert_eq!(
            reconciler.active().configuration().entries()[0].attributions[0]
                .source
                .frontend,
            frontend("phenix-config-dev")
        );
    }

    #[test]
    fn skill_content_change_invalidates_only_declared_derived_state() {
        let initial = ResolvedHarness::resolve_with_resources(
            [],
            [],
            [resource("sha256:one")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let previous_generation = initial.generation().clone();
        let mut reconciler = GraphReconciler::new(initial);
        let result = reconciler
            .reconcile_with_resources([], [], [resource("sha256:two")], [], &Authority::default())
            .unwrap();

        assert_ne!(result.active_generation, previous_generation);
        assert!(result.diff.components.is_empty());
        assert!(result.diff.bindings.is_empty());
        assert!(result.diff.interposition.is_empty());
        assert_eq!(
            result.diff.resources,
            vec![ResourceChange {
                resource: "review-skill".into(),
                kind: ResourceChangeKind::ContentChanged,
                invalidation_targets: BTreeSet::from(["skill-index".into()]),
            }]
        );
        assert_eq!(
            result.transition_plan,
            vec![ReconciliationAction::InvalidateResourceDerivedState {
                resource: "review-skill".into(),
                targets: BTreeSet::from(["skill-index".into()]),
            }]
        );
    }

    #[test]
    fn skill_version_change_is_an_explicit_upgrade() {
        let initial = ResolvedHarness::resolve_with_resources(
            [],
            [],
            [resource("sha256:one")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let mut upgraded = resource("sha256:one");
        upgraded.version = 2;
        let mut reconciler = GraphReconciler::new(initial);
        let result = reconciler
            .reconcile_with_resources([], [], [upgraded], [], &Authority::default())
            .unwrap();

        assert_eq!(
            result.diff.resources,
            vec![ResourceChange {
                resource: "review-skill".into(),
                kind: ResourceChangeKind::Upgraded,
                invalidation_targets: BTreeSet::from(["skill-index".into()]),
            }]
        );
        assert_eq!(
            result.transition_plan,
            vec![ReconciliationAction::InvalidateResourceDerivedState {
                resource: "review-skill".into(),
                targets: BTreeSet::from(["skill-index".into()]),
            }]
        );
    }

    #[test]
    fn skill_metadata_change_is_an_explicit_reconfiguration() {
        let initial = ResolvedHarness::resolve_with_resources(
            [],
            [],
            [resource("sha256:one")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let mut reconfigured = resource("sha256:one");
        reconfigured.priority = 10;
        let mut reconciler = GraphReconciler::new(initial);
        let result = reconciler
            .reconcile_with_resources([], [], [reconfigured], [], &Authority::default())
            .unwrap();

        assert_eq!(
            result.diff.resources,
            vec![ResourceChange {
                resource: "review-skill".into(),
                kind: ResourceChangeKind::Reconfigured,
                invalidation_targets: BTreeSet::from(["skill-index".into()]),
            }]
        );
        assert_eq!(
            result.transition_plan,
            vec![ReconciliationAction::InvalidateResourceDerivedState {
                resource: "review-skill".into(),
                targets: BTreeSet::from(["skill-index".into()]),
            }]
        );
    }

    #[test]
    fn invalid_resource_candidate_leaves_the_active_generation_unchanged() {
        let initial = ResolvedHarness::resolve_with_resources(
            [],
            [],
            [resource("sha256:one")],
            [],
            &Authority::default(),
        )
        .unwrap();
        let generation = initial.generation().clone();
        let mut reconciler = GraphReconciler::new(initial);
        let mut invalid = resource("sha256:two");
        invalid.dependencies.insert("missing".into());

        assert!(matches!(
            reconciler.reconcile_with_resources([], [], [invalid], [], &Authority::default(),),
            Err(ResolvedHarnessError::MissingResourceDependency { .. })
        ));
        assert_eq!(reconciler.active().generation(), &generation);
    }

    #[test]
    fn layer_policy_only_change_has_an_explicit_interposition_diff_and_action() {
        let service = ServiceId::parse("fixture.interposition@1").unwrap();
        let policy = |priority| {
            BTreeMap::from([(
                service.clone(),
                vec![LayerPolicy {
                    plugin: plugin("layer-owner"),
                    priority,
                    required: false,
                    enabled: true,
                }],
            )])
        };
        let resolved = |priority| {
            ResolvedHarness::resolve_with_layer_policies(
                [owner("consumer-owner"), owner("provider-a-owner")],
                [consumer(), provider("provider-a", "provider-a-owner", 10)],
                [],
                policy(priority),
                &Authority::default(),
            )
            .unwrap()
        };
        let initial = resolved(10);
        let previous_generation = initial.generation().clone();
        let candidate = resolved(20);
        let next_generation = candidate.generation().clone();
        assert_ne!(previous_generation, next_generation);

        let mut reconciler = GraphReconciler::new(initial);
        let result = reconciler.activate_candidate(candidate);

        assert!(!result.diff.is_empty());
        assert!(result.diff.components.is_empty());
        assert!(result.diff.bindings.is_empty());
        assert!(result.diff.resources.is_empty());
        assert_eq!(
            result.diff.interposition,
            vec![InterpositionChange {
                service: service.clone(),
                previous: policy(10).remove(&service).unwrap(),
                next: policy(20).remove(&service).unwrap(),
            }]
        );
        assert_eq!(
            result.transition_plan,
            vec![ReconciliationAction::ReconfigureInterposition {
                service: service.clone(),
                previous: policy(10).remove(&service).unwrap(),
                next: policy(20).remove(&service).unwrap(),
            }]
        );
        assert_eq!(reconciler.active().generation(), &next_generation);
    }

    #[test]
    fn valid_provider_rebinding_activates_a_new_generation_with_an_explicit_edge_diff() {
        let mut reconciler = GraphReconciler::new(initial());
        let previous = reconciler.active().generation().clone();
        let result = reconciler
            .reconcile(
                [owner("consumer-owner"), owner("provider-b-owner")],
                [consumer(), provider("provider-b", "provider-b-owner", 10)],
                [],
                &Authority::default(),
            )
            .unwrap();

        assert_ne!(result.active_generation, previous);
        assert_eq!(reconciler.active().generation(), &result.active_generation);
        assert_eq!(result.diff.bindings.len(), 1);
        assert_eq!(result.diff.bindings[0].importer, component("consumer"));
        assert_eq!(
            result.diff.bindings[0].previous_provider,
            Some(component("provider-a"))
        );
        assert_eq!(
            result.diff.bindings[0].next_provider,
            Some(component("provider-b"))
        );
        assert!(result
            .transition_plan
            .contains(&ReconciliationAction::RebindImport {
                importer: component("consumer"),
                interface: interface(),
                previous_provider: Some(component("provider-a")),
                next_provider: Some(component("provider-b")),
                authority_changed: false,
            }));
    }

    #[test]
    fn adding_an_unreferenced_optional_component_does_not_change_existing_bindings() {
        let mut reconciler = GraphReconciler::new(initial());
        let result = reconciler
            .reconcile(
                [
                    owner("consumer-owner"),
                    owner("provider-a-owner"),
                    owner("optional-owner"),
                ],
                [
                    consumer(),
                    provider("provider-a", "provider-a-owner", 10),
                    optional_component(),
                ],
                [],
                &Authority::default(),
            )
            .unwrap();

        assert!(result.diff.bindings.is_empty());
        assert!(result.diff.resources.is_empty());
        assert_eq!(
            result.diff.components,
            vec![ComponentChange {
                component: component("optional"),
                kind: ComponentChangeKind::Added
            }]
        );
        assert_eq!(
            result.transition_plan,
            vec![ReconciliationAction::ActivateComponent(component(
                "optional"
            ))]
        );
    }

    #[test]
    fn removing_an_unreferenced_optional_component_does_not_restart_existing_components() {
        let initial = ResolvedHarness::resolve(
            [
                owner("consumer-owner"),
                owner("provider-a-owner"),
                owner("optional-owner"),
            ],
            [
                consumer(),
                provider("provider-a", "provider-a-owner", 10),
                optional_component(),
            ],
            [],
            &Authority::default(),
        )
        .unwrap();
        let mut reconciler = GraphReconciler::new(initial);
        let result = reconciler
            .reconcile(
                [owner("consumer-owner"), owner("provider-a-owner")],
                [consumer(), provider("provider-a", "provider-a-owner", 10)],
                [],
                &Authority::default(),
            )
            .unwrap();

        assert!(result.diff.bindings.is_empty());
        assert!(result.diff.resources.is_empty());
        assert_eq!(
            result.diff.components,
            vec![ComponentChange {
                component: component("optional"),
                kind: ComponentChangeKind::Removed
            }]
        );
        assert_eq!(
            result.transition_plan,
            vec![ReconciliationAction::StopComponent(component("optional"))]
        );
    }

    #[test]
    fn reconfiguring_a_provider_restarts_its_required_dependency_closure() {
        let mut reconciler = GraphReconciler::new(initial());
        let result = reconciler
            .reconcile(
                [owner("consumer-owner"), owner("provider-a-owner")],
                [consumer(), provider("provider-a", "provider-a-owner", 20)],
                [],
                &Authority::default(),
            )
            .unwrap();

        assert_eq!(
            result.transition_plan,
            vec![
                ReconciliationAction::RestartComponent(component("consumer")),
                ReconciliationAction::RestartComponent(component("provider-a")),
            ]
        );
    }
}
