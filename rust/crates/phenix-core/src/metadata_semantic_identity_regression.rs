use crate::{
    Authority, CompatibilityMetadata, ComponentHostKind, ComponentId, ComponentManifest,
    ComponentRuntimeMetadata, ComponentStateClass, CompositionMetadataInput, ConfigNamespace,
    DurableMigrationMetadata, EventTypeId, GraphReconciler, InterfaceId, PluginExecution, PluginId,
    PluginManifest, PluginPackageMetadata, ReloadPolicy, ResolvedGeneration, ResourceNamespace,
};
use std::collections::BTreeSet;

fn plugin() -> PluginId {
    PluginId::parse("fixture.plugin").unwrap()
}

fn component() -> ComponentId {
    ComponentId::parse("fixture.component").unwrap()
}

fn package() -> PluginPackageMetadata {
    PluginPackageMetadata {
        manifest: PluginManifest {
            id: plugin(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        },
        packaged_components: BTreeSet::from([component()]),
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
    }
}

fn component_metadata() -> ComponentRuntimeMetadata {
    ComponentRuntimeMetadata {
        manifest: ComponentManifest {
            listeners: Vec::new(),
            id: component(),
            owner: plugin(),
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
    }
}

fn resolve_inspectable(
    package: PluginPackageMetadata,
    component: ComponentRuntimeMetadata,
) -> (ResolvedGeneration, crate::ResolvedCompositionMetadata) {
    CompositionMetadataInput {
        packages: vec![package],
        components: vec![component],
        resources: Vec::new(),
        configuration: Vec::new(),
    }
    .resolve_inspectable(&Authority::default())
    .unwrap()
}

fn resolve_harness(
    package: PluginPackageMetadata,
    component: ComponentRuntimeMetadata,
) -> ResolvedGeneration {
    resolve_inspectable(package, component).0
}

fn resolve(
    package: PluginPackageMetadata,
    component: ComponentRuntimeMetadata,
) -> crate::GenerationId {
    resolve_harness(package, component).generation().clone()
}

#[test]
fn component_lifecycle_metadata_changes_stable_generation_identity() {
    let baseline = resolve(package(), component_metadata());
    let mut changed = component_metadata();
    changed.state_class = ComponentStateClass::Durable;
    changed.reload_policy = ReloadPolicy::MigrationRequired;

    assert_ne!(baseline, resolve(package(), changed));
}

#[test]
fn component_contract_and_contribution_metadata_change_stable_generation_identity() {
    let baseline = resolve(package(), component_metadata());

    let mut changed = component_metadata();
    changed
        .configuration_contracts
        .insert(ConfigNamespace::parse("fixture.config@1").unwrap());
    assert_ne!(baseline, resolve(package(), changed));

    let mut changed = component_metadata();
    changed
        .interposition_interfaces
        .insert(InterfaceId::parse("fixture.interposition@1").unwrap());
    assert_ne!(baseline, resolve(package(), changed));

    let mut changed = component_metadata();
    changed
        .event_contributions
        .insert(EventTypeId::parse("fixture.event").unwrap());
    assert_ne!(baseline, resolve(package(), changed));

    let mut changed = component_metadata();
    changed
        .controller_contributions
        .insert("fixture.controller".into());
    assert_ne!(baseline, resolve(package(), changed));
}

#[test]
fn package_host_and_reload_metadata_changes_stable_generation_identity() {
    let baseline = resolve(package(), component_metadata());
    let mut changed = package();
    changed.component_hosts = BTreeSet::from([ComponentHostKind::Wasm]);
    changed.reload_policy = ReloadPolicy::DrainAndRestart;

    assert_ne!(baseline, resolve(changed, component_metadata()));
}

#[test]
fn compatible_kernel_range_changes_stable_generation_identity() {
    let baseline = resolve(package(), component_metadata());
    let mut changed = package();
    changed.compatibility.maximum_kernel_version = Some(1);

    assert_ne!(baseline, resolve(changed, component_metadata()));
}

#[test]
fn durable_namespace_and_migration_metadata_changes_stable_generation_identity() {
    let baseline = resolve(package(), component_metadata());
    let namespace = ResourceNamespace::parse("fixture.state").unwrap();
    let mut changed = package();
    changed.manifest.resource_namespaces.push(namespace.clone());
    changed.durable_namespaces.insert(namespace.clone());
    changed.migrations.push(DurableMigrationMetadata {
        namespace,
        from_version: 1,
        to_version: 2,
    });

    assert_ne!(baseline, resolve(changed, component_metadata()));
}

#[test]
fn component_lifecycle_metadata_change_is_visible_to_reconciliation() {
    let (active, active_metadata) = resolve_inspectable(package(), component_metadata());
    let mut changed = component_metadata();
    changed.state_class = ComponentStateClass::Durable;
    changed.reload_policy = ReloadPolicy::MigrationRequired;
    let (candidate, candidate_metadata) = resolve_inspectable(package(), changed);
    let reconciler = GraphReconciler::new(active);

    let error = reconciler
        .preview_candidate_with_metadata(&active_metadata, &candidate, &candidate_metadata)
        .unwrap_err();

    assert!(matches!(
        error,
        crate::MetadataReconciliationError::MigrationRequired { component: changed }
            if changed == component()
    ));
}

#[test]
fn package_host_metadata_change_is_visible_to_reconciliation() {
    let (active, active_metadata) = resolve_inspectable(package(), component_metadata());
    let mut changed = package();
    changed.component_hosts = BTreeSet::from([ComponentHostKind::Wasm]);
    changed.reload_policy = ReloadPolicy::DrainAndRestart;
    let (candidate, candidate_metadata) = resolve_inspectable(changed, component_metadata());
    let reconciler = GraphReconciler::new(active);

    let error = reconciler
        .preview_candidate_with_metadata(&active_metadata, &candidate, &candidate_metadata)
        .unwrap_err();

    assert!(matches!(
        error,
        crate::MetadataReconciliationError::DrainRequired { component: changed }
            if changed == component()
    ));
}
