use phenix_graph::{DeclaredValue, GraphBuilder, GraphContract, InterfaceShape, LocalExecutor};
use phenix_kernel::{
    ContractDefinition, ContractRequirement, Contribution, GraphOffer, ImportPolicy, Kernel, Key,
};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::sync::Arc;

fn contract() -> GraphContract {
    GraphContract::new(
        "example/service@1",
        InterfaceShape::new()
            .input_slot("run", "value", DeclaredValue::local::<i64>())
            .unwrap()
            .output("value", DeclaredValue::local::<i64>())
            .unwrap(),
    )
    .unwrap()
}

#[derive(Clone, Copy)]
struct Plugin {
    module: bool,
    claim: bool,
    missing_path: bool,
    late_edit: bool,
}
impl PluginInstance for Plugin {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        into.add(ContractDefinition {
            id: "definition".into(),
            contract: contract(),
        })?;
        into.add(GraphOffer {
            id: "offer".into(),
            producer: Key::new("provider", "main"),
            unit: if self.missing_path {
                "absent"
            } else {
                "service"
            }
            .into(),
            contract: Key::new("provider", "definition"),
        })?;
        into.add(ContractRequirement {
            id: "request".into(),
            consumer: Key::new("provider", "consumer"),
            contract: Key::new("provider", "definition"),
        })?;
        into.add(Contribution {
            id: "consumer".into(),
            requires: vec![],
            routes: vec![],
            compose: Box::new(|_| Ok(())),
        })?;
        let config = *self;
        into.add(Contribution {
            id: "main".into(),
            requires: vec![],
            routes: vec![],
            compose: Box::new(move |context| {
                let graph = &mut *context.graph;
                if config.module {
                    let mut module = GraphBuilder::new("implementation");
                    let node = module.node("private", |x: i64| x + 1)?;
                    module.expose_input_field(
                        "run",
                        phenix_graph::SlotField::<(), i64>::new("value"),
                        node.inlet(),
                    )?;
                    module.expose_output("value", node.output())?;
                    if config.claim {
                        module.implements_public(&contract())?;
                    }
                    graph.instance("service", &module.module())?;
                } else {
                    graph.node("service", |x: i64| x + 1)?;
                    if config.claim {
                        graph.implements("service", &contract())?;
                    }
                }
                Ok(())
            }),
        })?;
        if self.late_edit {
            into.add(Contribution {
                id: "late".into(),
                requires: vec![Key::new("provider", "main")],
                routes: vec![],
                compose: Box::new(|context| {
                    let graph = &mut *context.graph;
                    *graph = GraphBuilder::new("late");
                    graph.node("service", |x: i64| x + 2)?;
                    Ok(())
                }),
            })?;
        }
        Ok(())
    }
}

#[test]
fn node_and_module_offers_require_final_claimed_units_and_valid_host_selection() {
    let mut catalog = PluginCatalog::new();
    let key = catalog
        .register("provider", |config: Plugin| Ok(config))
        .unwrap();
    for module in [false, true] {
        for failure in [
            None,
            Some("claim"),
            Some("path"),
            Some("selection"),
            Some("late"),
        ] {
            let mut plugins = catalog.instance();
            let snapshot = plugins
                .load(
                    &key,
                    Plugin {
                        module,
                        claim: failure != Some("claim"),
                        missing_path: failure == Some("path"),
                        late_edit: failure == Some("late"),
                    },
                )
                .unwrap();
            let mut policy = ImportPolicy::default();
            if failure == Some("selection") {
                policy.selections.insert(
                    Key::new("provider", "request"),
                    Key::new("provider", "absent"),
                );
            }
            let result = Kernel::build_with_options(
                "host",
                [snapshot],
                Arc::new(LocalExecutor),
                phenix_kernel::BuildOptions {
                    imports: policy,
                    ..Default::default()
                },
            )
            .map_err(|error| error.to_string());
            if let Some(failure) = failure {
                let error = result.err().expect("invalid offer must fail");
                let expected = match failure {
                    "path" | "selection" => "absent",
                    _ => "claim",
                };
                assert!(
                    error.contains(expected),
                    "module={module}, case={failure}: {error}"
                );
            } else {
                result.unwrap();
            }
        }
    }
}

struct ForeignPlugin {
    module: bool,
    replace: bool,
}
impl PluginInstance for ForeignPlugin {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        if !self.replace {
            return into.add(GraphOffer {
                id: "stolen".into(),
                producer: Key::new("provider", "main"),
                unit: "service".into(),
                contract: Key::new("provider", "definition"),
            });
        }
        let module = self.module;
        into.add(Contribution {
            id: "patch".into(),
            requires: vec![Key::new("provider", "main")],
            routes: vec![],
            compose: Box::new(move |context| {
                let graph = &mut *context.graph;
                if module {
                    let mut child = GraphBuilder::new("replacement");
                    let node = child.node("private", |x: i64| x + 10)?;
                    child.expose_input_field(
                        "run",
                        phenix_graph::SlotField::<(), i64>::new("value"),
                        node.inlet(),
                    )?;
                    child.expose_output("value", node.output())?;
                    child.implements_public(&contract())?;
                    graph.replace("service", child.module())?;
                } else {
                    let mut replacement =
                        phenix_graph::NodeTemplate::local_function(|x: i64| x + 10);
                    replacement.claims.push(contract());
                    graph.replace("service", replacement)?;
                }
                Ok(())
            }),
        })
    }
}

#[test]
fn offers_cannot_launder_foreign_nodes_modules_or_replacement_generations() {
    let mut catalog = PluginCatalog::new();
    let provider = catalog
        .register("provider", |config: Plugin| Ok(config))
        .unwrap();
    let foreign = catalog
        .register("foreign", |config: ForeignPlugin| Ok(config))
        .unwrap();
    for module in [false, true] {
        for replace in [false, true] {
            let mut plugins = catalog.instance();
            plugins
                .load(
                    &provider,
                    Plugin {
                        module,
                        claim: true,
                        missing_path: false,
                        late_edit: false,
                    },
                )
                .unwrap();
            plugins
                .load(&foreign, ForeignPlugin { module, replace })
                .unwrap();
            let error = Kernel::build("host", plugins.snapshots(), Arc::new(LocalExecutor))
                .err()
                .unwrap();
            assert!(
                error.to_string().contains("origin mismatch"),
                "module={module}, replace={replace}: {error}"
            );
        }
    }
}
