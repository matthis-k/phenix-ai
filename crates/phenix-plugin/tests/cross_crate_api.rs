//! A graph-side type from another crate is a typed, leased contribution.
use phenix_contract::Failure;
use phenix_plugin::{Contribution, PluginCatalog, PluginInstance, Registrar};

struct GraphPatch {
    id: String,
    payload: u64,
}
impl Contribution for GraphPatch {
    const KIND: &'static str = "graph.patch";
    fn id(&self) -> &str { &self.id }
}
struct Source(u64);
impl PluginInstance for Source {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        into.add(GraphPatch { id: "compute".to_owned(), payload: self.0 })
    }
}

#[test]
fn an_external_crate_receives_its_own_typed_leased_graph_contribution() {
    let mut catalog = PluginCatalog::new();
    let source = catalog.register("external", |v: u64| Ok(Source(v))).unwrap();
    let mut instance = catalog.instance();
    let old = instance.load(&source, 41_u64).unwrap();
    let graph_action = old.get::<GraphPatch>("compute").unwrap();
    instance.reload(&source, 42_u64).unwrap();
    instance.unload(&source).unwrap();
    assert_eq!(graph_action.payload, 41);
}
