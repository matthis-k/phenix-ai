use phenix_graph::{
    CompiledGraph, DeclaredValue, GraphBuilder, GraphContract, InterfaceShape, LocalExecutor,
};
use phenix_kernel::{ContractDefinition, Contribution, GraphOffer, Kernel, Key};
use phenix_plugin::{Code, Failure, PluginCatalog, PluginInstance, Registrar};
use std::sync::{Arc, Mutex};

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

#[derive(Clone)]
struct Provider {
    module: bool,
    sibling: bool,
    compiled: Arc<Mutex<Option<Arc<CompiledGraph>>>>,
}

impl PluginInstance for Provider {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        into.add(ContractDefinition {
            id: "definition".into(),
            contract: contract(),
        })?;
        into.add(GraphOffer {
            id: "offer".into(),
            producer: Key::new("provider", "main"),
            unit: "service".into(),
            contract: Key::new("provider", "definition"),
        })?;
        if self.sibling {
            into.add(Contribution {
                id: "main".into(),
                requires: vec![],
                routes: vec![],
                compose: Box::new(|_| Ok(())),
            })?;
        }
        let config = self.clone();
        into.add(Contribution {
            id: if self.sibling { "sibling" } else { "main" }.into(),
            requires: vec![],
            routes: vec![],
            compose: Box::new(move |context| {
                let graph = &mut *context.graph;
                if config.module {
                    let mut child = GraphBuilder::new("implementation");
                    let node = child.node("private", |x: i64| x + 1)?;
                    child.expose_input_slot("run", "value", node.inlet())?;
                    child.expose_output("value", node.output())?;
                    child.implements_public(&contract())?;
                    graph.instance("service", &child.module())?;
                } else {
                    graph.node("service", |x: i64| x + 1)?;
                    graph.implements("service", &contract())?;
                }
                // Engine does not expose its compiled graph. Preserve the same
                // declarations and kernel-assigned provenance for Plugin API binding.
                *config.compiled.lock().unwrap() = Some(graph.clone().compile()?);
                Ok(())
            }),
        })
    }
}

#[test]
fn kernel_offer_binds_only_to_its_plugin_revision_and_exact_producer() {
    let mut catalog = PluginCatalog::new();
    let key = catalog
        .register("provider", |config: Provider| Ok(config))
        .unwrap();
    for module in [false, true] {
        let config = Provider {
            module,
            sibling: false,
            compiled: Arc::default(),
        };
        let mut plugins = catalog.instance();
        let snapshot = plugins.load(&key, config.clone()).unwrap();
        let kernel = Kernel::build("host", [snapshot.clone()], Arc::new(LocalExecutor)).unwrap();
        let compiled = config.compiled.lock().unwrap().take().unwrap();
        let origin = compiled.find("service").unwrap().origin().unwrap().clone();
        assert_ne!(&origin, snapshot.graph_origin());
        assert!(origin.belongs_to(snapshot.graph_origin()));
        snapshot
            .bind::<GraphOffer>("offer", &contract(), &compiled)
            .unwrap();

        let staged = plugins.prepare_reload(&key, config.clone()).unwrap();
        let candidate = staged.candidate().unwrap();
        drop(staged);
        let replacement = plugins.reload(&key, config.clone()).unwrap();
        let foreign = catalog.instance().load(&key, config.clone()).unwrap();
        assert_eq!(candidate.revision(), replacement.revision());
        assert_eq!(snapshot.revision(), foreign.revision());
        for other in [candidate, replacement, foreign] {
            let error = other
                .bind::<GraphOffer>("offer", &contract(), &compiled)
                .err()
                .unwrap();
            assert_eq!(error.code, Code::Denied);
        }
        drop(kernel);
        plugins.unload(&key).unwrap();
        snapshot
            .bind::<GraphOffer>("offer", &contract(), &compiled)
            .unwrap();

        let sibling = plugins
            .load(
                &key,
                Provider {
                    sibling: true,
                    ..config
                },
            )
            .unwrap();
        let error = Kernel::build("host", [sibling], Arc::new(LocalExecutor))
            .err()
            .unwrap();
        assert!(error.to_string().contains("origin mismatch"), "{error}");
    }
}
