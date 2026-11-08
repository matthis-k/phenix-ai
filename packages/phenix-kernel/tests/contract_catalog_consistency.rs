use phenix_graph::{DeclaredValue, GraphBuilder, GraphContract, InterfaceShape, LocalExecutor};
use phenix_kernel::{
    ContractDefinition, ContractRequirement, Contribution, GraphOffer, Kernel, Key,
};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::sync::Arc;

fn contract(id: &str, result: DeclaredValue) -> GraphContract {
    GraphContract::new(
        id,
        InterfaceShape::new()
            .input_slot("run", "value", DeclaredValue::local::<i64>())
            .unwrap()
            .output("value", result)
            .unwrap(),
    )
    .unwrap()
}

#[derive(Clone, Copy)]
enum Case {
    DifferentAdvertisedContract,
    EquivalentDefinitions,
    UnofferedGraphConflict,
}

struct Fixture(Case);

impl PluginInstance for Fixture {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        let x = contract("example/x@1", DeclaredValue::local::<i64>());
        let y = match self.0 {
            Case::DifferentAdvertisedContract => {
                contract("example/y@1", DeclaredValue::local::<i64>())
            }
            Case::EquivalentDefinitions | Case::UnofferedGraphConflict => x.clone(),
        };
        registrar.add(ContractDefinition {
            id: "x".into(),
            contract: x.clone(),
        })?;
        if !matches!(self.0, Case::UnofferedGraphConflict) {
            registrar.add(ContractDefinition {
                id: "y".into(),
                contract: y.clone(),
            })?;
            registrar.add(GraphOffer {
                id: "offer".into(),
                unit: "service".into(),
                contract: Key::new("provider", "x"),
            })?;
            registrar.add(ContractRequirement {
                id: "request".into(),
                contract: Key::new("provider", "y"),
                offer: Key::new("provider", "offer"),
            })?;
        }
        let case = self.0;
        registrar.add(Contribution {
            id: "compose".into(),
            requires: Vec::new(),
            routes: Vec::new(),
            activate: None,
            compose: Box::new(move |graph: &mut GraphBuilder| {
                match case {
                    Case::DifferentAdvertisedContract | Case::EquivalentDefinitions => {
                        graph
                            .node("service", |v: i64| v + 1)
                            .map_err(|error| error.to_string())?;
                        graph
                            .implements("service", &x)
                            .map_err(|error| error.to_string())?;
                        if matches!(case, Case::DifferentAdvertisedContract) {
                            graph
                                .implements("service", &y)
                                .map_err(|error| error.to_string())?;
                        }
                    }
                    Case::UnofferedGraphConflict => {
                        graph
                            .node("service", |v: i64| v.to_string())
                            .map_err(|error| error.to_string())?;
                        graph
                            .implements(
                                "service",
                                &contract("example/x@1", DeclaredValue::local::<String>()),
                            )
                            .map_err(|error| error.to_string())?;
                    }
                }
                Ok(())
            }),
        })
    }
}

fn build(case: Case) -> Result<Kernel, String> {
    let mut catalog = PluginCatalog::new();
    let key = catalog
        .register("provider", |case: Case| Ok(Fixture(case)))
        .unwrap();
    let mut plugins = catalog.instance();
    let provider = plugins.load(&key, case).unwrap();
    Kernel::build("host", [provider], Arc::new(LocalExecutor))
}

#[test]
fn a_requirement_cannot_reinterpret_an_offer_advertising_another_contract() {
    let failure = build(Case::DifferentAdvertisedContract)
        .err()
        .expect("a mismatched advertised contract must fail");
    assert!(
        failure.contains("advertising an incompatible contract"),
        "{failure}"
    );
}

#[test]
fn equivalent_named_contract_definitions_can_have_distinct_plugin_keys() {
    build(Case::EquivalentDefinitions)
        .expect("identical named contract definitions should interoperate");
}

#[test]
fn conflicting_registered_contract_and_unoffered_graph_claim_fail_closed() {
    let failure = build(Case::UnofferedGraphConflict)
        .err()
        .expect("unoffered conflicting graph claims must be rejected");
    assert!(
        failure.contains("host/graph contract catalog conflict"),
        "{failure}"
    );
}

struct HonestOffer;

impl PluginInstance for HonestOffer {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        registrar.add(ContractDefinition {
            id: "x".into(),
            contract: contract("example/x@1", DeclaredValue::local::<i64>()),
        })?;
        registrar.add(GraphOffer {
            id: "offer".into(),
            unit: "service".into(),
            contract: Key::new("provider", "x"),
        })?;
        registrar.add(Contribution {
            id: "main".into(),
            requires: Vec::new(),
            routes: Vec::new(),
            activate: None,
            compose: Box::new(|graph| {
                graph
                    .node("service", |v: i64| v + 1)
                    .map_err(|error| error.to_string())?;
                Ok(())
            }),
        })
    }
}

struct ForeignClaim;

impl PluginInstance for ForeignClaim {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        registrar.add(Contribution {
            id: "mutate".into(),
            requires: vec![Key::new("provider", "main")],
            routes: Vec::new(),
            activate: None,
            compose: Box::new(|graph| {
                graph
                    .implements(
                        "service",
                        &contract("example/x@1", DeclaredValue::local::<i64>()),
                    )
                    .map_err(|error| error.to_string())
            }),
        })
    }
}

#[test]
fn foreign_plugin_cannot_launder_a_claim_onto_the_selected_provider_unit() {
    let mut catalog = PluginCatalog::new();
    let provider = catalog.register("provider", |_: ()| Ok(HonestOffer)).unwrap();
    let foreign = catalog.register("foreign", |_: ()| Ok(ForeignClaim)).unwrap();
    let mut plugins = catalog.instance();
    plugins.load(&provider, ()).unwrap();
    plugins.load(&foreign, ()).unwrap();
    let failure = Kernel::build("host", plugins.snapshots(), Arc::new(LocalExecutor))
        .err()
        .expect("foreign semantic claim mutation must fail");
    assert!(
        failure.contains("another contribution generation"),
        "{failure}"
    );
}
