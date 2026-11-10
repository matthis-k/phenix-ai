//! Binding validates the final actual unit, not a copied provider descriptor.
use phenix_graph::{DeclaredValue, GraphBuilder, GraphContract, InterfaceShape};
use phenix_plugin::{Code, Contribution, Failure, PluginCatalog, PluginInstance, Registrar};

struct ProviderAction {
    path: Option<String>,
    increment: i64,
}
impl Contribution for ProviderAction {
    const KIND: &'static str = "graph.node";
    fn id(&self) -> &str {
        "model"
    }
    fn graph_unit(&self) -> Option<&str> {
        self.path.as_deref()
    }
}
struct Provider(Option<String>, i64);
fn contract(input: DeclaredValue) -> GraphContract {
    GraphContract::new(
        "plugina/provider@1",
        InterfaceShape::new()
            .input_slot("run", "value", input)
            .unwrap()
            .output("value", DeclaredValue::local::<i64>())
            .unwrap(),
    )
    .unwrap()
}
impl PluginInstance for Provider {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        into.add(ProviderAction {
            path: self.0.clone(),
            increment: self.1,
        })
    }
}

#[test]
fn plugin_request_checks_final_selected_unit() {
    let mut catalog = PluginCatalog::new();
    let key = catalog
        .register("provider", |path: Option<String>| Ok(Provider(path, 41)))
        .unwrap();
    let mut plugins = catalog.instance();
    let snapshot = plugins.load(&key, Some("provider".into())).unwrap();
    let required = contract(DeclaredValue::local::<i64>());
    let mut graph = GraphBuilder::new("agent");
    graph.with_origin(snapshot.graph_origin(), |graph| {
        graph.node("provider", |x: i64| x + 1).unwrap();
    });
    // Matching structure alone never creates semantic membership.
    let unclaimed = graph.clone().compile().unwrap();
    let error = snapshot
        .bind::<ProviderAction>("model", &required, &unclaimed)
        .err()
        .unwrap();
    assert_eq!(error.code, Code::SchemaRejected);
    let context = error.context.as_ref().unwrap();
    assert_eq!(context.plugin, "provider");
    assert_eq!(context.phase, phenix_plugin::CallbackPhase::Bind);
    assert_eq!(context.contribution.as_deref(), Some("model"));
    let source = std::error::Error::source(&error)
        .unwrap()
        .downcast_ref::<phenix_plugin::Failure>()
        .unwrap();
    assert!(
        std::error::Error::source(source)
            .unwrap()
            .is::<phenix_graph::InterfaceError>()
    );
    graph.implements("provider", &required).unwrap();
    let compiled = graph.compile().unwrap();
    let action = snapshot
        .bind::<ProviderAction>("model", &required, &compiled)
        .unwrap();
    assert_eq!(action.increment, 41);
    // A same-ID request with a dishonest definition must also be rejected.
    assert!(
        snapshot
            .bind::<ProviderAction>("model", &contract(DeclaredValue::wire::<i64>()), &compiled)
            .is_err()
    );
    let missing = plugins.reload(&key, Some("missing".into())).unwrap();
    assert_eq!(
        missing
            .bind::<ProviderAction>("model", &required, &compiled)
            .err()
            .unwrap()
            .code,
        Code::MissingBinding
    );
    let no_locator = plugins.reload(&key, None).unwrap();
    assert!(
        no_locator
            .bind::<ProviderAction>("model", &required, &compiled)
            .is_err()
    );
    plugins.unload(&key).unwrap();
    assert_eq!(
        snapshot
            .bind::<ProviderAction>("model", &required, &compiled)
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
    let required = contract(DeclaredValue::local::<i64>());
    let mut module = GraphBuilder::new("service");
    let node = module.node("implementation", |x: i64| x).unwrap();
    module
        .expose_input_field(
            "run",
            phenix_graph::SlotField::<(), i64>::new("value"),
            node.inlet(),
        )
        .unwrap();
    module.expose_output("value", node.output()).unwrap();
    module.implements_public(&required).unwrap();
    let definition = module.module();
    let mut graph = GraphBuilder::new("host");
    graph.with_origin(snapshot.graph_origin(), |graph| {
        graph.instance("service", &definition).unwrap();
    });
    let compiled = graph.compile().unwrap();
    snapshot
        .bind::<ProviderAction>("model", &required, &compiled)
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
        let required = contract(DeclaredValue::local::<i64>());
        let mut graph = GraphBuilder::new("host");
        graph.with_origin(old.graph_origin(), |graph| {
            let action = old.get::<ProviderAction>("model").unwrap();
            if module {
                let mut child = GraphBuilder::new("implementation");
                let node = child
                    .node("private", move |x: i64| x + action.increment)
                    .unwrap();
                child
                    .expose_input_field(
                        "run",
                        phenix_graph::SlotField::<(), i64>::new("value"),
                        node.inlet(),
                    )
                    .unwrap();
                child.expose_output("value", node.output()).unwrap();
                child.implements_public(&required).unwrap();
                graph.instance("service", &child.module()).unwrap();
            } else {
                graph
                    .node("service", move |x: i64| x + action.increment)
                    .unwrap();
                graph.implements("service", &required).unwrap();
            }
        });
        let compiled = graph.compile().unwrap();
        old.bind::<ProviderAction>("model", &required, &compiled)
            .unwrap();
        let staged = plugins.prepare_reload(&key, 2).unwrap();
        let rejected_candidate = staged.candidate().unwrap();
        drop(staged);
        let current = plugins.reload(&key, 3).unwrap();
        // A rejected candidate and the committed replacement can have the same
        // numeric revision. Only the opaque allocation proves the generation.
        assert_eq!(rejected_candidate.revision(), current.revision());
        let sibling = catalog.instance().load(&key, 1).unwrap();
        assert_eq!(sibling.revision(), old.revision());
        for foreign in [&rejected_candidate, &current, &sibling] {
            let error = foreign
                .bind::<ProviderAction>("model", &required, &compiled)
                .err()
                .unwrap();
            assert_eq!(error.code, Code::Denied);
            assert!(error.message.contains("origin mismatch"), "{error}");
        }
        plugins.unload(&key).unwrap();
        assert_eq!(
            old.bind::<ProviderAction>("model", &required, &compiled)
                .unwrap()
                .increment,
            1
        );
    }
}
