//! Public API contract: one set of names for typed and symbolic addressing,
//! one patch contribution type, one planner, and one compiled execution path.
use phenix_graph::ReportRetention;
use phenix_graph::Deployment;
use phenix_graph::{GraphBuilder, PatchBuilder, PatchPlanner,
    GraphAssembly, Store, Value};

#[test]
fn typed_and_symbolic_connections_share_one_canonical_graph_builder_api() {
    let mut graph = GraphBuilder::new("single-addressing-api");
    let a = graph.node("a", |x: i64| x + 1).unwrap();
    let b = graph.node("b", |x: i64| x * 2).unwrap();

    // Both type-safe and symbolic forms are one method: connect.
    graph.connect(a.output(), b.inlet()).unwrap();
    assert!(graph.connect("a", "b").err().unwrap().contains("duplicate"));
    graph.disconnect("a.out.value", "b.in.run.value").unwrap();
    assert!(graph.disconnect("a", "b").is_err());
    graph.connect("a", "b").unwrap();

    // Both endpoint forms are one expose_input/expose_output API.
    let ingress = graph.expose_input("input", a.inlet()).unwrap();
    graph.expose_output("output", "b").unwrap();
    assert!(graph.expose_input("absent", "not-a-node").is_err());

    // Both forms are one trigger method. Failed admission is not registered.
    assert!(graph.trigger("broken", ["missing"]).is_err());
    let trigger = graph.trigger("go", [ingress]).unwrap();
    let report = Deployment::new(graph.compile().unwrap()).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
        .fire(trigger.get(), vec![Value::local(20_i64)]).unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.publications().last().unwrap().borrow::<i64>(), Some(&42));
}

#[test]
fn patches_use_one_registration_and_compiler_path() {
    let mut planner = PatchPlanner::new();
    let mut base = PatchBuilder::new("base");
    base.node("a", |x: i64| x + 1)
        .node("b", |x: i64| x * 2)
        .expose_input("input", "a")
        .expose_output("output", "b")
        .trigger("go", &["input"]);
    base.connect("a", "b").unwrap();
    planner.add_patch(base).unwrap();

    let (compiled, resolution) = planner.build("one-compiler").unwrap();
    assert_eq!(resolution.selected, ["base"]);
    let report = Deployment::new(compiled.graph).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
        .fire(1, vec![Value::local(20_i64)]).unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.publications().last().unwrap().borrow::<i64>(), Some(&42));
}
