//! Round 9: test output routing through the generic node return contract.
//! No output-port selector or alternative patch-routing mechanism exists.
use phenix_graph::testing::{ExpectWave, Fixture};
use phenix_graph::{GraphBuilder, Output, Value};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn one_tuple_payload_can_be_broadcast_to_multiple_independent_consumers() {
    let mut graph = GraphBuilder::new("tuple-fanout");
    let source = graph.node("source", |x: i64| (x > 0, x.abs())).unwrap();
    let flag = graph
        .node("flag", |(positive, _): (bool, i64)| positive)
        .unwrap();
    let magnitude = graph
        .node("magnitude", |(_, value): (bool, i64)| value * 2)
        .unwrap();
    graph.connect(source.output(), flag.inlet()).unwrap();
    graph.connect(source.output(), magnitude.inlet()).unwrap();
    let request = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("go", [request]).unwrap();
    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    fixture
        .fire("go", vec![Value::local(-7_i64)])
        .unwrap()
        .expect_success()
        .expect_deliveries_from(
            fixture.output("source", "value"),
            &[
                fixture.input("flag", "run", "value"),
                fixture.input("magnitude", "run", "value"),
            ],
        )
        .expect_local_at(fixture.output("flag", "value"), &false)
        .expect_local_at(fixture.output("magnitude", "value"), &14_i64);
}

#[test]
fn option_and_result_are_payloads_even_when_wrapped_in_output() {
    let mut graph = GraphBuilder::new("data-not-control");
    let source = graph
        .node("source", |x: i64| {
            Output::all(if x > 0 {
                Some(Ok::<i64, &'static str>(x))
            } else {
                Some(Err::<i64, &'static str>("negative"))
            })
        })
        .unwrap();
    let sink = graph
        .node("sink", |data: Option<Result<i64, &'static str>>| {
            data.map(|result| result.is_ok()).unwrap_or(false)
        })
        .unwrap();
    graph.connect(source.output(), sink.inlet()).unwrap();
    let request = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("go", [request]).unwrap();
    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    fixture
        .fire("go", vec![Value::local(-2_i64)])
        .unwrap()
        .expect_success()
        .expect_deliveries_from(
            fixture.output("source", "value"),
            &[fixture.input("sink", "run", "value")],
        )
        .expect_local_at(fixture.output("sink", "value"), &false);
}

#[test]
fn empty_prefix_never_accidentally_broadcasts() {
    let mut graph = GraphBuilder::new("empty-prefix");
    let source = graph
        .node("source", |x: i64| Output::to_prefix("", x))
        .unwrap();
    let sink = graph.node("sink", |x: i64| x).unwrap();
    graph.connect(source.output(), sink.inlet()).unwrap();
    let request = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("go", [request]).unwrap();
    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    fixture
        .fire("go", vec![Value::local(42_i64)])
        .unwrap()
        .expect_success()
        .expect_publication_count(fixture.output("source", "value"), 1)
        .expect_deliveries_from(fixture.output("source", "value"), &[])
        .expect_invocations(fixture.node_id("sink"), 0);
}

#[test]
fn filtering_observes_only_compiled_connections_and_cannot_forge_targets() {
    let mut graph = GraphBuilder::new("filtered-candidates");
    let sink = graph.node("sink", |x: i64| x).unwrap();
    let unrelated = graph.node("unrelated", |x: i64| x).unwrap();
    let visited = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&visited);
    let source = graph
        .node("source", move |x: i64| {
            let counter = Arc::clone(&counter);
            Output::to_filter(
                move |destination| {
                    counter.fetch_add(1, Ordering::SeqCst);
                    destination.path() == "unrelated"
                },
                x,
            )
        })
        .unwrap();
    graph.connect(source.output(), sink.inlet()).unwrap();
    let request = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("go", [request]).unwrap();
    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    fixture
        .fire("go", vec![Value::local(4_i64)])
        .unwrap()
        .expect_success()
        .expect_deliveries_from(fixture.output("source", "value"), &[])
        .expect_invocations(fixture.node_id("sink"), 0)
        .expect_invocations(fixture.node_id("unrelated"), 0);
    assert_eq!(
        visited.load(Ordering::SeqCst),
        1,
        "only one compiled outgoing edge may be presented to the filter"
    );
    let _ = unrelated;
}
