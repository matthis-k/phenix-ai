//! A consumer builds a graph from a plugin contribution after validating
//! the graph's public input/output bounds. Leases pin exact plugin revisions.
use phenix_graph::{DeclaredValue, GraphBuilder, InterfaceBounds};
use phenix_plugin::{Contribution, Failure, PluginCatalog, PluginInstance, Registrar};

struct ProviderAction {
    id: String,
    increment: i64,
    interface: InterfaceBounds,
}
impl Contribution for ProviderAction {
    const KIND: &'static str = "graph.node";
    fn id(&self) -> &str {
        &self.id
    }
    fn graph_bounds(&self) -> Option<&InterfaceBounds> {
        Some(&self.interface)
    }
}
struct Provider(i64);
fn interface(input: DeclaredValue) -> InterfaceBounds {
    InterfaceBounds::new("plugina/provider@1")
        .unwrap()
        .input_slot("run", "value", input)
        .unwrap()
        .output("value", DeclaredValue::local::<i64>())
        .unwrap()
}
impl PluginInstance for Provider {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        into.add(ProviderAction {
            id: "model".into(),
            increment: self.0,
            interface: interface(DeclaredValue::local::<i64>()),
        })
    }
}
#[test]
fn provider_implementation_is_pinned_while_graph_compiles() {
    let mut catalog = PluginCatalog::new();
    let key = catalog
        .register("provider", |value: i64| Ok(Provider(value)))
        .unwrap();
    let mut plugins = catalog.instance();
    let original = plugins.load(&key, 41).unwrap();
    let required = interface(DeclaredValue::local::<i64>());
    // A consumer need not import the provider's Rust implementation type
    // merely to validate its graph contract.
    original
        .check_bounds("graph.node", "model", &required)
        .unwrap();
    assert!(
        original
            .check_bounds(
                "graph.node",
                "model",
                &interface(DeclaredValue::wire::<i64>())
            )
            .is_err()
    );
    let action = original.bind::<ProviderAction>("model", &required).unwrap();

    let mut graph = GraphBuilder::new("agent");
    let node = graph
        .node("provider", move |x: i64| x + action.increment)
        .unwrap();
    let ingress = graph.expose_input("request", node.inlet()).unwrap();
    graph.expose_output("response", node.output()).unwrap();
    graph.trigger("run", [ingress]).unwrap();
    graph.compile().unwrap();

    plugins.reload(&key, 42).unwrap();
    plugins.unload(&key).unwrap();
    assert_eq!(
        original
            .bind::<ProviderAction>("model", &required)
            .unwrap()
            .increment,
        41
    );
    assert!(plugins.snapshot(&key).is_err());
}
