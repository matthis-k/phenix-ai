mod support;
use phenix_graph::{
    Interface, PortDef,
    component::ComponentDef,
    compose::{Edit, NetworkPatch},
    contract::Contract,
    process::LocalProcessDispatcher,
};
use phenix_kernel::{ContractDefinition, ContractRequirement, Kernel, Key, NetworkOffer};
use phenix_plugin::{Failure, PluginInstance, Registrar};
use std::sync::Arc;

#[derive(Clone, Copy)]
enum Case {
    DifferentAdvertisedContract,
    EquivalentDefinitions,
    UnofferedNetworkConflict,
}
struct Fixture(Case);
impl PluginInstance for Fixture {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        let x = support::contract("example/x@1");
        let y = if matches!(self.0, Case::DifferentAdvertisedContract) {
            support::contract("example/y@1")
        } else {
            x.clone()
        };
        into.add(ContractDefinition {
            id: "x".into(),
            contract: x.clone(),
        })?;
        let patch = if matches!(self.0, Case::UnofferedNetworkConflict) {
            let conflicting = Contract::new(
                "example/x@1",
                Arc::new(
                    Interface::new(
                        [PortDef::local::<i64>("run")],
                        [PortDef::local::<String>("value")],
                    )
                    .unwrap(),
                ),
            )
            .unwrap();
            NetworkPatch::new([Edit::Add {
                path: "service".into(),
                definition: Arc::new(
                    ComponentDef::primitive(
                        conflicting.required().clone(),
                        Arc::new(|| support::Transform {
                            increment: 0,
                            emitted: false,
                        }),
                    )
                    .claim(&conflicting)
                    .unwrap(),
                ),
            }])
        } else {
            into.add(ContractDefinition {
                id: "y".into(),
                contract: y.clone(),
            })?;
            into.add(NetworkOffer {
                id: "offer".into(),
                producer: Key::new("provider", "compose"),
                unit: "service".into(),
                contract: Key::new("provider", "x"),
            })?;
            into.add(ContractRequirement {
                id: "request".into(),
                consumer: Key::new("provider", "consumer"),
                contract: Key::new("provider", "y"),
            })?;
            into.add(support::contribution("consumer", NetworkPatch::new([])))?;
            let definition = support::definition(false, true, 1, x.id());
            NetworkPatch::new([Edit::Add {
                path: "service".into(),
                definition: Arc::new((*definition).clone().claim(&y).unwrap()),
            }])
        };
        into.add(support::contribution("compose", patch))
    }
}
fn build(case: Case) -> Result<Kernel, String> {
    let snapshot = support::snapshot("provider", Fixture(case));
    let mut options = support::options(std::slice::from_ref(&snapshot));
    if !matches!(case, Case::UnofferedNetworkConflict) {
        options.imports.selections.insert(
            Key::new("provider", "request"),
            Key::new("provider", "offer"),
        );
    }
    Kernel::build_with_options(
        "host",
        [snapshot],
        Arc::new(LocalProcessDispatcher),
        options,
    )
    .map_err(|error| error.to_string())
}
#[test]
fn a_requirement_cannot_reinterpret_an_offer_advertising_another_contract() {
    assert!(
        build(Case::DifferentAdvertisedContract)
            .err()
            .unwrap()
            .contains("advertising an incompatible contract")
    );
}
#[test]
fn equivalent_named_contract_definitions_can_have_distinct_plugin_keys() {
    build(Case::EquivalentDefinitions).unwrap();
}
#[test]
fn conflicting_registered_contract_and_unoffered_network_claim_fail_closed() {
    assert!(
        build(Case::UnofferedNetworkConflict)
            .err()
            .unwrap()
            .contains("catalog conflict")
    );
}
#[test]
fn foreign_plugin_cannot_launder_a_claim_onto_the_selected_provider_unit() {
    // There is no claim-mutation edit. Even a claimed replacement needs an exact
    // host write grant and changes producer provenance, so cannot fix this offer.
    struct Honest;
    impl PluginInstance for Honest {
        fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
            into.add(ContractDefinition {
                id: "x".into(),
                contract: support::contract("example/x@1"),
            })?;
            into.add(NetworkOffer {
                id: "offer".into(),
                producer: Key::new("provider", "main"),
                unit: "service".into(),
                contract: Key::new("provider", "x"),
            })?;
            into.add(support::contribution(
                "main",
                support::patch("service", false, false, 1, "example/x@1"),
            ))?;
            let mut foreign = support::contribution(
                "foreign",
                NetworkPatch::new([Edit::Replace {
                    path: "service".into(),
                    definition: support::definition(false, true, 1, "example/x@1"),
                }]),
            );
            foreign.requires.push(Key::new("provider", "main"));
            into.add(foreign)
        }
    }
    let snapshot = support::snapshot("provider", Honest);
    let options = support::options(std::slice::from_ref(&snapshot));
    assert!(
        Kernel::build_with_options(
            "host",
            [snapshot],
            Arc::new(LocalProcessDispatcher),
            options
        )
        .err()
        .unwrap()
        .to_string()
        .contains("origin mismatch")
    );
}
