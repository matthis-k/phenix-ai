//! Test support must execute the public graph API, not its own model.
use phenix_graph::{Deployment, GraphBuilder, Node, NodeTemplate, PatchBuilder, PatchPlanner, ReportRetention, Value};
use phenix_graph::Output;
use phenix_graph::testing::{ExpectWave, Fixture};

#[test]
fn a_fixture_runs_a_typed_graph_through_production_deployment() {
    let mut graph = GraphBuilder::new("first-wave");
    let first = graph.node("first", |x: i64| x + 1).unwrap();
    let second = graph.node("second", |x: i64| x * 2).unwrap();
    graph.connect(first.output(), second.inlet()).unwrap();
    let input = graph.expose_input("request", first.inlet()).unwrap();
    graph.expose_output("reply", second.output()).unwrap();
    let go = graph.trigger("go", [input]).unwrap();

    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    fixture.fire("go", vec![Value::local(20_i64)]).unwrap()
        .expect_success().expect_local_at(fixture.output("second", "value"), &42_i64);
}

#[test]
fn result_payloads_do_not_fail_waves() {
    let mut graph = GraphBuilder::new("failed-wave-recovery");
    let validate = graph.node("validate", Node::new(|x: i64| Output::all(
        if x < 0 { Err("negative request") } else { Ok(x + 1) }
    ))).unwrap();
    let input = graph.expose_input("request", validate.inlet()).unwrap();
    graph.expose_output("reply", validate.output()).unwrap();
    let trigger = graph.trigger("go", [input]).unwrap();

    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    fixture.fire("go", vec![Value::local(-1_i64)]).unwrap()
        .expect_success().expect_local_at(fixture.output("validate", "value"), &Err::<i64, &str>("negative request"));
    fixture.fire("go", vec![Value::local(4_i64)]).unwrap()
        .expect_success().expect_local_at(fixture.output("validate", "value"), &Ok::<i64, &str>(5_i64));
}

#[test]
fn failed_trigger_admission_does_not_start_a_wave() {
    let mut graph = GraphBuilder::new("typed-admission");
    let node = graph.node("pass", |x: i64| x).unwrap();
    let input = graph.expose_input("request", node.inlet()).unwrap();
    graph.expose_output("reply", node.output()).unwrap();
    let trigger = graph.trigger("go", [input]).unwrap();
    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();

    let error = fixture.fire("go", vec![Value::of(4_i64)]).unwrap_err();
    assert!(error.contains("type identity"), "{error}");
    fixture.fire("go", vec![Value::local(4_i64)]).unwrap()
        .expect_success().expect_local_at(fixture.output("pass", "value"), &4_i64);
}

#[test]
fn patch_plan_fixtures_use_the_same_executor_and_wire_decoding() {
    let mut planner = PatchPlanner::new();
    let mut first = PatchBuilder::new("first");
    first.add("a", NodeTemplate::function(|x: i64| x + 1))
        .add("b", NodeTemplate::function(|x: i64| x * 2))
        .expose_input("request", "a")
        .expose_output("reply", "b")
        .trigger("go", &["request"]);
    first.connect("a", "b").unwrap();
    planner.add_patch(first).unwrap();

    let (built, resolution) = planner.build("patch-fixture").unwrap();
    assert_eq!(resolution.selected, ["first"]);
    let fixture = Fixture::from_deployment(
        Deployment::new(built.graph).report_retention(ReportRetention::RetainPayloads),
    ).unwrap();
    fixture.fire(1, vec![Value::of(20_i64)]).unwrap()
        .expect_success().expect_wire_at(fixture.output("b", "value"), 42_i64);
}

#[test]
fn fixture_exposes_real_host_driven_sessions_without_a_test_scheduler() {
    let mut graph = GraphBuilder::new("host-drive");
    let node = graph.node("increment", |x: i64| x + 1).unwrap();
    let ingress = graph.expose_input("request", node.inlet()).unwrap();
    graph.expose_output("response", node.output()).unwrap();
    let trigger = graph.trigger("run", [ingress]).unwrap();
    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();

    let mut pending = fixture.begin("run", vec![Value::local(10_i64)]).unwrap();
    assert!(pending.next_ready().is_some());
    assert_eq!(pending.in_flight_count(), 1);
    assert_eq!(pending.cancel().len(), 1);
    pending.finish().unwrap().expect_cancelled();
    fixture.fire("run", vec![Value::local(10_i64)]).unwrap()
        .expect_success().expect_local_at(fixture.output("increment", "value"), &11_i64);
}
