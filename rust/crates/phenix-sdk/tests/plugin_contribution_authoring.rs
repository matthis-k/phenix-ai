//! Static module declarations lower to inert contribution data.

use phenix_sdk::{
    ContractId, Contribution, ContributionRole, PhenixValue, PluginId,
    StaticContributionDefinition, StaticPluginDefinition, StaticRawContribution,
};

struct TypedNote {
    text: &'static str,
}

impl StaticContributionDefinition for TypedNote {
    fn contribution(&self, owner: &PluginId) -> Result<Contribution, String> {
        Ok(Contribution {
            owner: owner.clone(),
            id: ContractId::parse("fixture.declarations.typed@1").unwrap(),
            kind: ContractId::parse("fixture.note@1").unwrap(),
            role: ContributionRole::Provide,
            payload: PhenixValue::String(self.text.into()),
        })
    }
}

#[phenix_sdk::plugin("fixture.declarations")]
mod plugin {
    use super::*;

    #[phenix(contribute)]
    const EXAMPLE: StaticRawContribution = StaticRawContribution {
        id: "fixture.declarations.raw@1",
        kind: "fixture.note@1",
        role: ContributionRole::Declare,
        payload_json: r#"{"type":"string","value":"static"}"#,
    };

    #[phenix(contribute)]
    const CUSTOM: TypedNote = TypedNote { text: "typed" };
}

#[test]
fn annotated_consts_produce_ordered_portable_descriptors() {
    let declared = <plugin::Plugin as StaticPluginDefinition>::contributions().unwrap();
    assert_eq!(declared.len(), 2);

    let raw = declared
        .get(&ContractId::parse("fixture.declarations.raw@1").unwrap())
        .unwrap();
    assert_eq!(raw.owner, PluginId::parse("fixture.declarations").unwrap());
    assert_eq!(raw.role, ContributionRole::Declare);
    assert_eq!(raw.payload, PhenixValue::String("static".into()));

    let typed = declared
        .get(&ContractId::parse("fixture.declarations.typed@1").unwrap())
        .unwrap();
    assert_eq!(typed.role, ContributionRole::Provide);
    assert_eq!(typed.payload, PhenixValue::String("typed".into()));

    let round_trip: phenix_sdk::ContributionSet =
        serde_json::from_slice(&declared.canonical_bytes().unwrap()).unwrap();
    assert_eq!(round_trip, declared);
}

#[phenix_sdk::plugin("fixture.no-declarations")]
mod plain {
    #[phenix(value("fixture.ping@1"), public)]
    #[allow(dead_code)]
    fn ping() -> u64 {
        1
    }
}

#[test]
fn portable_declaration_matches_rust_authored_bytes() {
    let authored = <plugin::Plugin as StaticPluginDefinition>::contributions().unwrap();
    let raw = authored
        .get(&ContractId::parse("fixture.declarations.raw@1").unwrap())
        .unwrap()
        .clone();
    let rust = phenix_sdk::ContributionSet::collect([raw]).unwrap();
    let portable: phenix_sdk::ContributionSet = serde_json::from_str(
        r#"[{
            "owner": "fixture.declarations",
            "id": "fixture.declarations.raw@1",
            "kind": "fixture.note@1",
            "role": "declare",
            "payload": {"type": "string", "value": "static"}
        }]"#,
    )
    .unwrap();
    assert_eq!(
        rust.canonical_bytes().unwrap(),
        portable.canonical_bytes().unwrap()
    );
}

#[test]
fn existing_stateless_plugin_remains_valid_without_contributions() {
    let contributions = <plain::Plugin as StaticPluginDefinition>::contributions().unwrap();
    assert!(contributions.is_empty());
}

#[allow(dead_code)]
#[phenix_sdk::plugin("fixture.composed-parent")]
struct ComposedParent {
    #[phenix(dep)]
    declarations: plugin::Plugin,
}

#[test]
fn static_graph_collects_contributions_from_transitive_dependencies() {
    let graph = phenix_sdk::StaticPluginGraph::compose::<ComposedParent>().unwrap();
    let contributions = graph.contributions().unwrap();
    assert_eq!(contributions.len(), 2);
    let typed = contributions
        .get(&ContractId::parse("fixture.declarations.typed@1").unwrap())
        .unwrap();
    assert_eq!(
        typed.owner,
        PluginId::parse("fixture.declarations").unwrap()
    );
}

mod other_owner {
    use super::*;

    #[phenix_sdk::plugin("fixture.other-owner")]
    mod plugin {
        use super::*;

        #[phenix(contribute)]
        const DUPLICATE: StaticRawContribution = StaticRawContribution {
            id: "fixture.declarations.raw@1",
            kind: "fixture.note@1",
            role: ContributionRole::Declare,
            payload_json: r#"{"type":"string","value":"another"}"#,
        };
    }

    pub type Plugin = plugin::Plugin;
}

#[allow(dead_code)]
#[phenix_sdk::plugin("fixture.conflicting-parent")]
struct ConflictingParent {
    #[phenix(dep)]
    first: plugin::Plugin,
    #[phenix(dep)]
    second: other_owner::Plugin,
}

#[test]
fn static_graph_rejects_cross_plugin_contribution_id_conflicts_before_activation() {
    assert!(matches!(
        phenix_sdk::StaticPluginGraph::compose::<ConflictingParent>(),
        Err(phenix_sdk::StaticPluginGraphError::ContributionConflict(
            phenix_sdk::ContributionSetError::DuplicateIdentity { .. }
        ))
    ));
}

#[phenix_sdk::plugin(
    id = "fixture.data-only",
    execution = phenix_sdk::PluginExecution::ResourceOnly
)]
mod data_only {
    use super::*;

    #[phenix(contribute)]
    const VALUE: StaticRawContribution = StaticRawContribution {
        id: "fixture.data-only.config@1",
        kind: "fixture.note@1",
        role: ContributionRole::Declare,
        payload_json: r#"{"type":"string","value":"resource"}"#,
    };
}

#[test]
fn resource_only_module_carries_declarations_without_embedded_handlers() {
    let descriptor = <data_only::Plugin as StaticPluginDefinition>::descriptor();
    assert!(matches!(
        descriptor.execution,
        phenix_sdk::PluginExecution::ResourceOnly
    ));
    assert!(descriptor.embedded_factory.is_none());
    let components = <data_only::Plugin as phenix_sdk::StaticPluginComponents>::components();
    assert!(components.is_empty());

    let manifest = <data_only::Plugin as StaticPluginDefinition>::manifest();
    assert!(matches!(
        manifest.execution,
        phenix_sdk::PluginExecution::ResourceOnly
    ));
    let graph = phenix_sdk::StaticPluginGraph::compose::<data_only::Plugin>().unwrap();
    assert_eq!(graph.contributions().unwrap().len(), 1);

    let declarations = <data_only::Plugin as StaticPluginDefinition>::contributions().unwrap();
    assert_eq!(declarations.len(), 1);
    assert_eq!(
        declarations.iter().next().unwrap().payload,
        PhenixValue::String("resource".into())
    );
}

const STRUCT_DECL: StaticRawContribution = StaticRawContribution {
    id: "fixture.struct-contributions.extra@1",
    kind: "fixture.note@1",
    role: ContributionRole::Provide,
    payload_json: r#"{"type":"string","value":"struct-field"}"#,
};

#[allow(dead_code)]
#[phenix_sdk::plugin("fixture.struct-contributions")]
struct StructContributions {
    #[phenix(dep)]
    dependency: plugin::Plugin,
    #[phenix(contribute(value = STRUCT_DECL))]
    config: StaticRawContribution,
}

#[test]
fn annotated_struct_field_matches_portable_contribution_bytes() {
    let authored = phenix_sdk::StaticPluginGraph::compose::<StructContributions>()
        .unwrap()
        .contributions()
        .unwrap();
    let field = authored
        .get(&ContractId::parse("fixture.struct-contributions.extra@1").unwrap())
        .unwrap()
        .clone();
    let rust = phenix_sdk::ContributionSet::collect([field]).unwrap();
    let portable: phenix_sdk::ContributionSet = serde_json::from_str(
        r#"[{
            "owner": "fixture.struct-contributions",
            "id": "fixture.struct-contributions.extra@1",
            "kind": "fixture.note@1",
            "role": "provide",
            "payload": {"type": "string", "value": "struct-field"}
        }]"#,
    )
    .unwrap();
    assert_eq!(
        rust.canonical_bytes().unwrap(),
        portable.canonical_bytes().unwrap()
    );
}

#[test]
fn portable_selected_plugin_envelopes_match_typed_dependency_graph() {
    // These independent JSON artifacts need only a manifest-verified owner,
    // not a Rust plugin definition or procedural macro.
    let module = br#"[
        {
            "owner":"fixture.declarations",
            "id":"fixture.declarations.raw@1",
            "kind":"fixture.note@1",
            "role":"declare",
            "payload":{"type":"string","value":"static"}
        },
        {
            "owner":"fixture.declarations",
            "id":"fixture.declarations.typed@1",
            "kind":"fixture.note@1",
            "role":"provide",
            "payload":{"type":"string","value":"typed"}
        }
    ]"#;
    let parent = br#"[
        {
            "owner":"fixture.struct-contributions",
            "id":"fixture.struct-contributions.extra@1",
            "kind":"fixture.note@1",
            "role":"provide",
            "payload":{"type":"string","value":"struct-field"}
        }
    ]"#;
    let module_owner = PluginId::parse("fixture.declarations").unwrap();
    let parent_owner = PluginId::parse("fixture.struct-contributions").unwrap();
    let portable = phenix_sdk::ContributionSet::decode_selected([
        (&module_owner, module.as_slice()),
        (&parent_owner, parent.as_slice()),
    ])
    .unwrap();
    let rust = phenix_sdk::StaticPluginGraph::compose::<StructContributions>()
        .unwrap()
        .contributions()
        .unwrap();
    assert_eq!(portable.len(), 3);
    assert_eq!(
        portable.canonical_bytes().unwrap(),
        rust.canonical_bytes().unwrap()
    );

    assert!(matches!(
        phenix_sdk::ContributionSet::decode_selected([(&parent_owner, module.as_slice())]),
        Err(phenix_sdk::ContributionSetError::OwnerMismatch { .. })
    ));
}

#[test]
fn static_struct_field_contribution_uses_typed_const_and_closure() {
    let graph = phenix_sdk::StaticPluginGraph::compose::<StructContributions>().unwrap();
    let declarations = graph.contributions().unwrap();
    assert_eq!(declarations.len(), 3);
    let own = declarations
        .get(&ContractId::parse("fixture.struct-contributions.extra@1").unwrap())
        .unwrap();
    assert_eq!(own.owner.as_str(), "fixture.struct-contributions");
    assert_eq!(own.role, ContributionRole::Provide);
    assert_eq!(own.payload, PhenixValue::String("struct-field".into()));
}

struct SnapshotPlugin;
struct ForgedPlugin;

static SNAPSHOT_READS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn snapshot_contributions() -> Result<phenix_sdk::ContributionSet, String> {
    let count = SNAPSHOT_READS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    phenix_sdk::ContributionSet::collect([Contribution {
        owner: PluginId::parse("fixture.snapshot").unwrap(),
        id: ContractId::parse("fixture.snapshot.record@1").unwrap(),
        kind: ContractId::parse("fixture.note@1").unwrap(),
        role: ContributionRole::Provide,
        payload: PhenixValue::String(count.to_string()),
    }])
    .map_err(|error| error.to_string())
}

fn forged_contributions() -> Result<phenix_sdk::ContributionSet, String> {
    phenix_sdk::ContributionSet::collect([Contribution {
        owner: PluginId::parse("fixture.not-the-owner").unwrap(),
        id: ContractId::parse("fixture.forged.record@1").unwrap(),
        kind: ContractId::parse("fixture.note@1").unwrap(),
        role: ContributionRole::Provide,
        payload: PhenixValue::Unit,
    }])
    .map_err(|error| error.to_string())
}

fn contributed_descriptor(
    id: &str,
    contributions: fn() -> Result<phenix_sdk::ContributionSet, String>,
) -> phenix_sdk::StaticPluginDescriptor {
    phenix_sdk::StaticPluginDescriptor {
        id: PluginId::parse(id).unwrap(),
        definition: "fixture::StaticDeclaration",
        version: 1,
        execution: phenix_sdk::PluginExecution::ResourceOnly,
        maximum_authority: phenix_sdk::Authority::default(),
        dependencies: Vec::new(),
        embedded_factory: None,
        contributions,
    }
}

impl StaticPluginDefinition for SnapshotPlugin {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        contributed_descriptor("fixture.snapshot", snapshot_contributions)
    }
}

impl StaticPluginDefinition for ForgedPlugin {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        contributed_descriptor("fixture.forged", forged_contributions)
    }
}

#[test]
fn graph_composition_snapshots_contributions_only_once() {
    let before = SNAPSHOT_READS.load(std::sync::atomic::Ordering::SeqCst);
    let graph = phenix_sdk::StaticPluginGraph::compose::<SnapshotPlugin>().unwrap();
    let after = SNAPSHOT_READS.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(after - before, 1);

    let first = graph.contributions().unwrap().canonical_bytes().unwrap();
    let second = graph.contributions().unwrap().canonical_bytes().unwrap();
    assert_eq!(first, second);
    assert_eq!(
        SNAPSHOT_READS.load(std::sync::atomic::Ordering::SeqCst),
        after,
        "a later read must not re-evaluate plugin author code"
    );
}

#[test]
fn graph_composition_rejects_forged_contribution_owners() {
    assert!(matches!(
        phenix_sdk::StaticPluginGraph::compose::<ForgedPlugin>(),
        Err(phenix_sdk::StaticPluginGraphError::InvalidContributions { plugin, .. })
            if plugin.as_str() == "fixture.forged"
    ));
}
