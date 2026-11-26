mod support;
use phenix_graph::{
    Interface, PortDef,
    component::ComponentDef,
    compose::{Edit, NetworkPatch},
    contract::Contract,
    interface::{InterfaceError, InterfaceReason},
    process::LocalProcessDispatcher,
};
use phenix_kernel::{
    BuildOptions, ContractDefinition, IneligibleReason, Kernel, Key, ResolutionError,
};
use phenix_plugin::{Failure, PluginInstance, Registrar};
use std::error::Error;
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
            support::contribution("consumer", NetworkPatch::new([]))
                .import("request", Key::new("provider", "y"))
                .register(into)?;
            let definition = support::definition(false, true, 1, x.id());
            NetworkPatch::new([Edit::Add {
                path: "service".into(),
                definition: Arc::new((*definition).clone().claim(&y).unwrap()),
            }])
        };
        let mut contribution = support::contribution("compose", patch);
        if !matches!(self.0, Case::UnofferedNetworkConflict) {
            contribution = contribution.offer("offer", "service", Key::new("provider", "x"));
        }
        contribution.register(into)
    }
}
fn build(case: Case) -> Result<Kernel, ResolutionError> {
    let mut options = BuildOptions::default();
    support::grant(
        &mut options,
        Key::new("provider", "compose"),
        [phenix_graph::compose::Footprint::Component(
            "service".into(),
        )],
    );
    let snapshot = support::snapshot("provider", Fixture(case));
    if !matches!(case, Case::UnofferedNetworkConflict) {
        options.imports.selections.insert(
            Key::new("provider", "request"),
            Key::new("provider", "offer"),
        );
    }
    Kernel::build_with_options(
        "host",
        [snapshot],
        options.with_dispatcher(Arc::new(LocalProcessDispatcher)),
    )
}
#[test]
fn a_requirement_cannot_reinterpret_an_offer_advertising_another_contract() {
    let error = build(Case::DifferentAdvertisedContract).err().unwrap();
    let ResolutionError::Ineligible {
        requirement,
        offer,
        reason: IneligibleReason::IncompatibleContract(cause),
    } = &error
    else {
        panic!("expected an incompatible selected offer: {error:?}");
    };
    assert_eq!(*requirement, Key::new("provider", "request"));
    assert_eq!(*offer, Key::new("provider", "offer"));
    let source = error.source().unwrap();
    assert!(std::ptr::eq(
        source.downcast_ref::<InterfaceError>().unwrap(),
        cause.as_ref()
    ));
    assert!(source.source().is_none());
    assert!(matches!(
        cause.reason.as_ref(),
        InterfaceReason::DifferingContractDefinition { expected, actual }
            if expected.id() == "example/y@1" && actual.id() == "example/x@1"
    ));
    assert_eq!(
        error.to_string(),
        format!(
            "ineligible selection for requirement {requirement:?}, offer {offer:?}: offer advertising an incompatible contract: {cause}"
        )
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
            .to_string()
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
            support::contribution(
                "main",
                support::patch("service", false, false, 1, "example/x@1"),
            )
            .offer("offer", "service", Key::new("provider", "x"))
            .register(into)?;
            support::contribution(
                "foreign",
                NetworkPatch::new([Edit::Replace {
                    path: "service".into(),
                    definition: support::definition(false, true, 1, "example/x@1"),
                }]),
            )
            .after([Key::new("provider", "main")])
            .register(into)
        }
    }
    let mut options = BuildOptions::default();
    for id in ["main", "foreign"] {
        support::grant(
            &mut options,
            Key::new("provider", id),
            [phenix_graph::compose::Footprint::Component(
                "service".into(),
            )],
        );
    }
    let snapshot = support::snapshot("provider", Honest);
    assert!(
        Kernel::build_with_options(
            "host",
            [snapshot],
            options.with_dispatcher(Arc::new(LocalProcessDispatcher))
        )
        .err()
        .unwrap()
        .to_string()
        .contains("origin mismatch")
    );
}
