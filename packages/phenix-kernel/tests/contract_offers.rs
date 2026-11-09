use phenix_graph::{DeclaredValue, GraphBuilder, GraphContract, InterfaceShape, LocalExecutor};
use phenix_kernel::{
    ContractDefinition, Contribution, ImportPolicy, Kernel, Key,
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
        
        
        Contribution::new("consumer", |_| Ok(()))
.import("request", Key::new("provider", "definition"))
.register(into)?;
        let config = *self;
        Contribution::new("main", move |context| {
                let graph = &mut *context.graph;
                if config.module {
                    let mut module = GraphBuilder::new("implementation");
                    let node = module.node("private", |x: i64| x + 1)?;
                    module.expose_input_slot("run", "value", node.inlet())?;
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
            })
.offer("offer", if self.missing_path {
                "absent"
            } else {
                "service"
            }
            , Key::new("provider", "definition"))
.register(into)?;
        if self.late_edit {
            Contribution::new("late", |context| {
                    let graph = &mut *context.graph;
                    *graph = GraphBuilder::new("late");
                    graph.node("service", |x: i64| x + 2)?;
                    Ok(())
                })
.after(vec![Key::new("provider", "main")])
.register(into)?;
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
            // This plugin cannot forge a provider key: it can only offer a
            // foreign unit from its own composer, which final origin checks reject.
            return Contribution::new("fraud", |_| Ok(()))
                .offer("stolen", "service", Key::new("provider", "definition"))
                .register(into);
        }
        let module = self.module;
        Contribution::new("patch", move |context| {
                let graph = &mut *context.graph;
                if module {
                    let mut child = GraphBuilder::new("replacement");
                    let node = child.node("private", |x: i64| x + 10)?;
                    child.expose_input_slot("run", "value", node.inlet())?;
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
            })
.after(vec![Key::new("provider", "main")])
.register(into)
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
