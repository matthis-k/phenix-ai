mod sessions {
    #[phenix_sdk::plugin("fixture.sessions")]
    pub struct Plugin;
}

#[allow(dead_code)]
#[phenix_sdk::plugin("fixture.parent")]
struct Plugin {
    #[phenix(dep)]
    sessions: sessions::Plugin,
}

mod diamond {
    #[phenix_sdk::plugin("fixture.diamond.base")]
    pub struct Base;

    #[allow(dead_code)]
    #[phenix_sdk::plugin("fixture.diamond.left")]
    pub struct Left {
        #[phenix(dep)]
        base: Base,
    }

    #[allow(dead_code)]
    #[phenix_sdk::plugin("fixture.diamond.right")]
    pub struct Right {
        #[phenix(dep)]
        base: Base,
    }

    #[allow(dead_code)]
    #[phenix_sdk::plugin("fixture.diamond.root")]
    pub struct Root {
        #[phenix(dep)]
        left: Left,
        #[phenix(dep)]
        right: Right,
    }
}

#[phenix_sdk::plugin(
    id = "fixture.resource-only",
    execution = phenix_sdk::PluginExecution::ResourceOnly
)]
struct ResourceOnly;

fn external_execution() -> phenix_sdk::PluginExecution {
    phenix_sdk::PluginExecution::Runtime {
        runtime: phenix_sdk::PluginRuntimeId::parse("fixture.runtime").unwrap(),
        artifact: phenix_sdk::PluginArtifact {
            locator: "fixture.wasm".into(),
            revision: phenix_sdk::ArtifactRevision::from_content(b"fixture"),
            configuration: std::collections::BTreeMap::new(),
        },
    }
}

#[phenix_sdk::plugin(
    id = "fixture.external",
    execution = external_execution()
)]
struct External;

struct DuplicateFirst;
struct DuplicateSecond;
struct DuplicateRoot;
struct SameDefinitionV1;
struct SameDefinitionV2;
struct SameDefinitionRoot;
struct SameDefinitionDepsA;
struct SameDefinitionDepsB;
struct SameDefinitionDepsRoot;
struct SameDefinitionFactoryA;
struct SameDefinitionFactoryB;
struct SameDefinitionFactoryRoot;
struct SameFactoryCodeA;
struct SameFactoryCodeB;
struct SameFactoryCodeRoot;
struct SamePayloadA;
struct SamePayloadB;
struct SamePayloadRoot;
struct PermutedDepsA;
struct PermutedDepsB;
struct PermutedDepsRoot;
struct DuplicateReversedRoot;
struct TransitiveLeafA;
struct TransitiveLeafB;
struct TransitiveParentA;
struct TransitiveParentB;
struct TransitiveRoot;
struct CountingLeaf;
struct CountingParent;
struct RepeatedRoot;
struct CycleA;
struct CycleB;
struct ResourceWithEmbeddedFactory;

impl phenix_sdk::StaticPluginDefinition for ResourceWithEmbeddedFactory {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut invalid = descriptor(
            "fixture.invalid-resource-factory",
            "fixture::ResourceFactory",
        );
        invalid.execution = phenix_sdk::PluginExecution::ResourceOnly;
        invalid.embedded_factory = Some(|| panic!("factory must not execute"));
        invalid
    }
}

#[test]
fn resource_only_plugin_rejects_embedded_factory_at_graph_preparation() {
    assert!(matches!(
        phenix_sdk::StaticPluginGraph::compose::<ResourceWithEmbeddedFactory>(),
        Err(phenix_sdk::StaticPluginGraphError::InvalidExecutionFactory { plugin })
            if plugin.as_str() == "fixture.invalid-resource-factory"
    ));
}

fn descriptor(id: &str, definition: &'static str) -> phenix_sdk::StaticPluginDescriptor {
    phenix_sdk::StaticPluginDescriptor {
        id: phenix_sdk::PluginId::parse(id).unwrap(),
        definition,
        version: 1,
        execution: phenix_sdk::__phenix_plugin::PluginExecution::Embedded,
        maximum_authority: phenix_sdk::Authority::default(),
        dependencies: Vec::new(),
        embedded_factory: None,
        contributions: || Ok(Default::default()),
    }
}

#[test]
fn direct_dependency_module_reexports_only_the_declared_plugin_type() {
    fn accepts_dependency(_: plugin::dependencies::sessions::Plugin) {}

    accepts_dependency(sessions::Plugin);
}

#[test]
fn plugin_execution_defaults_to_embedded() {
    let descriptor = <Plugin as phenix_sdk::StaticPluginDefinition>::descriptor();
    assert!(matches!(
        descriptor.execution,
        phenix_sdk::PluginExecution::Embedded
    ));
}

#[test]
fn plugin_execution_preserves_resource_only_and_runtime_metadata() {
    let resource = <ResourceOnly as phenix_sdk::StaticPluginDefinition>::descriptor();
    assert!(matches!(
        resource.execution,
        phenix_sdk::PluginExecution::ResourceOnly
    ));

    let external = <External as phenix_sdk::StaticPluginDefinition>::descriptor();
    let phenix_sdk::PluginExecution::Runtime { runtime, artifact } = external.execution else {
        panic!("external plugin should preserve runtime execution metadata");
    };
    assert_eq!(runtime.as_str(), "fixture.runtime");
    assert_eq!(artifact.locator, "fixture.wasm");
    assert_eq!(
        artifact.revision,
        phenix_sdk::ArtifactRevision::from_content(b"fixture")
    );
}

#[test]
fn concrete_dependency_still_participates_in_recursive_graph_composition() {
    let graph = phenix_sdk::StaticPluginGraph::compose::<Plugin>().unwrap();
    let ids = graph
        .ids()
        .map(phenix_sdk::PluginId::as_str)
        .collect::<Vec<_>>();

    assert_eq!(ids, ["fixture.parent", "fixture.sessions"]);
}

#[test]
fn diamond_dependencies_are_deduplicated_by_plugin_id() {
    let graph = phenix_sdk::StaticPluginGraph::compose::<diamond::Root>().unwrap();
    let ids = graph
        .ids()
        .map(phenix_sdk::PluginId::as_str)
        .collect::<Vec<_>>();

    assert_eq!(
        ids,
        [
            "fixture.diamond.base",
            "fixture.diamond.left",
            "fixture.diamond.right",
            "fixture.diamond.root",
        ]
    );
}

impl phenix_sdk::StaticPluginDefinition for DuplicateFirst {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        descriptor("fixture.duplicate", "fixture::DuplicateFirst")
    }
}

impl phenix_sdk::StaticPluginDefinition for DuplicateSecond {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        descriptor("fixture.duplicate", "fixture::DuplicateSecond")
    }
}

impl phenix_sdk::StaticPluginDefinition for DuplicateRoot {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.duplicate-root", "fixture::DuplicateRoot");
        descriptor.dependencies = vec![
            phenix_sdk::StaticPluginDependency::of::<DuplicateFirst>(),
            phenix_sdk::StaticPluginDependency::of::<DuplicateSecond>(),
        ];
        descriptor
    }
}

#[test]
fn incompatible_duplicate_plugin_ids_are_rejected() {
    let error = match phenix_sdk::StaticPluginGraph::compose::<DuplicateRoot>() {
        Ok(_) => panic!("duplicate plugin identities must be rejected"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        phenix_sdk::StaticPluginGraphError::DuplicateId { ref id, first, second }
            if id.as_str() == "fixture.duplicate"
                && first == "fixture::DuplicateFirst"
                && second == "fixture::DuplicateSecond"
    ));
}

impl phenix_sdk::StaticPluginDefinition for SameDefinitionV1 {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut declaration = descriptor("fixture.same-definition", "fixture::SharedDefinition");
        declaration.execution = phenix_sdk::PluginExecution::ResourceOnly;
        declaration
    }
}

impl phenix_sdk::StaticPluginDefinition for SameDefinitionV2 {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = <SameDefinitionV1 as phenix_sdk::StaticPluginDefinition>::descriptor();
        descriptor.version = 2;
        descriptor
    }
}

impl phenix_sdk::StaticPluginDefinition for SameDefinitionRoot {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor(
            "fixture.same-definition-root",
            "fixture::SameDefinitionRoot",
        );
        descriptor.dependencies = vec![
            phenix_sdk::StaticPluginDependency::of::<SameDefinitionV1>(),
            phenix_sdk::StaticPluginDependency::of::<SameDefinitionV2>(),
        ];
        descriptor
    }
}

#[test]
fn identical_definition_names_cannot_hide_different_plugin_versions() {
    assert!(matches!(
        phenix_sdk::StaticPluginGraph::compose::<SameDefinitionRoot>(),
        Err(phenix_sdk::StaticPluginGraphError::DuplicateId { id, first, second })
            if id.as_str() == "fixture.same-definition"
                && first == "fixture::SharedDefinition"
                && second == "fixture::SharedDefinition"
    ));
}

impl phenix_sdk::StaticPluginDefinition for SameDefinitionDepsA {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.same-deps", "fixture::SameDeps");
        descriptor.execution = phenix_sdk::PluginExecution::ResourceOnly;
        descriptor.dependencies = vec![phenix_sdk::StaticPluginDependency::of::<diamond::Base>()];
        descriptor
    }
}

impl phenix_sdk::StaticPluginDefinition for SameDefinitionDepsB {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.same-deps", "fixture::SameDeps");
        descriptor.execution = phenix_sdk::PluginExecution::ResourceOnly;
        descriptor.dependencies =
            vec![phenix_sdk::StaticPluginDependency::of::<sessions::Plugin>()];
        descriptor
    }
}

impl phenix_sdk::StaticPluginDefinition for SameDefinitionDepsRoot {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.same-deps-root", "fixture::SameDepsRoot");
        descriptor.dependencies = vec![
            phenix_sdk::StaticPluginDependency::of::<SameDefinitionDepsA>(),
            phenix_sdk::StaticPluginDependency::of::<SameDefinitionDepsB>(),
        ];
        descriptor
    }
}

#[test]
fn identical_definition_names_cannot_hide_different_dependency_closures() {
    assert!(matches!(
        phenix_sdk::StaticPluginGraph::compose::<SameDefinitionDepsRoot>(),
        Err(phenix_sdk::StaticPluginGraphError::DuplicateId { id, .. })
            if id.as_str() == "fixture.same-deps"
    ));
}

struct FactoryInstance;

impl phenix_sdk::__phenix_plugin::PluginInstance for FactoryInstance {
    fn start(&mut self, _host: &phenix_sdk::__phenix_plugin::PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }
}

fn factory_a() -> Box<dyn phenix_sdk::__phenix_plugin::PluginInstance> {
    Box::new(FactoryInstance)
}

impl phenix_sdk::StaticPluginDefinition for SameDefinitionFactoryA {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.same-factory", "fixture::SameFactory");
        descriptor.embedded_factory = Some(factory_a);
        descriptor
    }
}

impl phenix_sdk::StaticPluginDefinition for SameDefinitionFactoryB {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.same-factory", "fixture::SameFactory");
        descriptor.embedded_factory = None;
        descriptor
    }
}

impl phenix_sdk::StaticPluginDefinition for SameDefinitionFactoryRoot {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.same-factory-root", "fixture::SameFactoryRoot");
        descriptor.dependencies = vec![
            phenix_sdk::StaticPluginDependency::of::<SameDefinitionFactoryA>(),
            phenix_sdk::StaticPluginDependency::of::<SameDefinitionFactoryB>(),
        ];
        descriptor
    }
}

#[test]
fn identical_definition_names_cannot_hide_different_factory_availability() {
    assert!(matches!(
        phenix_sdk::StaticPluginGraph::compose::<SameDefinitionFactoryRoot>(),
        Err(phenix_sdk::StaticPluginGraphError::DuplicateId { id, .. })
            if id.as_str() == "fixture.same-factory"
    ));
}

fn factory_b() -> Box<dyn phenix_sdk::__phenix_plugin::PluginInstance> {
    Box::new(FactoryInstance)
}

impl phenix_sdk::StaticPluginDefinition for SameFactoryCodeA {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.code-collision", "fixture::SharedExecutable");
        descriptor.embedded_factory = Some(factory_a);
        descriptor
    }
}

impl phenix_sdk::StaticPluginDefinition for SameFactoryCodeB {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.code-collision", "fixture::SharedExecutable");
        descriptor.embedded_factory = Some(factory_b);
        descriptor
    }
}

impl phenix_sdk::StaticPluginDefinition for SameFactoryCodeRoot {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.code-collision-root", "fixture::CodeRoot");
        descriptor.dependencies = vec![
            phenix_sdk::StaticPluginDependency::of::<SameFactoryCodeA>(),
            phenix_sdk::StaticPluginDependency::of::<SameFactoryCodeB>(),
        ];
        descriptor
    }
}

#[test]
fn identical_descriptors_cannot_hide_distinct_embedded_factories() {
    assert!(matches!(
        phenix_sdk::StaticPluginGraph::compose::<SameFactoryCodeRoot>(),
        Err(phenix_sdk::StaticPluginGraphError::DuplicateId { id, .. })
            if id.as_str() == "fixture.code-collision"
    ));
}

// An embedded plugin can receive executable state after composition even
// when its static descriptor has no zero-input factory. Identical declarative
// bytes therefore cannot make two Rust origins for it interchangeable.
struct StatefulAliasA;
struct StatefulAliasB;
struct StatefulAliasRoot;

impl phenix_sdk::StaticPluginDefinition for StatefulAliasA {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        descriptor("fixture.stateful-alias", "fixture::StatefulAlias")
    }
}

impl phenix_sdk::StaticPluginDefinition for StatefulAliasB {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        descriptor("fixture.stateful-alias", "fixture::StatefulAlias")
    }
}

impl phenix_sdk::StaticPluginDefinition for StatefulAliasRoot {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor =
            descriptor("fixture.stateful-alias-root", "fixture::StatefulAliasRoot");
        descriptor.dependencies = vec![
            phenix_sdk::StaticPluginDependency::of::<StatefulAliasA>(),
            phenix_sdk::StaticPluginDependency::of::<StatefulAliasB>(),
        ];
        descriptor
    }
}

#[test]
fn identical_embedded_declarations_without_factories_reject_distinct_rust_origins() {
    assert!(matches!(
        phenix_sdk::StaticPluginGraph::compose::<StatefulAliasRoot>(),
        Err(phenix_sdk::StaticPluginGraphError::DuplicateId { id, .. })
            if id.as_str() == "fixture.stateful-alias"
    ));
}

static SNAPSHOT_ALIAS_A_READS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
static SNAPSHOT_ALIAS_B_READS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

struct SnapshotAliasA;
struct SnapshotAliasB;
struct SnapshotAliasRoot;

fn snapshot_alias_contribution() -> Result<phenix_sdk::ContributionSet, String> {
    phenix_sdk::ContributionSet::collect([phenix_sdk::Contribution {
        owner: phenix_sdk::PluginId::parse("fixture.snapshot-alias").unwrap(),
        id: phenix_sdk::ContractId::parse("fixture.snapshot-alias.note@1").unwrap(),
        kind: phenix_sdk::ContractId::parse("fixture.note@1").unwrap(),
        role: phenix_sdk::ContributionRole::Declare,
        payload: phenix_sdk::PhenixValue::Unit,
    }])
    .map_err(|error| error.to_string())
}

fn snapshot_alias_a() -> Result<phenix_sdk::ContributionSet, String> {
    SNAPSHOT_ALIAS_A_READS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    snapshot_alias_contribution()
}

fn snapshot_alias_b() -> Result<phenix_sdk::ContributionSet, String> {
    SNAPSHOT_ALIAS_B_READS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    snapshot_alias_contribution()
}

impl phenix_sdk::StaticPluginDefinition for SnapshotAliasA {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.snapshot-alias", "fixture::SnapshotAlias");
        descriptor.execution = phenix_sdk::PluginExecution::ResourceOnly;
        descriptor.contributions = snapshot_alias_a;
        descriptor
    }
}

impl phenix_sdk::StaticPluginDefinition for SnapshotAliasB {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.snapshot-alias", "fixture::SnapshotAlias");
        descriptor.execution = phenix_sdk::PluginExecution::ResourceOnly;
        descriptor.contributions = snapshot_alias_b;
        descriptor
    }
}

impl phenix_sdk::StaticPluginDefinition for SnapshotAliasRoot {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor =
            descriptor("fixture.snapshot-alias-root", "fixture::SnapshotAliasRoot");
        descriptor.dependencies = vec![
            phenix_sdk::StaticPluginDependency::of::<SnapshotAliasA>(),
            phenix_sdk::StaticPluginDependency::of::<SnapshotAliasB>(),
        ];
        descriptor
    }
}

#[test]
fn equivalent_duplicate_contributors_are_each_evaluated_once() {
    use std::sync::atomic::Ordering;

    let a = SNAPSHOT_ALIAS_A_READS.load(Ordering::SeqCst);
    let b = SNAPSHOT_ALIAS_B_READS.load(Ordering::SeqCst);
    let graph = phenix_sdk::StaticPluginGraph::compose::<SnapshotAliasRoot>().unwrap();
    assert_eq!(SNAPSHOT_ALIAS_A_READS.load(Ordering::SeqCst) - a, 1);
    assert_eq!(SNAPSHOT_ALIAS_B_READS.load(Ordering::SeqCst) - b, 1);
    let first = graph.contributions().unwrap().canonical_bytes().unwrap();
    assert_eq!(
        first,
        graph.contributions().unwrap().canonical_bytes().unwrap()
    );
    assert_eq!(SNAPSHOT_ALIAS_A_READS.load(Ordering::SeqCst) - a, 1);
    assert_eq!(SNAPSHOT_ALIAS_B_READS.load(Ordering::SeqCst) - b, 1);
    assert_eq!(graph.contributions().unwrap().len(), 1);
}

static DEPENDENCY_DESCRIPTOR_READS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

struct SnapshotDependencyLeaf;
struct SnapshotDependencyA;
struct SnapshotDependencyB;
struct SnapshotDependencyRoot;

impl phenix_sdk::StaticPluginDefinition for SnapshotDependencyLeaf {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        DEPENDENCY_DESCRIPTOR_READS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut leaf = descriptor("fixture.snapshot-dependency.leaf", "fixture::SnapshotLeaf");
        leaf.execution = phenix_sdk::PluginExecution::ResourceOnly;
        leaf
    }
}

impl phenix_sdk::StaticPluginDefinition for SnapshotDependencyA {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut item = descriptor("fixture.snapshot-dependency", "fixture::SnapshotParent");
        item.execution = phenix_sdk::PluginExecution::ResourceOnly;
        item.dependencies = vec![phenix_sdk::StaticPluginDependency::of::<
            SnapshotDependencyLeaf,
        >()];
        item
    }
}

impl phenix_sdk::StaticPluginDefinition for SnapshotDependencyB {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        <SnapshotDependencyA as phenix_sdk::StaticPluginDefinition>::descriptor()
    }
}

impl phenix_sdk::StaticPluginDefinition for SnapshotDependencyRoot {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut item = descriptor("fixture.snapshot-dependency.root", "fixture::SnapshotRoot");
        item.dependencies = vec![
            phenix_sdk::StaticPluginDependency::of::<SnapshotDependencyA>(),
            phenix_sdk::StaticPluginDependency::of::<SnapshotDependencyB>(),
        ];
        item
    }
}

#[test]
fn duplicate_dependency_checks_reuse_frozen_descriptor_identity() {
    use std::sync::atomic::Ordering;

    let before = DEPENDENCY_DESCRIPTOR_READS.load(Ordering::SeqCst);
    let graph = phenix_sdk::StaticPluginGraph::compose::<SnapshotDependencyRoot>().unwrap();
    assert_eq!(
        DEPENDENCY_DESCRIPTOR_READS.load(Ordering::SeqCst) - before,
        1
    );
    let ids = graph
        .ids()
        .map(phenix_sdk::PluginId::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        [
            "fixture.snapshot-dependency",
            "fixture.snapshot-dependency.leaf",
            "fixture.snapshot-dependency.root",
        ]
    );
    assert_eq!(
        DEPENDENCY_DESCRIPTOR_READS.load(Ordering::SeqCst) - before,
        1
    );
}

fn note_contributions(value: &str) -> Result<phenix_sdk::ContributionSet, String> {
    let item = phenix_sdk::Contribution {
        owner: phenix_sdk::PluginId::parse("fixture.same-payload").unwrap(),
        id: phenix_sdk::ContractId::parse("fixture.same-payload.note@1").unwrap(),
        kind: phenix_sdk::ContractId::parse("fixture.note@1").unwrap(),
        role: phenix_sdk::ContributionRole::Provide,
        payload: phenix_sdk::PhenixValue::String(value.into()),
    };
    phenix_sdk::ContributionSet::collect([item]).map_err(|error| error.to_string())
}

fn contributions_a() -> Result<phenix_sdk::ContributionSet, String> {
    note_contributions("first")
}

fn contributions_b() -> Result<phenix_sdk::ContributionSet, String> {
    note_contributions("second")
}

impl phenix_sdk::StaticPluginDefinition for SamePayloadA {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.same-payload", "fixture::SamePayload");
        descriptor.execution = phenix_sdk::PluginExecution::ResourceOnly;
        descriptor.contributions = contributions_a;
        descriptor
    }
}

impl phenix_sdk::StaticPluginDefinition for SamePayloadB {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.same-payload", "fixture::SamePayload");
        descriptor.execution = phenix_sdk::PluginExecution::ResourceOnly;
        descriptor.contributions = contributions_b;
        descriptor
    }
}

impl phenix_sdk::StaticPluginDefinition for SamePayloadRoot {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.same-payload-root", "fixture::SamePayloadRoot");
        descriptor.dependencies = vec![
            phenix_sdk::StaticPluginDependency::of::<SamePayloadA>(),
            phenix_sdk::StaticPluginDependency::of::<SamePayloadB>(),
        ];
        descriptor
    }
}

#[test]
fn matching_plugin_metadata_cannot_hide_different_canonical_contributions() {
    assert!(matches!(
        phenix_sdk::StaticPluginGraph::compose::<SamePayloadRoot>(),
        Err(phenix_sdk::StaticPluginGraphError::DuplicateId { id, .. })
            if id.as_str() == "fixture.same-payload"
    ));
}

impl phenix_sdk::StaticPluginDefinition for PermutedDepsA {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.permuted-deps", "fixture::PermutedDeps");
        descriptor.execution = phenix_sdk::PluginExecution::ResourceOnly;
        descriptor.dependencies = vec![
            phenix_sdk::StaticPluginDependency::of::<sessions::Plugin>(),
            phenix_sdk::StaticPluginDependency::of::<diamond::Base>(),
        ];
        descriptor
    }
}

impl phenix_sdk::StaticPluginDefinition for PermutedDepsB {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.permuted-deps", "fixture::PermutedDeps");
        descriptor.execution = phenix_sdk::PluginExecution::ResourceOnly;
        descriptor.dependencies = vec![
            phenix_sdk::StaticPluginDependency::of::<diamond::Base>(),
            phenix_sdk::StaticPluginDependency::of::<sessions::Plugin>(),
        ];
        descriptor
    }
}

impl phenix_sdk::StaticPluginDefinition for PermutedDepsRoot {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.permuted-deps-root", "fixture::PermutedDepsRoot");
        descriptor.dependencies = vec![
            phenix_sdk::StaticPluginDependency::of::<PermutedDepsA>(),
            phenix_sdk::StaticPluginDependency::of::<PermutedDepsB>(),
        ];
        descriptor
    }
}

#[test]
fn matching_plugin_dependency_sets_are_unordered() {
    let graph = phenix_sdk::StaticPluginGraph::compose::<PermutedDepsRoot>().unwrap();
    assert_eq!(graph.len(), 4);
    assert!(
        graph
            .descriptor(&phenix_sdk::PluginId::parse("fixture.permuted-deps").unwrap())
            .is_some()
    );
}

impl phenix_sdk::StaticPluginDefinition for DuplicateReversedRoot {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.duplicate-reverse", "fixture::DuplicateReverse");
        descriptor.dependencies = vec![
            phenix_sdk::StaticPluginDependency::of::<DuplicateSecond>(),
            phenix_sdk::StaticPluginDependency::of::<DuplicateFirst>(),
        ];
        descriptor
    }
}

#[test]
fn duplicate_plugin_errors_have_stable_provenance_across_dependency_order() {
    let first = phenix_sdk::StaticPluginGraph::compose::<DuplicateRoot>()
        .err()
        .unwrap();
    let reversed = phenix_sdk::StaticPluginGraph::compose::<DuplicateReversedRoot>()
        .err()
        .unwrap();
    assert_eq!(first, reversed);
}

impl phenix_sdk::StaticPluginDefinition for TransitiveLeafA {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        descriptor("fixture.transitive-leaf", "fixture::TransitiveLeaf")
    }
}

impl phenix_sdk::StaticPluginDefinition for TransitiveLeafB {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut declaration = descriptor("fixture.transitive-leaf", "fixture::TransitiveLeaf");
        declaration.version = 2;
        declaration
    }
}

impl phenix_sdk::StaticPluginDefinition for TransitiveParentA {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut declaration = descriptor("fixture.transitive-parent", "fixture::TransitiveParent");
        declaration.execution = phenix_sdk::PluginExecution::ResourceOnly;
        declaration.dependencies =
            vec![phenix_sdk::StaticPluginDependency::of::<TransitiveLeafA>()];
        declaration
    }
}

impl phenix_sdk::StaticPluginDefinition for TransitiveParentB {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut declaration = descriptor("fixture.transitive-parent", "fixture::TransitiveParent");
        declaration.execution = phenix_sdk::PluginExecution::ResourceOnly;
        declaration.dependencies =
            vec![phenix_sdk::StaticPluginDependency::of::<TransitiveLeafB>()];
        declaration
    }
}

impl phenix_sdk::StaticPluginDefinition for TransitiveRoot {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut declaration = descriptor("fixture.transitive-root", "fixture::TransitiveRoot");
        declaration.dependencies = vec![
            phenix_sdk::StaticPluginDependency::of::<TransitiveParentA>(),
            phenix_sdk::StaticPluginDependency::of::<TransitiveParentB>(),
        ];
        declaration
    }
}

#[test]
fn repeated_parent_ids_do_not_hide_conflicting_transitive_dependencies() {
    let error = match phenix_sdk::StaticPluginGraph::compose::<TransitiveRoot>() {
        Ok(_) => panic!("the transitive leaf's mismatched version must be rejected"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        phenix_sdk::StaticPluginGraphError::DuplicateId { id, .. }
            if id.as_str() == "fixture.transitive-leaf"
    ));
}

static COUNTING_LEAF_VISITS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

impl phenix_sdk::StaticPluginDefinition for CountingLeaf {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        COUNTING_LEAF_VISITS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        descriptor("fixture.counting-leaf", "fixture::CountingLeaf")
    }
}

impl phenix_sdk::StaticPluginDefinition for CountingParent {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut declaration = descriptor("fixture.counting-parent", "fixture::CountingParent");
        declaration.dependencies = vec![phenix_sdk::StaticPluginDependency::of::<CountingLeaf>()];
        declaration
    }
}

impl phenix_sdk::StaticPluginDefinition for RepeatedRoot {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut declaration = descriptor("fixture.repeated-root", "fixture::RepeatedRoot");
        declaration.dependencies = (0..128)
            .map(|_| phenix_sdk::StaticPluginDependency::of::<CountingParent>())
            .collect();
        declaration
    }
}

#[test]
fn repeated_static_origin_traverses_transitive_closure_once() {
    let before = COUNTING_LEAF_VISITS.load(std::sync::atomic::Ordering::SeqCst);
    let graph = phenix_sdk::StaticPluginGraph::compose::<RepeatedRoot>().unwrap();
    let after = COUNTING_LEAF_VISITS.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(graph.len(), 3);
    assert_eq!(
        after - before,
        1,
        "transitive validation should be memoized"
    );
}

impl phenix_sdk::StaticPluginDefinition for CycleA {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.cycle-a", "fixture::CycleA");
        descriptor.dependencies = vec![phenix_sdk::StaticPluginDependency::of::<CycleB>()];
        descriptor
    }
}

impl phenix_sdk::StaticPluginDefinition for CycleB {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.cycle-b", "fixture::CycleB");
        descriptor.dependencies = vec![phenix_sdk::StaticPluginDependency::of::<CycleA>()];
        descriptor
    }
}

#[test]
fn dependency_cycles_are_rejected() {
    let error = match phenix_sdk::StaticPluginGraph::compose::<CycleA>() {
        Ok(_) => panic!("dependency cycles must be rejected"),
        Err(error) => error,
    };

    let phenix_sdk::StaticPluginGraphError::Cycle { ref path } = error else {
        panic!("expected dependency cycle error");
    };
    assert_eq!(
        path.iter()
            .map(phenix_sdk::PluginId::as_str)
            .collect::<Vec<_>>(),
        ["fixture.cycle-a", "fixture.cycle-b", "fixture.cycle-a"]
    );
    assert_eq!(
        error.to_string(),
        "static plugin dependency cycle: fixture.cycle-a -> fixture.cycle-b -> fixture.cycle-a"
    );
}
