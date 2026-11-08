//! An absent *value* is different from a produced value with no recipients.
//! All three cases go through the production compiler and executor.
use phenix_graph::testing::{ExpectWave, Fixture};
use phenix_graph::{GraphBuilder, Output, Value};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn no_value_and_zero_recipients_have_distinct_publication_reports() {
    let mut graph = GraphBuilder::new("presence");
    let sink = graph.node("sink", |x: i64| x + 1).unwrap();
    let source = graph
        .node("source", |x: i64| {
            if x < 0 {
                Output::none()
            } else if x == 0 {
                Output::to_all([], x)
            } else {
                Output::all(x)
            }
        })
        .unwrap();
    graph.connect(source.output(), sink.inlet()).unwrap();
    let request = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("go", [request]).unwrap();
    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    let from = fixture.output("source", "value");
    let to = fixture.input("sink", "run", "value");

    // Successful activation, but no value was produced.
    fixture
        .fire("go", vec![Value::local(-1_i64)])
        .unwrap()
        .expect_success()
        .expect_publication_count(from, 0)
        .expect_deliveries_from(from, &[])
        .expect_invocations(fixture.node_id("sink"), 0);

    // Value exists, but zero existing edges were selected.
    fixture
        .fire("go", vec![Value::local(0_i64)])
        .unwrap()
        .expect_success()
        .expect_publication_count(from, 1)
        .expect_local_at(from, &0_i64)
        .expect_deliveries_from(from, &[])
        .expect_invocations(fixture.node_id("sink"), 0);

    // Value exists and broadcast is the default selection.
    fixture
        .fire("go", vec![Value::local(4_i64)])
        .unwrap()
        .expect_success()
        .expect_publication_count(from, 1)
        .expect_local_at(from, &4_i64)
        .expect_deliveries_from(from, &[to])
        .expect_invocations(fixture.node_id("sink"), 1);
}

#[test]
fn output_none_skips_payload_creation_but_keeps_typed_output_port() {
    let count = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&count);
    let mut graph = GraphBuilder::new("no-payload-allocation");
    let source = graph
        .node("source", move |x: i64| {
            if x < 0 {
                Output::none()
            } else {
                observed.fetch_add(1, Ordering::SeqCst);
                Output::all(String::from("produced"))
            }
        })
        .unwrap();
    let sink = graph.node("sink", |s: String| s.len()).unwrap();
    graph.connect(source.output(), sink.inlet()).unwrap();
    let request = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("go", [request]).unwrap();
    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    let from = fixture.output("source", "value");

    fixture
        .fire("go", vec![Value::local(-1_i64)])
        .unwrap()
        .expect_success()
        .expect_publication_count(from, 0);
    assert_eq!(count.load(Ordering::SeqCst), 0);

    fixture
        .fire("go", vec![Value::local(1_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(from, &String::from("produced"));
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn output_none_does_not_mean_option_none_payload() {
    let mut graph = GraphBuilder::new("optional-domain-value");
    let source = graph
        .node("source", |x: i64| {
            if x == 0 {
                Output::none()
            } else {
                Output::all((x > 0).then_some(x))
            }
        })
        .unwrap();
    let sink = graph
        .node("sink", |x: Option<i64>| x.unwrap_or(-7))
        .unwrap();
    graph.connect(source.output(), sink.inlet()).unwrap();
    let request = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("go", [request]).unwrap();
    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    let from = fixture.output("source", "value");
    let target = fixture.input("sink", "run", "value");

    // The node returns an Option<i64> payload containing None, so it publishes.
    fixture
        .fire("go", vec![Value::local(-1_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(from, &None::<i64>)
        .expect_deliveries_from(from, &[target]);

    // The node returns Output::None, so it publishes nothing at all.
    fixture
        .fire("go", vec![Value::local(0_i64)])
        .unwrap()
        .expect_success()
        .expect_publication_count(from, 0)
        .expect_deliveries_from(from, &[]);
}

#[test]
fn standalone_none_can_use_explicit_unit_return_type() {
    let mut graph = GraphBuilder::new("unit-no-value");
    let quiet = graph
        .node("quiet", |_: i64| -> Output<()> { Output::none() })
        .unwrap();
    let sink = graph.node("sink", |_: ()| 42_i64).unwrap();
    graph.connect(quiet.output(), sink.inlet()).unwrap();
    let request = graph.expose_input("request", quiet.inlet()).unwrap();
    graph.trigger("go", [request]).unwrap();
    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    fixture
        .fire("go", vec![Value::local(1_i64)])
        .unwrap()
        .expect_success()
        .expect_publication_count(fixture.output("quiet", "value"), 0)
        .expect_invocations(fixture.node_id("sink"), 0);
}
