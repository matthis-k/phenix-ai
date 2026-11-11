//! Frozen obligations must be stable under builder round-trips and replay.
//! Deduplication cannot erase a conflicting definition of the same semantic ID.
use phenix_graph::{
    GraphOrigin, Interface, PortDef,
    component::ComponentDef,
    compose::{Composer, Edit, Footprint, NetworkPatch, Registration},
    contract::Contract,
    network::{NetworkBuilder, NetworkError},
    process::{Process, ProcessContext, Step},
};
use std::sync::Arc;

struct Noop;
impl Process for Noop {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        Ok(Step::Finished)
    }
}

fn claim() -> Contract {
    Contract::new("fbp/obligation-stable", Arc::new(Interface::default())).unwrap()
}

fn leaf(contract: &Contract) -> Arc<ComponentDef> {
    Arc::new(
        ComponentDef::primitive(Arc::new(Interface::default()), Arc::new(|| Noop))
            .claim(contract)
            .unwrap(),
    )
}

#[test]
fn cloned_and_rebuilt_networks_never_accumulate_frozen_obligations() {
    let contract = claim();
    let mut child = NetworkBuilder::new();
    child.add("leaf", leaf(&contract)).unwrap();
    child.require_contract("leaf", &contract).unwrap();
    let child = child.build().unwrap();
    assert_eq!(child.requirements().len(), 1);

    let mut root = NetworkBuilder::new();
    root.module("child", child).unwrap();
    root.add("peer", leaf(&contract)).unwrap();
    root.require_contract("child/leaf", &contract).unwrap();
    let mut network = root.build().unwrap();
    let expected = vec![
        ("child/leaf".to_owned(), contract.clone()),
        ("peer".to_owned(), contract.clone()),
    ];
    assert_eq!(network.requirements(), expected);

    for _ in 0..32 {
        network = NetworkBuilder::from_def(&network).build().unwrap();
        assert_eq!(network.requirements(), expected);
        network.compile().unwrap();
    }
}

#[test]
fn replayed_add_then_replace_retains_one_identical_obligation() {
    let contract = claim();
    let mut composer = Composer::new(NetworkBuilder::new().build().unwrap()).unwrap();
    let origin = GraphOrigin::default();
    composer
        .register([Registration::new(
            "plugin",
            [],
            0,
            origin,
            [Footprint::Component("worker".into())],
            NetworkPatch::new([
                Edit::Add {
                    path: "worker".into(),
                    definition: leaf(&contract),
                },
                Edit::Replace {
                    path: "worker".into(),
                    definition: leaf(&contract),
                },
            ]),
        )])
        .unwrap();
    assert_eq!(
        composer.current().network().requirements(),
        [("worker".to_owned(), contract.clone())]
    );
    composer.revoke("plugin").unwrap();
    assert!(composer.current().network().requirements().is_empty());
}

#[test]
fn same_id_with_incompatible_shape_is_rejected_before_deduplication() {
    let valid = claim();
    let wrong = Contract::new(
        valid.id(),
        Arc::new(Interface::new([PortDef::local::<i64>("required")], []).unwrap()),
    )
    .unwrap();
    let mut builder = NetworkBuilder::new();
    builder.require_contract("worker", &wrong).unwrap();
    builder.add("worker", leaf(&valid)).unwrap();

    assert!(matches!(builder.build(), Err(NetworkError::Interface(_))));
}
