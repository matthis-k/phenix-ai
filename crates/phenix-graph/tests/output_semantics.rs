//! Output<T> is only a payload plus a routing choice. Option<T> and Result<T,E>
//! are data, whether the node returns them directly or wraps them in Output.
//! These tests use the actual graph compiler and production executor.
use phenix_graph::testing::{ExpectWave, Fixture};
use phenix_graph::{
    GraphBuilder, InputSlot, InvocationCtx, NodeTemplate, Output, OutputSlot, Port, Value,
    WaveError, emit_local,
};

fn fixture(graph: GraphBuilder) -> Fixture {
    Fixture::new(graph.compile().unwrap()).unwrap()
}

#[test]
fn plain_option_is_data_even_when_none() {
    let mut graph = GraphBuilder::new("option-data");
    let source = graph.node("source", |x: i64| (x > 0).then_some(x)).unwrap();
    let sink = graph
        .node("sink", |value: Option<i64>| value.unwrap_or(-1))
        .unwrap();
    graph.connect(source.output(), sink.inlet()).unwrap();
    let request = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("run", [request]).unwrap();
    let f = fixture(graph);
    f.fire("run", vec![Value::local(-10_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(f.output("source", "value"), &None::<i64>)
        .expect_deliveries_from(
            f.output("source", "value"),
            &[f.input("sink", "run", "value")],
        )
        .expect_local_at(f.output("sink", "value"), &-1_i64);
}

#[test]
fn plain_result_is_data_even_when_err() {
    let mut graph = GraphBuilder::new("result-data");
    let source = graph
        .node("source", |x: i64| -> Result<i64, &'static str> {
            if x > 0 {
                Ok(x)
            } else {
                Err("domain rejection")
            }
        })
        .unwrap();
    let sink = graph
        .node("sink", |r: Result<i64, &'static str>| r.unwrap_or(-7))
        .unwrap();
    graph.connect(source.output(), sink.inlet()).unwrap();
    let request = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("run", [request]).unwrap();
    let f = fixture(graph);
    f.fire("run", vec![Value::local(-2_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(
            f.output("source", "value"),
            &Err::<i64, &str>("domain rejection"),
        )
        .expect_deliveries_from(
            f.output("source", "value"),
            &[f.input("sink", "run", "value")],
        )
        .expect_local_at(f.output("sink", "value"), &-7_i64);
}

#[test]
fn output_all_preserves_option_payload_and_broadcasts_none() {
    let mut graph = GraphBuilder::new("explicit-option-data");
    let source = graph
        .node("source", |x: i64| Output::all((x > 0).then_some(x)))
        .unwrap();
    let sink = graph
        .node("sink", |value: Option<i64>| value.unwrap_or(-1) * 2)
        .unwrap();
    graph.connect(source.output(), sink.inlet()).unwrap();
    let request = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("run", [request]).unwrap();
    let f = fixture(graph);
    let dest = f.input("sink", "run", "value");
    f.fire("run", vec![Value::local(-1_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(f.output("source", "value"), &None::<i64>)
        .expect_deliveries_from(f.output("source", "value"), &[dest])
        .expect_local_at(f.output("sink", "value"), &-2_i64);
    f.fire("run", vec![Value::local(5_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(f.output("source", "value"), &Some(5_i64))
        .expect_deliveries_from(f.output("source", "value"), &[dest])
        .expect_local_at(f.output("sink", "value"), &10_i64);
}

#[test]
fn output_to_routes_result_without_interpreting_err() {
    let mut graph = GraphBuilder::new("directed-result-data");
    let left = graph
        .node("left", |r: Result<i64, &'static str>| r.unwrap_or(-100))
        .unwrap();
    let right = graph
        .node("right", |r: Result<i64, &'static str>| r.unwrap_or(-200))
        .unwrap();
    let (l, r) = (left.inlet(), right.inlet());
    let source = graph
        .node("source", move |x: i64| {
            let left_wins = x >= 0; // Routing can use intermediate computations.
            let value = if left_wins {
                Ok(x * 2)
            } else {
                Err("domain rejection")
            };
            Output::to(if left_wins { &l } else { &r }, value)
        })
        .unwrap();
    graph.connect(source.output(), left.inlet()).unwrap();
    graph.connect(source.output(), right.inlet()).unwrap();
    let request = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("run", [request]).unwrap();
    let f = fixture(graph);
    f.fire("run", vec![Value::local(-3_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(
            f.output("source", "value"),
            &Err::<i64, &str>("domain rejection"),
        )
        .expect_deliveries_from(
            f.output("source", "value"),
            &[f.input("right", "run", "value")],
        )
        .expect_local_at(f.output("right", "value"), &-200_i64);
    f.fire("run", vec![Value::local(4_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(f.output("source", "value"), &Ok::<i64, &str>(8_i64))
        .expect_deliveries_from(
            f.output("source", "value"),
            &[f.input("left", "run", "value")],
        )
        .expect_local_at(f.output("left", "value"), &8_i64);
}

#[test]
fn output_none_carries_value_and_selects_zero_edges() {
    let mut graph = GraphBuilder::new("zero-destinations");
    let source = graph
        .node("source", |x: i64| {
            let payload = (x > 0).then_some(x);
            if x == 0 {
                Output::to_all([], payload)
            } else {
                Output::all(payload)
            }
        })
        .unwrap();
    let sink = graph
        .node("sink", |value: Option<i64>| value.unwrap_or(-1))
        .unwrap();
    graph.connect(source.output(), sink.inlet()).unwrap();
    let request = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("run", [request]).unwrap();
    let f = fixture(graph);
    f.fire("run", vec![Value::local(0_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(f.output("source", "value"), &None::<i64>)
        .expect_publication_count(f.output("source", "value"), 1)
        .expect_deliveries_from(f.output("source", "value"), &[])
        .expect_publication_count(f.output("sink", "value"), 0);
    f.fire("run", vec![Value::local(2_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(f.output("source", "value"), &Some(2_i64))
        .expect_deliveries_from(
            f.output("source", "value"),
            &[f.input("sink", "run", "value")],
        )
        .expect_local_at(f.output("sink", "value"), &2_i64);
}

#[test]
fn node_action_failure_is_independent_of_payload_result_type() {
    let mut graph = GraphBuilder::new("node-errors");
    graph
        .add(
            "validate",
            NodeTemplate::new(
                0,
                vec![Port::all(0, vec![InputSlot::local::<i64>(0).named("value")]).named("run")],
                vec![OutputSlot::local::<i64>(0).named("value")],
                |mut ctx: InvocationCtx| {
                    let x = ctx.take_local::<i64>(0)?;
                    if x < 0 {
                        Err("invalid execution".into())
                    } else {
                        Ok(vec![emit_local(0, x + 1)])
                    }
                },
            ),
        )
        .unwrap();
    let input = graph
        .local_input::<i64>("validate", "run", "value")
        .unwrap();
    let request = graph.expose_input("request", input).unwrap();
    graph.trigger("run", [request]).unwrap();
    let f = fixture(graph);
    f.fire("run", vec![Value::local(-1_i64)]).unwrap()
        .expect_error(|e| matches!(e, WaveError::NodeFailed(_, reason) if reason.contains("invalid execution")))
        .expect_publication_count(f.output("validate", "value"), 0);
    f.fire("run", vec![Value::local(4_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(f.output("validate", "value"), &5_i64);
}
