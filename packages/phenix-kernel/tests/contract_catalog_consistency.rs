use phenix_graph::{DeclaredValue, GraphContract, InterfaceShape, LocalExecutor};
use phenix_kernel::{
    ContractDefinition, ContractRequirement, Contribution, GraphOffer, ImportPolicy, Kernel, Key,
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
                producer: Key::new("provider", "compose"),
                unit: "service".into(),
                contract: Key::new("provider", "x"),
            })?;
            registrar.add(ContractRequirement {
                id: "request".into(),
                consumer: Key::new("provider", "consumer"),
                contract: Key::new("provider", "y"),
            })?;
            registrar.add(Contribution {
                id: "consumer".into(),
                requires: vec![],
                routes: vec![],
                compose: Box::new(|_| Ok(())),
            })?;
        }
        let case = self.0;
        registrar.add(Contribution {
            id: "compose".into(),
            requires: Vec::new(),
            routes: Vec::new(),
            compose: Box::new(move |context| {
                let graph = &mut *context.graph;
                match case {
                    Case::DifferentAdvertisedContract | Case::EquivalentDefinitions => {
                        graph.node("service", |v: i64| v + 1)?;
                        graph.implements("service", &x)?;
                        if matches!(case, Case::DifferentAdvertisedContract) {
                            graph.implements("service", &y)?;
                        }
                    }
                    Case::UnofferedGraphConflict => {
                        graph.node("service", |v: i64| v.to_string())?;
                        graph.implements(
                            "service",
                            &contract("example/x@1", DeclaredValue::local::<String>()),
                        )?;
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
    let mut policy = ImportPolicy::default();
    if !matches!(case, Case::UnofferedGraphConflict) {
        policy.selections.insert(
            Key::new("provider", "request"),
            Key::new("provider", "offer"),
        );
    }
    Kernel::build_with_options(
        "host",
        [provider],
        Arc::new(LocalExecutor),
        phenix_kernel::BuildOptions {
            imports: policy,
            ..Default::default()
        },
    )
    .map_err(|error| error.to_string())
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
            producer: Key::new("provider", "main"),
            unit: "service".into(),
            contract: Key::new("provider", "x"),
        })?;
        registrar.add(Contribution {
            id: "main".into(),
            requires: Vec::new(),
            routes: Vec::new(),
            compose: Box::new(|context| {
                let graph = &mut *context.graph;
                graph.node("service", |v: i64| v + 1)?;
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
            compose: Box::new(|context| {
                let graph = &mut *context.graph;
                graph
                    .implements(
                        "service",
                        &contract("example/x@1", DeclaredValue::local::<i64>()),
                    )
                    .map_err(Into::into)
            }),
        })
    }
}

#[test]
fn foreign_plugin_cannot_launder_a_claim_onto_the_selected_provider_unit() {
    let mut catalog = PluginCatalog::new();
    let provider = catalog
        .register("provider", |_: ()| Ok(HonestOffer))
        .unwrap();
    let foreign = catalog
        .register("foreign", |_: ()| Ok(ForeignClaim))
        .unwrap();
    let mut plugins = catalog.instance();
    plugins.load(&provider, ()).unwrap();
    plugins.load(&foreign, ()).unwrap();
    let failure = Kernel::build("host", plugins.snapshots(), Arc::new(LocalExecutor))
        .err()
        .expect("foreign semantic claim mutation must fail");
    assert!(
        failure
            .to_string()
            .contains("another contribution generation"),
        "{failure}"
    );
}
