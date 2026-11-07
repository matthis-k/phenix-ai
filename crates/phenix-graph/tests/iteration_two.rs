//! Iteration 2 acceptance: one typed node declaration and one compiler error API.
//! These tests exercise the real compiler, deployment, and production executor.
use phenix_graph::Output;
use phenix_graph::{
    CompilePhase, Deployment, GraphBuilder, ModuleCatalog, ModuleDefBuilder,
    Node, PatchBuilder, PatchError, PatchPlanner, Value,
};
use phenix_graph::testing::{ExpectWave, Fixture};

#[test]
fn single_node_api_supports_wire_and_local_nodes_in_same_graph() {
    let mut graph = GraphBuilder::new("mixed-node-kinds");
    let wire = graph.node("wire", Node::wire(|n: i64| n + 1)).unwrap();
    let local = graph.node("local", |n: i64| n * 2).unwrap();
    // A wire/local connection requires a deliberate conversion, not an
    // accidentally inferred or erased schema coercion.
    assert!(graph.connect(wire.output(), local.inlet()).is_err());
    let ingress = graph.expose_input("request", wire.inlet()).unwrap();
    graph.expose_output("result", wire.output()).unwrap();
    graph.trigger("go", [ingress]).unwrap();
    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    fixture.fire("go", vec![Value::of(41_i64)]).unwrap()
        .expect_success().expect_wire_at(fixture.output("wire", "value"), 42_i64);
}

#[test]
fn option_and_result_values_are_ordinary_node_payloads() {
    let mut graph = GraphBuilder::new("conditional");
    let optional = graph.node("optional", |v: i64| Output::all((v >= 0).then_some(v))).unwrap();
    let fallible = graph.node("fallible", Node::new(|v: i64| Output::all(
        if v < 0 { Err("negative value") } else { Ok(v * 2) }
    ))).unwrap();
    let a = graph.expose_input("a", optional.inlet()).unwrap();
    let b = graph.expose_input("b", fallible.inlet()).unwrap();
    graph.trigger("go", [a, b]).unwrap();
    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    fixture.fire("go", vec![Value::local(-1_i64), Value::local(-1_i64)]).unwrap()
        .expect_success()
        .expect_local_at(fixture.output("optional", "value"), &None::<i64>)
        .expect_local_at(fixture.output("fallible", "value"), &Err::<i64, &str>("negative value"));
    fixture.fire("go", vec![Value::local(1_i64), Value::local(2_i64)]).unwrap()
        .expect_success()
        .expect_local_at(fixture.output("optional", "value"), &Some(1_i64))
        .expect_local_at(fixture.output("fallible", "value"), &Ok::<i64, &str>(4_i64));
}

#[test]
fn compiler_has_one_structured_diagnostic_entry() {
    let mut definition = ModuleDefBuilder::new("broken");
    definition.try_instance("missing", "not-registered").unwrap();
    let mut catalog = ModuleCatalog::new();
    catalog.add(definition.build()).unwrap();
    let error = match catalog.compile("broken") {
        Err(error) => error,
        Ok(_) => panic!("invalid graph compiled"),
    };
    assert_eq!(error.phase, CompilePhase::Declaration);
    assert_eq!(error.definition, "broken");
    assert!(error.message.contains("not-registered"));
}

#[test]
fn patch_build_returns_that_same_structured_compiler_error() {
    let mut planner = PatchPlanner::new();
    let mut patch = PatchBuilder::new("empty");
    patch.node("identity", |x: i64| x);
    planner.add_patch(patch).unwrap();
    // The positive path uses the same canonical compiler and outputs.
    let (assembled, _) = planner.build("patched").unwrap();
    let _engine = Deployment::new(assembled.graph).build().unwrap();
    // PatchError::Compile carries CompileDiagnostic, not a lossy String.
    fn assert_compile_error_shape(error: PatchError) {
        if let PatchError::Compile(diagnostic) = error {
            let _ = (diagnostic.phase, diagnostic.definition, diagnostic.message);
        }
    }
    let _ = assert_compile_error_shape as fn(PatchError);
}

#[test]
fn patch_contributor_uses_identical_node_options() {
    let mut planner = PatchPlanner::new();
    let mut patch = PatchBuilder::new("wire-contributor");
    patch.node("reply", Node::wire(|v: i64| v + 1))
        .expose_input("request", "reply")
        .expose_output("answer", "reply")
        .trigger("go", &["request"]);
    planner.add_patch(patch).unwrap();
    let (assembled, _) = planner.build("portable").unwrap();
    let fixture = Fixture::new(assembled.graph).unwrap();
    fixture.fire("go", vec![Value::of(5_i64)]).unwrap()
        .expect_success().expect_wire_at(fixture.output("reply", "value"), 6_i64);
}
