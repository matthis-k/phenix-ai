//! Every typed node has an output schema; plain unit is a real value.
//! Producing unit with no recipients differs from returning `Output::none()`.
//! These tests run against the production graph compiler and executor.
use phenix_graph::testing::{ExpectWave, Fixture};
use phenix_graph::{
    AllSameCohort, Atomic, GraphBuilder, Node, NodeTemplate, Output, ReportRetention, TypedPort,
    Value,
};

#[test]
fn ordinary_unit_value_broadcasts_to_connected_consumer() {
    let mut graph = GraphBuilder::new("unit-broadcast");
    let source = graph.node("source", |_: i64| {}).unwrap();
    let sink = graph.node("sink", |_: ()| 42_i64).unwrap();
    graph.connect(source.output(), sink.inlet()).unwrap();
    let request = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("run", [request]).unwrap();

    let f = Fixture::new(graph.compile().unwrap()).unwrap();
    f.fire("run", vec![Value::local(1_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(f.output("source", "value"), &())
        .expect_deliveries_from(
            f.output("source", "value"),
            &[f.input("sink", "run", "value")],
        )
        .expect_local_at(f.output("sink", "value"), &42_i64);
}

#[test]
fn unit_with_empty_recipient_set_is_still_an_output_publication() {
    let mut graph = GraphBuilder::new("unit-no-recipient");
    let source = graph
        .node("source", |_: i64| Output::to_all([], ()))
        .unwrap();
    let sink = graph.node("sink", |_: ()| 42_i64).unwrap();
    graph.connect(source.output(), sink.inlet()).unwrap();
    let request = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("run", [request]).unwrap();

    let f = Fixture::new(graph.compile().unwrap()).unwrap();
    f.fire("run", vec![Value::local(1_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(f.output("source", "value"), &())
        .expect_deliveries_from(f.output("source", "value"), &[])
        .expect_invocations(f.node_id("sink"), 0);
}

#[test]
fn typed_unit_output_can_feed_a_wire_typed_consumer() {
    let mut graph = GraphBuilder::new("typed-unit-edge");
    let source = graph
        .add(
            "source",
            NodeTemplate::multi_port::<i64, (), _>(
                vec![TypedPort::new::<Atomic<i64>, _>(0, "run", AllSameCohort, |x| x).unwrap()],
                |_x, _ctx| Ok(()),
            ),
        )
        .unwrap();
    let sink = graph.node("sink", Node::wire(|_: ()| 42_i64)).unwrap();
    let wire_output = graph.wire_output::<()>("source", "value").unwrap();
    graph.connect(wire_output, sink.inlet()).unwrap();
    let wire_input = graph.wire_input::<i64>("source", "run", "value").unwrap();
    let request = graph.expose_input("request", wire_input).unwrap();
    graph.trigger("run", [request]).unwrap();

    let report = phenix_graph::Deployment::new(graph.compile().unwrap())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire("run", vec![Value::of(1_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.delivery_events().len(), 1);
    assert_eq!(report.publication_events().len(), 2);
    assert!(report.publications().any(|p| p.value::<()>() == Ok(())));
    assert!(report.publications().any(|p| p.value::<i64>() == Ok(42)));
}
