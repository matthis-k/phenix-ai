use crate::{
    ComponentId, ConfigurationFrontendId, GraphGenerationId, GraphReconciler, PluginId,
    ReconciliationAction, ReconciliationPreview, ReloadPolicy, ResolvedCompositionMetadata,
    ResolvedHarness,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetadataChangeKind {
    Added,
    Removed,
    Reconfigured,
    Upgraded,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageMetadataChange {
    pub plugin: PluginId,
    pub kind: MetadataChangeKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentMetadataChange {
    pub component: ComponentId,
    pub kind: MetadataChangeKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendMetadataChange {
    pub frontend: ConfigurationFrontendId,
    pub kind: MetadataChangeKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceMetadataChange {
    pub resource: String,
    pub kind: MetadataChangeKind,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CompositionMetadataDiff {
    pub packages: Vec<PackageMetadataChange>,
    pub components: Vec<ComponentMetadataChange>,
    pub frontends: Vec<FrontendMetadataChange>,
    pub resources: Vec<ResourceMetadataChange>,
}

impl CompositionMetadataDiff {
    pub fn between(
        previous: &ResolvedCompositionMetadata,
        next: &ResolvedCompositionMetadata,
    ) -> Self {
        Self {
            packages: metadata_changes(
                previous
                    .packages()
                    .iter()
                    .map(|value| (&value.manifest.id, value)),
                next.packages()
                    .iter()
                    .map(|value| (&value.manifest.id, value)),
                |previous, next| {
                    if previous.manifest.version != next.manifest.version {
                        MetadataChangeKind::Upgraded
                    } else {
                        MetadataChangeKind::Reconfigured
                    }
                },
                |plugin, kind| PackageMetadataChange {
                    plugin: plugin.clone(),
                    kind,
                },
            ),
            components: metadata_changes(
                previous
                    .components()
                    .iter()
                    .map(|value| (&value.manifest.id, value)),
                next.components()
                    .iter()
                    .map(|value| (&value.manifest.id, value)),
                |previous, next| {
                    if previous.version != next.version {
                        MetadataChangeKind::Upgraded
                    } else {
                        MetadataChangeKind::Reconfigured
                    }
                },
                |component, kind| ComponentMetadataChange {
                    component: component.clone(),
                    kind,
                },
            ),
            frontends: metadata_changes(
                previous.frontends().iter().map(|value| (&value.id, value)),
                next.frontends().iter().map(|value| (&value.id, value)),
                |previous, next| {
                    if previous.version != next.version {
                        MetadataChangeKind::Upgraded
                    } else {
                        MetadataChangeKind::Reconfigured
                    }
                },
                |frontend, kind| FrontendMetadataChange {
                    frontend: frontend.clone(),
                    kind,
                },
            ),
            resources: metadata_changes(
                previous
                    .resources()
                    .iter()
                    .map(|value| (&value.identity, value)),
                next.resources()
                    .iter()
                    .map(|value| (&value.identity, value)),
                |previous, next| {
                    if previous.version != next.version {
                        MetadataChangeKind::Upgraded
                    } else {
                        MetadataChangeKind::Reconfigured
                    }
                },
                |resource, kind| ResourceMetadataChange {
                    resource: resource.clone(),
                    kind,
                },
            ),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
            && self.components.is_empty()
            && self.frontends.is_empty()
            && self.resources.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MetadataReconciliationError {
    ActiveGenerationMismatch {
        graph: GraphGenerationId,
        metadata: GraphGenerationId,
    },
    CandidateGenerationMismatch {
        graph: GraphGenerationId,
        metadata: GraphGenerationId,
    },
    DrainRequired {
        component: ComponentId,
    },
    MigrationRequired {
        component: ComponentId,
    },
    ResourceDrainRequired {
        resource: String,
    },
    ResourceMigrationRequired {
        resource: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetadataReconciliationPreview {
    pub graph: ReconciliationPreview,
    pub metadata: CompositionMetadataDiff,
}

impl GraphReconciler {
    pub fn preview_candidate_with_metadata(
        &self,
        active_metadata: &ResolvedCompositionMetadata,
        candidate: &ResolvedHarness,
        candidate_metadata: &ResolvedCompositionMetadata,
    ) -> Result<MetadataReconciliationPreview, MetadataReconciliationError> {
        if active_metadata.generation() != self.active().generation() {
            return Err(MetadataReconciliationError::ActiveGenerationMismatch {
                graph: self.active().generation().clone(),
                metadata: active_metadata.generation().clone(),
            });
        }
        if candidate_metadata.generation() != candidate.generation() {
            return Err(MetadataReconciliationError::CandidateGenerationMismatch {
                graph: candidate.generation().clone(),
                metadata: candidate_metadata.generation().clone(),
            });
        }

        let mut graph = self.preview_candidate(candidate);
        let metadata = CompositionMetadataDiff::between(active_metadata, candidate_metadata);
        let mut restart = BTreeSet::new();

        for change in &metadata.components {
            if matches!(
                change.kind,
                MetadataChangeKind::Reconfigured | MetadataChangeKind::Upgraded
            ) && component_survives(self.active(), candidate, &change.component)
            {
                apply_component_reload_policy(
                    active_metadata,
                    candidate_metadata,
                    &change.component,
                    &mut restart,
                )?;
            }
        }

        for change in &metadata.packages {
            for component in
                components_owned_by_package(active_metadata, candidate_metadata, &change.plugin)
            {
                if component_survives(self.active(), candidate, &component) {
                    apply_package_reload_policy(
                        active_metadata,
                        candidate_metadata,
                        &change.plugin,
                        &component,
                        &mut restart,
                    )?;
                }
            }
        }

        for change in &metadata.frontends {
            for plugin in
                packages_declaring_frontend(active_metadata, candidate_metadata, &change.frontend)
            {
                for component in
                    components_owned_by_package(active_metadata, candidate_metadata, &plugin)
                {
                    if component_survives(self.active(), candidate, &component) {
                        apply_package_reload_policy(
                            active_metadata,
                            candidate_metadata,
                            &plugin,
                            &component,
                            &mut restart,
                        )?;
                    }
                }
            }
        }

        for change in &metadata.resources {
            if matches!(
                change.kind,
                MetadataChangeKind::Reconfigured | MetadataChangeKind::Upgraded
            ) && resource_survives(active_metadata, candidate_metadata, &change.resource)
            {
                apply_resource_reload_policy(
                    active_metadata,
                    candidate_metadata,
                    &change.resource,
                )?;
            }

            let targets = resource_invalidation_targets(
                active_metadata,
                candidate_metadata,
                &change.resource,
            );
            if targets.is_empty()
                || graph.transition_plan.iter().any(|action| {
                    matches!(
                        action,
                        ReconciliationAction::InvalidateResourceDerivedState { resource, .. }
                            if resource == &change.resource
                    )
                })
            {
                continue;
            }
            graph
                .transition_plan
                .push(ReconciliationAction::InvalidateResourceDerivedState {
                    resource: change.resource.clone(),
                    targets,
                });
        }

        let already_restarted: BTreeSet<_> = graph
            .transition_plan
            .iter()
            .filter_map(|action| match action {
                ReconciliationAction::RestartComponent(component) => Some(component.clone()),
                _ => None,
            })
            .collect();
        graph.transition_plan.extend(
            restart
                .into_iter()
                .filter(|component| !already_restarted.contains(component))
                .map(ReconciliationAction::RestartComponent),
        );

        Ok(MetadataReconciliationPreview { graph, metadata })
    }
}

fn apply_component_reload_policy(
    previous: &ResolvedCompositionMetadata,
    next: &ResolvedCompositionMetadata,
    component: &ComponentId,
    restart: &mut BTreeSet<ComponentId>,
) -> Result<(), MetadataReconciliationError> {
    let policies = previous
        .components()
        .iter()
        .chain(next.components().iter())
        .filter(|metadata| &metadata.manifest.id == component)
        .map(|metadata| metadata.reload_policy);
    apply_reload_policies(component, policies, restart)
}

fn apply_package_reload_policy(
    previous: &ResolvedCompositionMetadata,
    next: &ResolvedCompositionMetadata,
    plugin: &PluginId,
    component: &ComponentId,
    restart: &mut BTreeSet<ComponentId>,
) -> Result<(), MetadataReconciliationError> {
    let policies = previous
        .packages()
        .iter()
        .chain(next.packages().iter())
        .filter(|metadata| &metadata.manifest.id == plugin)
        .map(|metadata| metadata.reload_policy);
    apply_reload_policies(component, policies, restart)
}

fn apply_resource_reload_policy(
    previous: &ResolvedCompositionMetadata,
    next: &ResolvedCompositionMetadata,
    resource: &str,
) -> Result<(), MetadataReconciliationError> {
    for policy in previous
        .resources()
        .iter()
        .chain(next.resources().iter())
        .filter(|metadata| metadata.identity == resource)
        .map(|metadata| metadata.reload_policy)
    {
        match policy {
            ReloadPolicy::Retain | ReloadPolicy::Restart => {}
            ReloadPolicy::DrainAndRestart => {
                return Err(MetadataReconciliationError::ResourceDrainRequired {
                    resource: resource.to_owned(),
                });
            }
            ReloadPolicy::MigrationRequired => {
                return Err(MetadataReconciliationError::ResourceMigrationRequired {
                    resource: resource.to_owned(),
                });
            }
        }
    }
    Ok(())
}

fn apply_reload_policies(
    component: &ComponentId,
    policies: impl IntoIterator<Item = ReloadPolicy>,
    restart: &mut BTreeSet<ComponentId>,
) -> Result<(), MetadataReconciliationError> {
    let mut requires_restart = false;
    for policy in policies {
        match policy {
            ReloadPolicy::Retain => {}
            ReloadPolicy::Restart => requires_restart = true,
            ReloadPolicy::DrainAndRestart => {
                return Err(MetadataReconciliationError::DrainRequired {
                    component: component.clone(),
                });
            }
            ReloadPolicy::MigrationRequired => {
                return Err(MetadataReconciliationError::MigrationRequired {
                    component: component.clone(),
                });
            }
        }
    }
    if requires_restart {
        restart.insert(component.clone());
    }
    Ok(())
}

fn metadata_changes<'a, K, V, I, J, G, F, C>(previous: I, next: J, classify: G, make: F) -> Vec<C>
where
    K: Clone + Ord + 'a,
    V: PartialEq + 'a,
    I: IntoIterator<Item = (&'a K, &'a V)>,
    J: IntoIterator<Item = (&'a K, &'a V)>,
    G: Fn(&V, &V) -> MetadataChangeKind,
    F: Fn(&K, MetadataChangeKind) -> C,
{
    let previous: BTreeMap<_, _> = previous.into_iter().collect();
    let next: BTreeMap<_, _> = next.into_iter().collect();
    let keys: BTreeSet<_> = previous.keys().chain(next.keys()).copied().collect();

    keys.into_iter()
        .filter_map(|key| match (previous.get(key), next.get(key)) {
            (None, Some(_)) => Some(make(key, MetadataChangeKind::Added)),
            (Some(_), None) => Some(make(key, MetadataChangeKind::Removed)),
            (Some(previous), Some(next)) if previous != next => {
                Some(make(key, classify(previous, next)))
            }
            _ => None,
        })
        .collect()
}

fn component_survives(
    previous: &ResolvedHarness,
    next: &ResolvedHarness,
    component: &ComponentId,
) -> bool {
    previous
        .components()
        .iter()
        .any(|value| &value.id == component)
        && next.components().iter().any(|value| &value.id == component)
}

fn resource_survives(
    previous: &ResolvedCompositionMetadata,
    next: &ResolvedCompositionMetadata,
    resource: &str,
) -> bool {
    previous
        .resources()
        .iter()
        .any(|metadata| metadata.identity == resource)
        && next
            .resources()
            .iter()
            .any(|metadata| metadata.identity == resource)
}

fn components_owned_by_package(
    previous: &ResolvedCompositionMetadata,
    next: &ResolvedCompositionMetadata,
    plugin: &PluginId,
) -> BTreeSet<ComponentId> {
    previous
        .components()
        .iter()
        .chain(next.components().iter())
        .filter(|component| &component.manifest.owner == plugin)
        .map(|component| component.manifest.id.clone())
        .collect()
}

fn packages_declaring_frontend(
    previous: &ResolvedCompositionMetadata,
    next: &ResolvedCompositionMetadata,
    frontend: &ConfigurationFrontendId,
) -> BTreeSet<PluginId> {
    previous
        .packages()
        .iter()
        .chain(next.packages().iter())
        .filter(|package| package.configuration_frontends.contains(frontend))
        .map(|package| package.manifest.id.clone())
        .collect()
}

fn resource_invalidation_targets(
    previous: &ResolvedCompositionMetadata,
    next: &ResolvedCompositionMetadata,
    resource: &str,
) -> BTreeSet<String> {
    previous
        .resources()
        .iter()
        .chain(next.resources().iter())
        .filter(|metadata| metadata.identity == resource)
        .flat_map(|metadata| metadata.invalidation_targets.iter().cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Authority, CompatibilityMetadata, ComponentHostKind, ComponentManifest,
        ComponentRuntimeMetadata, ComponentStateClass, CompositionMetadataInput, PluginExecution,
        PluginManifest, PluginPackageMetadata, ReloadPolicy, SkillResourceMetadata,
    };

    fn fixture() -> CompositionMetadataInput {
        let plugin = PluginId::parse("fixture.plugin").unwrap();
        let component = ComponentId::parse("fixture.component").unwrap();
        CompositionMetadataInput {
            packages: vec![PluginPackageMetadata {
                manifest: PluginManifest {
                    id: plugin.clone(),
                    version: 1,
                    execution: PluginExecution::Embedded,
                    dependencies: Vec::new(),
                    services: Vec::new(),
                    resource_namespaces: Vec::new(),
                    maximum_authority: Authority::default(),
                },
                packaged_components: BTreeSet::from([component.clone()]),
                packaged_resources: BTreeSet::new(),
                packaged_skills: BTreeSet::new(),
                compatibility: CompatibilityMetadata {
                    minimum_kernel_version: 1,
                    maximum_kernel_version: None,
                },
                durable_namespaces: BTreeSet::new(),
                migrations: Vec::new(),
                configuration_frontends: BTreeSet::new(),
                component_hosts: BTreeSet::from([ComponentHostKind::EmbeddedRust]),
                reload_policy: ReloadPolicy::Restart,
            }],
            components: vec![ComponentRuntimeMetadata {
                manifest: ComponentManifest {
                    listeners: Vec::new(),
                    id: component,
                    owner: plugin,
                    imports: Vec::new(),
                    exports: Vec::new(),
                    maximum_authority: Authority::default(),
                },
                version: 1,
                configuration_contracts: BTreeSet::new(),
                requested_capabilities: BTreeSet::new(),
                state_class: ComponentStateClass::Stateless,
                reload_policy: ReloadPolicy::Restart,
                interposition_interfaces: BTreeSet::new(),
                event_contributions: BTreeSet::new(),
                controller_contributions: BTreeSet::new(),
            }],
            resources: Vec::new(),
            configuration: Vec::new(),
        }
    }

    fn resource() -> SkillResourceMetadata {
        SkillResourceMetadata {
            identity: "fixture.skill".into(),
            version: 1,
            content_identity: "sha256:fixture".into(),
            dependencies: BTreeSet::new(),
            conflicts: BTreeSet::new(),
            triggers: BTreeSet::from(["review".into()]),
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

    #[test]
    fn package_restart_policy_produces_an_explicit_restart_plan() {
        let mut active_input = fixture();
        active_input.packages[0].reload_policy = ReloadPolicy::Restart;
        let mut candidate_input = active_input.clone();
        candidate_input.packages[0]
            .compatibility
            .maximum_kernel_version = Some(2);
        let (active, active_metadata) = active_input
            .resolve_inspectable(&Authority::default())
            .unwrap();
        let (candidate, candidate_metadata) = candidate_input
            .resolve_inspectable(&Authority::default())
            .unwrap();
        let component = ComponentId::parse("fixture.component").unwrap();
        let reconciler = GraphReconciler::new(active);

        let preview = reconciler
            .preview_candidate_with_metadata(&active_metadata, &candidate, &candidate_metadata)
            .unwrap();

        assert!(preview.graph.diff.is_empty());
        assert_eq!(preview.metadata.packages.len(), 1);
        assert!(preview
            .graph
            .transition_plan
            .contains(&ReconciliationAction::RestartComponent(component)));
    }

    #[test]
    fn component_version_change_is_classified_as_an_upgrade() {
        let active_input = fixture();
        let mut candidate_input = fixture();
        candidate_input.components[0].version = 2;
        let (active, active_metadata) = active_input
            .resolve_inspectable(&Authority::default())
            .unwrap();
        let (candidate, candidate_metadata) = candidate_input
            .resolve_inspectable(&Authority::default())
            .unwrap();
        let component = ComponentId::parse("fixture.component").unwrap();
        let reconciler = GraphReconciler::new(active);

        let preview = reconciler
            .preview_candidate_with_metadata(&active_metadata, &candidate, &candidate_metadata)
            .unwrap();

        assert_eq!(
            preview.metadata.components,
            vec![ComponentMetadataChange {
                component: component.clone(),
                kind: MetadataChangeKind::Upgraded,
            }]
        );
        assert!(preview
            .graph
            .transition_plan
            .contains(&ReconciliationAction::RestartComponent(component)));
    }

    #[test]
    fn resource_metadata_upgrade_is_inspectable_and_invalidates_derived_state() {
        let mut active_input = fixture();
        active_input.packages[0]
            .packaged_resources
            .insert("fixture.skill".into());
        active_input.resources.push(resource());
        let mut candidate_input = active_input.clone();
        candidate_input.resources[0].version = 2;
        candidate_input.resources[0].priority = 10;
        let (active, active_metadata) = active_input
            .resolve_inspectable(&Authority::default())
            .unwrap();
        let (candidate, candidate_metadata) = candidate_input
            .resolve_inspectable(&Authority::default())
            .unwrap();
        let reconciler = GraphReconciler::new(active);

        let preview = reconciler
            .preview_candidate_with_metadata(&active_metadata, &candidate, &candidate_metadata)
            .unwrap();

        assert_eq!(preview.graph.diff.resources.len(), 1);
        assert_eq!(
            preview.metadata.resources,
            vec![ResourceMetadataChange {
                resource: "fixture.skill".into(),
                kind: MetadataChangeKind::Upgraded,
            }]
        );
        assert!(preview.graph.transition_plan.contains(
            &ReconciliationAction::InvalidateResourceDerivedState {
                resource: "fixture.skill".into(),
                targets: BTreeSet::from(["skill-index".into()]),
            }
        ));
    }

    #[test]
    fn drain_policy_rejects_plain_restart_reconciliation() {
        let active_input = fixture();
        let mut candidate_input = fixture();
        candidate_input.packages[0].reload_policy = ReloadPolicy::DrainAndRestart;
        let (active, active_metadata) = active_input
            .resolve_inspectable(&Authority::default())
            .unwrap();
        let (candidate, candidate_metadata) = candidate_input
            .resolve_inspectable(&Authority::default())
            .unwrap();
        let component = ComponentId::parse("fixture.component").unwrap();
        let reconciler = GraphReconciler::new(active);

        assert_eq!(
            reconciler.preview_candidate_with_metadata(
                &active_metadata,
                &candidate,
                &candidate_metadata,
            ),
            Err(MetadataReconciliationError::DrainRequired { component })
        );
    }

    #[test]
    fn component_lifecycle_change_produces_an_explicit_restart_plan() {
        let active_input = fixture();
        let mut candidate_input = fixture();
        candidate_input.components[0].state_class = ComponentStateClass::Ephemeral;
        let (active, active_metadata) = active_input
            .resolve_inspectable(&Authority::default())
            .unwrap();
        let (candidate, candidate_metadata) = candidate_input
            .resolve_inspectable(&Authority::default())
            .unwrap();
        let component = ComponentId::parse("fixture.component").unwrap();
        let reconciler = GraphReconciler::new(active);

        let preview = reconciler
            .preview_candidate_with_metadata(&active_metadata, &candidate, &candidate_metadata)
            .unwrap();

        assert!(preview.graph.diff.is_empty());
        assert_eq!(preview.metadata.components.len(), 1);
        assert!(preview
            .graph
            .transition_plan
            .contains(&ReconciliationAction::RestartComponent(component)));
    }

    #[test]
    fn retain_policy_does_not_restart_a_surviving_component_for_metadata_only_change() {
        let mut active_input = fixture();
        active_input.components[0].reload_policy = ReloadPolicy::Retain;
        let mut candidate_input = active_input.clone();
        candidate_input.components[0].state_class = ComponentStateClass::Ephemeral;
        let (active, active_metadata) = active_input
            .resolve_inspectable(&Authority::default())
            .unwrap();
        let (candidate, candidate_metadata) = candidate_input
            .resolve_inspectable(&Authority::default())
            .unwrap();
        let component = ComponentId::parse("fixture.component").unwrap();
        let reconciler = GraphReconciler::new(active);

        let preview = reconciler
            .preview_candidate_with_metadata(&active_metadata, &candidate, &candidate_metadata)
            .unwrap();

        assert!(preview
            .metadata
            .components
            .iter()
            .any(|change| change.component == component));
        assert!(!preview
            .graph
            .transition_plan
            .contains(&ReconciliationAction::RestartComponent(component)));
    }

    #[test]
    fn migration_required_policy_rejects_automatic_reconciliation() {
        let active_input = fixture();
        let mut candidate_input = fixture();
        candidate_input.components[0].reload_policy = ReloadPolicy::MigrationRequired;
        let (active, active_metadata) = active_input
            .resolve_inspectable(&Authority::default())
            .unwrap();
        let (candidate, candidate_metadata) = candidate_input
            .resolve_inspectable(&Authority::default())
            .unwrap();
        let component = ComponentId::parse("fixture.component").unwrap();
        let reconciler = GraphReconciler::new(active);

        assert_eq!(
            reconciler.preview_candidate_with_metadata(
                &active_metadata,
                &candidate,
                &candidate_metadata,
            ),
            Err(MetadataReconciliationError::MigrationRequired { component })
        );
    }
}
