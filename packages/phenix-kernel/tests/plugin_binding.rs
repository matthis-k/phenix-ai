mod support;
use phenix_graph::{compose::NetworkPatch, process::LocalProcessDispatcher};
use phenix_kernel::{BuildOptions, ContractDefinition, Kernel, Key, plugin_binding::bind_component};
use phenix_plugin::{Code, Failure, PluginCatalog, PluginInstance, Registrar};
use std::sync::Arc;
struct Locator;
impl phenix_plugin::Contribution for Locator {
    const KIND: &'static str = "test.locator";
    fn id(&self) -> &str {
        "offer"
    }
}
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
        into.add(Locator)?;
        if self.sibling {
            support::contribution("main", NetworkPatch::new([]))
                .after([Key::new("provider", "sibling")])
                .offer("offer", "service", Key::new("provider", "definition"))
                .register(into)?;
        }
        let mut contribution = support::contribution(
            if self.sibling { "sibling" } else { "main" },
            support::patch("service", self.module, true, 1, "example/service@1"),
        );
        if !self.sibling {
            contribution =
                contribution.offer("offer", "service", Key::new("provider", "definition"));
        }
        contribution.register(into)
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
        let mut options = BuildOptions::default();
        support::grant(
            &mut options,
            Key::new("provider", "main"),
            [phenix_graph::compose::Footprint::Component(
                "service".into(),
            )],
        );
        let snapshot = plugins.load(&key, config.clone()).unwrap();
        let kernel = Kernel::build_with_options(
            "host",
            [snapshot.clone()],
            options.with_dispatcher(Arc::new(LocalProcessDispatcher)),
        )
        .unwrap();
        let compiled = kernel.compiled().clone();
        let origin = compiled.inspect("service").unwrap().origin.unwrap();
        assert_ne!(
            &origin,
            &phenix_graph::GraphOrigin::from_root(snapshot.revision_identity().clone())
        );
        assert!(
            origin.belongs_to(&phenix_graph::GraphOrigin::from_root(
                snapshot.revision_identity().clone(),
            ))
        );
        bind_component::<Locator>(
            &snapshot,
            "offer",
            |_| Some("service"),
            &support::contract("example/service@1"),
            &compiled,
        )
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
                bind_component::<Locator>(
                    &other,
                    "offer",
                    |_| Some("service"),
                    &support::contract("example/service@1"),
                    &compiled,
                )
                    .err()
                    .unwrap()
                    .code,
                Code::Denied
            );
        }
        drop(kernel);
        plugins.unload(&key).unwrap();
        bind_component::<Locator>(
            &snapshot,
            "offer",
            |_| Some("service"),
            &support::contract("example/service@1"),
            &compiled,
        )
            .unwrap();
        let mut options = BuildOptions::default();
        support::grant(
            &mut options,
            Key::new("provider", "sibling"),
            [phenix_graph::compose::Footprint::Component(
                "service".into(),
            )],
        );
        let sibling = plugins
            .load(
                &key,
                Provider {
                    sibling: true,
                    ..config
                },
            )
            .unwrap();
        assert!(
            Kernel::build_with_options(
                "host",
                [sibling],
                options.with_dispatcher(Arc::new(LocalProcessDispatcher))
            )
            .err()
            .unwrap()
            .to_string()
            .contains("origin mismatch")
        );
    }
}
