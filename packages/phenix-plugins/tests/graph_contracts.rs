//! Binding validates actual native declarations and exact revision ancestry.
use phenix_graph::{
    Interface, PortDef,
    component::ComponentDef,
    contract::Contract,
    network::NetworkBuilder,
    process::{Process, ProcessContext, Step},
};
use phenix_plugin::{Code, Contribution, Failure, PluginCatalog, PluginInstance, Registrar};
use std::{num::NonZeroUsize, sync::Arc};

struct ProviderAction {
    path: Option<String>,
    increment: i64,
}
impl Contribution for ProviderAction {
    const KIND: &'static str = "network.node";
    fn id(&self) -> &str {
        "model"
    }
    fn component_path(&self) -> Option<&str> {
        self.path.as_deref()
    }
}
struct Provider(Option<String>, i64);
impl PluginInstance for Provider {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        into.add(ProviderAction {
            path: self.0.clone(),
            increment: self.1,
        })
    }
}
struct Identity;
impl Process for Identity {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        Ok(Step::Complete)
    }
}
fn contract(wire: bool) -> Contract {
    let input = if wire {
        PortDef::wire::<i64>("run").unwrap()
    } else {
        PortDef::local::<i64>("run")
    };
    Contract::new(
        "plugina/provider@1",
        Arc::new(Interface::new([input], [PortDef::local::<i64>("value")]).unwrap()),
    )
    .unwrap()
}
fn compiled(
    snapshot: &phenix_plugin::PluginSnapshot,
    claimed: bool,
    module: bool,
) -> phenix_graph::network::CompiledNetwork {
    let required = contract(false);
    let mut definition =
        ComponentDef::primitive(required.required().clone(), Arc::new(|| Identity));
    if claimed {
        definition = definition.claim(&required).unwrap();
    }
    if module {
        let mut child = NetworkBuilder::new();
        let node = child.add("private", Arc::new(definition)).unwrap();
        child
            .expose_input(
                "run",
                node.input::<i64>("run").unwrap(),
                NonZeroUsize::new(1).unwrap(),
            )
            .unwrap();
        child
            .expose_output("value", node.output::<i64>("value").unwrap())
            .unwrap();
        if claimed {
            child.claim(&required).unwrap();
        }
        definition = ComponentDef::composite(child.build().unwrap());
    }
    let mut network = NetworkBuilder::new();
    network
        .add(
            "service",
            Arc::new(definition.with_origin(snapshot.graph_origin().child())),
        )
        .unwrap();
    network.build().unwrap().compile().unwrap()
}

#[test]
fn plugin_request_checks_final_selected_unit() {
    let mut catalog = PluginCatalog::new();
    let key = catalog
        .register("provider", |path: Option<String>| Ok(Provider(path, 41)))
        .unwrap();
    let mut plugins = catalog.instance();
    let snapshot = plugins.load(&key, Some("service".into())).unwrap();
    let required = contract(false);
    let error = snapshot
        .bind::<ProviderAction>("model", &required, &compiled(&snapshot, false, false))
        .err()
        .unwrap();
    assert_eq!(error.code, Code::SchemaRejected);
    let context = error.context.as_ref().unwrap();
    assert_eq!(context.plugin, "provider");
    assert_eq!(context.phase, phenix_plugin::CallbackPhase::Bind);
    assert_eq!(context.contribution.as_deref(), Some("model"));
    let source = std::error::Error::source(&error)
        .unwrap()
        .source()
        .unwrap()
        .downcast_ref::<Failure>()
        .unwrap();
    assert!(
        std::error::Error::source(source)
            .unwrap()
            .is::<phenix_graph::network::NetworkError>()
    );
    let network = compiled(&snapshot, true, false);
    let action = snapshot
        .bind::<ProviderAction>("model", &required, &network)
        .unwrap();
    assert_eq!(action.increment, 41);
    assert!(
        snapshot
            .bind::<ProviderAction>("model", &contract(true), &network)
            .is_err()
    );
    let missing = plugins.reload(&key, Some("missing".into())).unwrap();
    assert_eq!(
        missing
            .bind::<ProviderAction>("model", &required, &network)
            .err()
            .unwrap()
            .code,
        Code::MissingBinding
    );
    let no_locator = plugins.reload(&key, None).unwrap();
    assert!(
        no_locator
            .bind::<ProviderAction>("model", &required, &network)
            .is_err()
    );
    plugins.unload(&key).unwrap();
    assert_eq!(
        snapshot
            .bind::<ProviderAction>("model", &required, &network)
            .unwrap()
            .increment,
        41
    );
    assert_eq!(action.increment, 41);
}

#[test]
fn module_offers_use_the_same_compiled_unit_validation() {
    let mut catalog = PluginCatalog::new();
    let key = catalog
        .register("provider", |_: ()| Ok(Provider(Some("service".into()), 1)))
        .unwrap();
    let snapshot = catalog.instance().load(&key, ()).unwrap();
    snapshot
        .bind::<ProviderAction>("model", &contract(false), &compiled(&snapshot, true, true))
        .unwrap();
}

#[test]
fn binding_rejects_other_revisions_sets_and_uncommitted_candidates() {
    let mut catalog = PluginCatalog::new();
    let key = catalog
        .register("provider", |increment: i64| {
            Ok(Provider(Some("service".into()), increment))
        })
        .unwrap();
    for module in [false, true] {
        let mut plugins = catalog.instance();
        let old = plugins.load(&key, 1).unwrap();
        let network = compiled(&old, true, module);
        old.bind::<ProviderAction>("model", &contract(false), &network)
            .unwrap();
        let stage = plugins.prepare_reload(&key, 2).unwrap();
        let rejected = stage.candidate().unwrap();
        drop(stage);
        let current = plugins.reload(&key, 3).unwrap();
        assert_eq!(rejected.revision(), current.revision());
        let sibling = catalog.instance().load(&key, 1).unwrap();
        assert_eq!(sibling.revision(), old.revision());
        for foreign in [&rejected, &current, &sibling] {
            let error = foreign
                .bind::<ProviderAction>("model", &contract(false), &network)
                .err()
                .unwrap();
            assert_eq!(error.code, Code::Denied);
            assert!(error.message.contains("origin mismatch"));
        }
        plugins.unload(&key).unwrap();
        assert_eq!(
            old.bind::<ProviderAction>("model", &contract(false), &network)
                .unwrap()
                .increment,
            1
        );
    }
}
