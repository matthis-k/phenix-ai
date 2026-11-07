//! Iteration 5: delivered edges, output retention, and fan-out atomicity.
//! Tests use the real compiler, deployment and executor, not mock scheduling.
use std::sync::Arc;

use phenix_graph::{
    Deployment, EdgeAdapter, GraphBuilder, Output, ReportRetention, Value, WaveError,
};
use phenix_graph::testing::{ExpectWave, Fixture};

#[test]
fn selective_routing_reports_only_committed_targets() {
    let mut graph = GraphBuilder::new("selected-deliveries");
    let left = graph.node("left", |x: i64| x * 2).unwrap();
    let right = graph.node("right", |x: i64| x * 3).unwrap();
    let selected = left.inlet();
    let source = graph.node("source", move |x: i64| Output::to(&selected, x + 1)).unwrap();
    graph.connect(source.output(), left.inlet()).unwrap();
    graph.connect(source.output(), right.inlet()).unwrap();
    let input = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("go", [input]).unwrap();

    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    let output = fixture.output("source", "value");
    let left = fixture.input("left", "run", "value");
    fixture.fire("go", vec![Value::local(4_i64)]).unwrap()
        .expect_success()
        .expect_publication_count(output, 1)
        .expect_deliveries_from(output, &[left])
        .expect_invocations(fixture.node_id("left"), 1)
        .expect_invocations(fixture.node_id("right"), 0);
}

#[test]
fn broadcast_still_reports_every_actual_edge() {
    let mut graph = GraphBuilder::new("broadcast-deliveries");
    let source = graph.node("source", |x: i64| x + 1).unwrap();
    let left = graph.node("left", |x: i64| x * 2).unwrap();
    let right = graph.node("right", |x: i64| x * 3).unwrap();
    graph.connect(source.output(), left.inlet()).unwrap();
    graph.connect(source.output(), right.inlet()).unwrap();
    let input = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("go", [input]).unwrap();

    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    fixture.fire("go", vec![Value::local(4_i64)]).unwrap()
        .expect_success()
        .expect_deliveries_from(fixture.output("source", "value"), &[
            fixture.input("left", "run", "value"),
            fixture.input("right", "run", "value"),
        ]);
}

#[test]
fn routed_output_with_no_selected_edges_is_terminal_for_that_wave() {
    let mut graph = GraphBuilder::new("routed-terminal");
    let source = graph.node("source", |x: i64| Output::to_all([], x + 1)).unwrap();
    let sink = graph.node("sink", |x: i64| x * 3).unwrap();
    graph.connect(source.output(), sink.inlet()).unwrap();
    let input = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("go", [input]).unwrap();

    // Default deployment retention is TerminalOutputs; the source has a
    // structural outgoing edge but does not deliver along it in this wave.
    let fixture = Fixture::from_deployment(Deployment::new(graph.compile().unwrap())).unwrap();
    let source = fixture.output("source", "value");
    fixture.fire("go", vec![Value::local(9_i64)]).unwrap()
        .expect_success()
        .expect_deliveries_from(source, &[])
        .expect_local_at(source, &10_i64)
        .expect_invocations(fixture.node_id("sink"), 0);
}

#[test]
fn invalid_selected_edge_cannot_publish_any_delivery() {
    let mut graph = GraphBuilder::new("invalid-selection");
    let sink = graph.node("sink", |x: i64| x).unwrap();
    let disconnected = graph.node("disconnected", |x: i64| x).unwrap();
    let target = disconnected.inlet();
    let source = graph.node("source", move |x: i64| Output::to(&target, x)).unwrap();
    graph.connect(source.output(), sink.inlet()).unwrap();
    let input = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("go", [input]).unwrap();
    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    let output = fixture.output("source", "value");
    let report = fixture.fire("go", vec![Value::local(4_i64)]).unwrap();
    report.expect_error(|error| matches!(error, WaveError::OutputRoutingFailed { .. }))
        .expect_deliveries_from(output, &[])
        .expect_publication_count(output, 0)
        .expect_invocations(fixture.node_id("sink"), 0);
}

struct RejectEdge;
impl EdgeAdapter for RejectEdge {
    fn prepare(&self, _value: &Value) -> Result<Value, String> {
        Err("deliberately rejected".into())
    }
}

#[test]
fn rejected_second_edge_does_not_commit_partially_prepared_fanout() {
    let mut graph = GraphBuilder::new("atomic-deliveries");
    let source = graph.node("source", |x: i64| x + 1).unwrap();
    let left = graph.node("left", |x: i64| x * 2).unwrap();
    let right = graph.node("right", |x: i64| x * 3).unwrap();
    graph.connect(source.output(), left.inlet()).unwrap();
    graph.connect(source.output(), right.inlet()).unwrap();
    let input = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("go", [input]).unwrap();

    let mut deployment = Deployment::new(graph.compile().unwrap());
    deployment.edge_adapter("source", "value", "right", "run", "value", Arc::new(RejectEdge))
        .unwrap();
    let fixture = Fixture::from_deployment(deployment).unwrap();
    let from = fixture.output("source", "value");
    fixture.fire("go", vec![Value::local(4_i64)]).unwrap()
        .expect_error(|error| matches!(error, WaveError::EdgeFailed { .. }))
        .expect_deliveries_from(from, &[])
        .expect_publication_count(from, 0)
        .expect_invocations(fixture.node_id("left"), 0)
        .expect_invocations(fixture.node_id("right"), 0);
}

#[test]
fn metadata_only_reports_deliveries_without_retaining_payloads() {
    let mut graph = GraphBuilder::new("metadata-deliveries");
    let source = graph.node("source", |x: i64| x + 1).unwrap();
    let sink = graph.node("sink", |x: i64| x + 2).unwrap();
    graph.connect(source.output(), sink.inlet()).unwrap();
    let input = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("go", [input]).unwrap();
    let fixture = Fixture::from_deployment(Deployment::new(graph.compile().unwrap())
        .report_retention(ReportRetention::MetadataOnly)).unwrap();
    let report = fixture.fire("go", vec![Value::local(4_i64)]).unwrap();
    report.expect_success().expect_deliveries_from(
        fixture.output("source", "value"), &[fixture.input("sink", "run", "value")]);
    assert_eq!(report.publications().count(), 0);
    assert_eq!(report.publication_events().len(), 2);
}

#[test]
fn invalid_routing_wave_does_not_poison_later_valid_wave() {
    let mut graph = GraphBuilder::new("wave-isolation");
    let sink = graph.node("sink", |x: i64| x + 1).unwrap();
    let disconnected = graph.node("disconnected", |x: i64| x).unwrap();
    let valid = sink.inlet();
    let invalid = disconnected.inlet();
    let source = graph.node("source", move |x: i64| {
        Output::to(if x < 0 { &invalid } else { &valid }, x)
    }).unwrap();
    graph.connect(source.output(), sink.inlet()).unwrap();
    let input = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("go", [input]).unwrap();
    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    let from = fixture.output("source", "value");
    let to = fixture.input("sink", "run", "value");
    fixture.fire("go", vec![Value::local(-1_i64)]).unwrap()
        .expect_error(|error| matches!(error, WaveError::OutputRoutingFailed { .. }))
        .expect_deliveries_from(from, &[]);
    fixture.fire("go", vec![Value::local(4_i64)]).unwrap()
        .expect_success()
        .expect_deliveries_from(from, &[to])
        .expect_local_at(fixture.output("sink", "value"), &5_i64);
}

#[test]
fn connection_patch_uses_one_validated_endpoint_grammar() {
    use phenix_graph::{ConnectionRoute, PatchBuilder, PatchError};
    let connection = ConnectionRoute::parse("source.out.value", "sink.in.run.value").unwrap();
    assert_eq!(connection.from.node, "source");
    assert_eq!(connection.from.port.as_deref(), Some("value"));
    let mut patch = PatchBuilder::new("connection-grammar");
    patch.connect("source.out.value", "sink.in.run.value").unwrap();
    for invalid in ["", "source.out.", "source.out.value.extra", "source.in.value"] {
        assert!(matches!(patch.connect(invalid, "sink.in.run.value"),
            Err(PatchError::InvalidEndpoint(_))),
            "unexpected accepted connection source: {invalid:?}");
    }
}
