mod support;
use phenix_graph::{compose::NetworkPatch, process::LocalProcessDispatcher};
use phenix_kernel::{ContractDefinition, Kernel, Key, NetworkOffer};
use phenix_plugin::{Code, Failure, PluginCatalog, PluginInstance, Registrar};
use std::sync::Arc;
#[derive(Clone)]
struct Provider {
    module: bool,
    sibling: bool,
}
impl PluginInstance for Provider {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        into.add(ContractDefinition {
            id: "definition".into(),
            contract: support::contract("example/service@1"),
        })?;
        into.add(NetworkOffer {
            id: "offer".into(),
            producer: Key::new("provider", "main"),
            unit: "service".into(),
            contract: Key::new("provider", "definition"),
        })?;
        if self.sibling {
            into.add(support::contribution("main", NetworkPatch::new([])))?;
        }
        into.add(support::contribution(
            if self.sibling { "sibling" } else { "main" },
            support::patch("service", self.module, true, 1, "example/service@1"),
        ))
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
        };
        let mut plugins = catalog.instance();
        let snapshot = plugins.load(&key, config.clone()).unwrap();
        let options = support::options(std::slice::from_ref(&snapshot));
        let kernel = Kernel::build_with_options(
            "host",
            [snapshot.clone()],
            Arc::new(LocalProcessDispatcher),
            options,
        )
        .unwrap();
        let compiled = kernel.compiled().clone();
        let origin = compiled.inspect("service").unwrap().origin.unwrap();
        assert_ne!(&origin, snapshot.graph_origin());
        assert!(origin.belongs_to(snapshot.graph_origin()));
        snapshot
            .bind::<NetworkOffer>("offer", &support::contract("example/service@1"), &compiled)
            .unwrap();
        let stage = plugins.prepare_reload(&key, config.clone()).unwrap();
        let candidate = stage.candidate().unwrap();
        drop(stage);
        let replacement = plugins.reload(&key, config.clone()).unwrap();
        let foreign = catalog.instance().load(&key, config.clone()).unwrap();
        assert_eq!(candidate.revision(), replacement.revision());
        assert_eq!(snapshot.revision(), foreign.revision());
        for other in [candidate, replacement, foreign] {
            assert_eq!(
                other
                    .bind::<NetworkOffer>(
                        "offer",
                        &support::contract("example/service@1"),
                        &compiled
                    )
                    .err()
                    .unwrap()
                    .code,
                Code::Denied
            );
        }
        drop(kernel);
        plugins.unload(&key).unwrap();
        snapshot
            .bind::<NetworkOffer>("offer", &support::contract("example/service@1"), &compiled)
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
        let options = support::options(std::slice::from_ref(&sibling));
        assert!(
            Kernel::build_with_options(
                "host",
                [sibling],
                Arc::new(LocalProcessDispatcher),
                options
            )
            .err()
            .unwrap()
            .to_string()
            .contains("origin mismatch")
        );
    }
}
