//! One-value output dispatch. These tests use the production graph compiler,
//! executor and delivery report, never a mock scheduler.
use phenix_graph::testing::{ExpectWave, Fixture};
use phenix_graph::{GraphBuilder, Node, Output, Value, WaveError};

fn fixture(graph: GraphBuilder) -> Fixture {
    Fixture::new(graph.compile().unwrap()).unwrap()
}

#[test]
fn plain_return_broadcasts_to_all_existing_connections() {
    let mut g = GraphBuilder::new("plain");
    let source = g.node("source", Node::new(|x: i64| x + 1)).unwrap();
    let left = g.node("left", |x: i64| x * 2).unwrap();
    let right = g.node("right", |x: i64| x * 3).unwrap();
    g.connect(source.output(), left.inlet()).unwrap();
    g.connect(source.output(), right.inlet()).unwrap();
    let input = g.expose_input("request", source.inlet()).unwrap();
    g.trigger("go", [input]).unwrap();
    let f = fixture(g);
    f.fire("go", vec![Value::local(2_i64)])
        .unwrap()
        .expect_success()
        .expect_deliveries_from(
            f.output("source", "value"),
            &[
                f.input("left", "run", "value"),
                f.input("right", "run", "value"),
            ],
        );
}

#[test]
fn explicit_target_uses_intermediate_calculation_not_just_final_value() {
    let mut g = GraphBuilder::new("decision");
    let left = g.node("left", |x: i64| x).unwrap();
    let right = g.node("right", |x: i64| x).unwrap();
    let l = left.inlet();
    let r = right.inlet();
    let source = g
        .node(
            "source",
            Node::new(move |x: i64| {
                let score = x > 0; // Intermediate decision isn't part of returned data.
                let value = x.abs();
                Output::to(if score { &l } else { &r }, value)
            }),
        )
        .unwrap();
    g.connect(source.output(), left.inlet()).unwrap();
    g.connect(source.output(), right.inlet()).unwrap();
    let input = g.expose_input("request", source.inlet()).unwrap();
    g.trigger("go", [input]).unwrap();
    let f = fixture(g);
    f.fire("go", vec![Value::local(4_i64)])
        .unwrap()
        .expect_success()
        .expect_deliveries_from(
            f.output("source", "value"),
            &[f.input("left", "run", "value")],
        );
    f.fire("go", vec![Value::local(-4_i64)])
        .unwrap()
        .expect_success()
        .expect_deliveries_from(
            f.output("source", "value"),
            &[f.input("right", "run", "value")],
        );
}

#[test]
fn to_all_reuses_one_payload_across_multiple_connected_destinations() {
    let mut g = GraphBuilder::new("multicast");
    let left = g.node("left", |x: i64| x).unwrap();
    let right = g.node("right", |x: i64| x).unwrap();
    let (l, r) = (left.inlet(), right.inlet());
    let source = g
        .node("source", move |x: i64| Output::to_all([&l, &r], x))
        .unwrap();
    g.connect(source.output(), left.inlet()).unwrap();
    g.connect(source.output(), right.inlet()).unwrap();
    let input = g.expose_input("request", source.inlet()).unwrap();
    g.trigger("go", [input]).unwrap();
    let f = fixture(g);
    f.fire("go", vec![Value::local(7_i64)])
        .unwrap()
        .expect_success()
        .expect_deliveries_from(
            f.output("source", "value"),
            &[
                f.input("left", "run", "value"),
                f.input("right", "run", "value"),
            ],
        );
}

#[test]
fn prefix_is_segment_aware_and_selects_only_connected_nodes() {
    let mut g = GraphBuilder::new("prefix");
    let workers = g.node("workers", |x: i64| x).unwrap();
    let other = g.node("workers-old", |x: i64| x).unwrap();
    let source = g
        .node("source", |x: i64| Output::to_prefix("workers", x))
        .unwrap();
    g.connect(source.output(), workers.inlet()).unwrap();
    g.connect(source.output(), other.inlet()).unwrap();
    let input = g.expose_input("request", source.inlet()).unwrap();
    g.trigger("go", [input]).unwrap();
    let f = fixture(g);
    f.fire("go", vec![Value::local(9_i64)])
        .unwrap()
        .expect_success()
        .expect_deliveries_from(
            f.output("source", "value"),
            &[f.input("workers", "run", "value")],
        );
}

#[test]
fn filter_can_use_intermediate_data_and_destination_metadata() {
    let mut g = GraphBuilder::new("filter");
    let left = g.node("left", |x: i64| x).unwrap();
    let right = g.node("right", |x: i64| x).unwrap();
    let source = g
        .node("source", |x: i64| {
            let chosen = if x % 2 == 0 { "left" } else { "right" };
            let result = x * 10;
            Output::to_filter(move |destination| destination.path() == chosen, result)
        })
        .unwrap();
    g.connect(source.output(), left.inlet()).unwrap();
    g.connect(source.output(), right.inlet()).unwrap();
    let input = g.expose_input("request", source.inlet()).unwrap();
    g.trigger("go", [input]).unwrap();
    let f = fixture(g);
    f.fire("go", vec![Value::local(3_i64)])
        .unwrap()
        .expect_success()
        .expect_deliveries_from(
            f.output("source", "value"),
            &[f.input("right", "run", "value")],
        );
}

#[test]
fn no_destinations_still_publishes_value_and_option_is_data() {
    let mut g = GraphBuilder::new("none");
    let source = g
        .node("source", |x: i64| {
            if x > 0 {
                Output::all(x)
            } else {
                Output::to_all([], x)
            }
        })
        .unwrap();
    let sink = g.node("sink", |x: i64| x + 1).unwrap();
    g.connect(source.output(), sink.inlet()).unwrap();
    let input = g.expose_input("request", source.inlet()).unwrap();
    g.trigger("go", [input]).unwrap();
    let f = fixture(g);
    f.fire("go", vec![Value::local(-1_i64)])
        .unwrap()
        .expect_success()
        .expect_publication_count(f.output("source", "value"), 1)
        .expect_local_at(f.output("source", "value"), &-1_i64)
        .expect_deliveries_from(f.output("source", "value"), &[]);
    f.fire("go", vec![Value::local(1_i64)])
        .unwrap()
        .expect_success()
        .expect_deliveries_from(
            f.output("source", "value"),
            &[f.input("sink", "run", "value")],
        );

    let mut g = GraphBuilder::new("option-is-data");
    let source = g.node("source", |x: i64| Some(x)).unwrap();
    let sink = g.node("sink", |v: Option<i64>| v.unwrap_or(0)).unwrap();
    g.connect(source.output(), sink.inlet()).unwrap();
    let input = g.expose_input("request", source.inlet()).unwrap();
    g.trigger("go", [input]).unwrap();
    let f = fixture(g);
    f.fire("go", vec![Value::local(7_i64)])
        .unwrap()
        .expect_success()
        .expect_deliveries_from(
            f.output("source", "value"),
            &[f.input("sink", "run", "value")],
        );
}

#[test]
fn disconnected_explicit_target_fails_without_partial_delivery_and_recovers() {
    let mut g = GraphBuilder::new("unconnected");
    let left = g.node("left", |x: i64| x).unwrap();
    let right = g.node("right", |x: i64| x).unwrap();
    let (l, r) = (left.inlet(), right.inlet());
    let source = g
        .node("source", move |x: i64| {
            if x < 0 {
                Output::to(&r, x)
            } else {
                Output::to(&l, x)
            }
        })
        .unwrap();
    g.connect(source.output(), left.inlet()).unwrap();
    let input = g.expose_input("request", source.inlet()).unwrap();
    g.trigger("go", [input]).unwrap();
    let f = fixture(g);
    f.fire("go", vec![Value::local(-1_i64)])
        .unwrap()
        .expect_error(|error| {
            matches!(error, WaveError::OutputRoutingFailed { reason, .. }
            if reason.contains("not connected"))
        });
    f.fire("go", vec![Value::local(1_i64)])
        .unwrap()
        .expect_success()
        .expect_deliveries_from(
            f.output("source", "value"),
            &[f.input("left", "run", "value")],
        );
}

#[test]
fn duplicate_explicit_destination_is_rejected_before_delivery() {
    let mut g = GraphBuilder::new("duplicate");
    let left = g.node("left", |x: i64| x).unwrap();
    let target = left.inlet();
    let source = g
        .node("source", move |x: i64| {
            Output::to_all([&target, &target], x)
        })
        .unwrap();
    g.connect(source.output(), left.inlet()).unwrap();
    let input = g.expose_input("request", source.inlet()).unwrap();
    g.trigger("go", [input]).unwrap();
    let f = fixture(g);
    f.fire("go", vec![Value::local(5_i64)])
        .unwrap()
        .expect_error(|error| {
            matches!(error, WaveError::OutputRoutingFailed { reason, .. }
            if reason.contains("duplicate"))
        });
}

#[test]
fn filter_panic_is_isolated_and_next_wave_can_succeed() {
    let mut g = GraphBuilder::new("filter-panic");
    let sink = g.node("sink", |x: i64| x).unwrap();
    let source = g
        .node("source", |x: i64| {
            Output::to_filter(
                move |_| {
                    assert!(x >= 0, "intentional filter panic");
                    true
                },
                x,
            )
        })
        .unwrap();
    g.connect(source.output(), sink.inlet()).unwrap();
    let input = g.expose_input("request", source.inlet()).unwrap();
    g.trigger("go", [input]).unwrap();
    let f = fixture(g);
    f.fire("go", vec![Value::local(-1_i64)])
        .unwrap()
        .expect_error(|error| {
            matches!(error, WaveError::OutputRoutingFailed { reason, .. }
            if reason.contains("panicked"))
        });
    f.fire("go", vec![Value::local(1_i64)])
        .unwrap()
        .expect_success()
        .expect_deliveries_from(
            f.output("source", "value"),
            &[f.input("sink", "run", "value")],
        );
}

#[test]
fn foreign_graph_input_handle_is_not_a_routing_capability() {
    let mut foreign = GraphBuilder::new("foreign");
    let foreign_sink = foreign.node("left", |x: i64| x).unwrap();
    let foreign_input = foreign_sink.inlet();

    let mut g = GraphBuilder::new("real");
    let left = g.node("left", |x: i64| x).unwrap();
    let source = g
        .node("source", move |x: i64| Output::to(&foreign_input, x))
        .unwrap();
    g.connect(source.output(), left.inlet()).unwrap();
    let input = g.expose_input("request", source.inlet()).unwrap();
    g.trigger("go", [input]).unwrap();
    fixture(g)
        .fire("go", vec![Value::local(1_i64)])
        .unwrap()
        .expect_error(|error| {
            matches!(error, WaveError::OutputRoutingFailed { reason, .. }
            if reason.contains("different authoring graph"))
        });
}

#[test]
fn filter_cannot_select_a_node_without_an_existing_connection() {
    let mut g = GraphBuilder::new("filtered-candidates");
    let connected = g.node("connected", |x: i64| x).unwrap();
    let _unconnected = g.node("unconnected", |x: i64| x).unwrap();
    let source = g
        .node("source", |x: i64| {
            Output::to_filter(|dest| dest.path() == "unconnected", x)
        })
        .unwrap();
    g.connect(source.output(), connected.inlet()).unwrap();
    let input = g.expose_input("request", source.inlet()).unwrap();
    g.trigger("go", [input]).unwrap();
    let f = fixture(g);
    f.fire("go", vec![Value::local(9_i64)])
        .unwrap()
        .expect_success()
        .expect_deliveries_from(f.output("source", "value"), &[]);
}

#[test]
fn prefix_matches_flattened_module_descendants_not_sibling_names() {
    let mut child = GraphBuilder::new("worker-module");
    let worker = child.node("process", |x: i64| x).unwrap();
    child.expose_input("in", worker.inlet()).unwrap();
    child.expose_output("result", worker.output()).unwrap();
    let child = child.module();

    let mut g = GraphBuilder::new("nested-prefix");
    let selected = g.instance("workers", &child).unwrap();
    let excluded = g.instance("workers-old", &child).unwrap();
    let source = g
        .node("source", |x: i64| Output::to_prefix("workers", x))
        .unwrap();
    g.connect(
        source.output(),
        selected
            .operation()
            .unwrap()
            .inlets::<(), phenix_graph::LocalValue>()
            .port(phenix_graph::InletPort::<(), i64>::new("in"))
            .field(phenix_graph::SlotField::<i64, i64>::new("value"))
            .target()
            .unwrap(),
    )
    .unwrap();
    g.connect(
        source.output(),
        excluded
            .operation()
            .unwrap()
            .inlets::<(), phenix_graph::LocalValue>()
            .port(phenix_graph::InletPort::<(), i64>::new("in"))
            .field(phenix_graph::SlotField::<i64, i64>::new("value"))
            .target()
            .unwrap(),
    )
    .unwrap();
    let input = g.expose_input("request", source.inlet()).unwrap();
    g.trigger("go", [input]).unwrap();
    let f = fixture(g);
    f.fire("go", vec![Value::local(42_i64)])
        .unwrap()
        .expect_success()
        .expect_deliveries_from(
            f.output("source", "value"),
            &[f.input("workers/process", "run", "value")],
        );
}

#[test]
fn result_is_data_without_an_explicit_failure_contract() {
    let mut g = GraphBuilder::new("result-data");
    let source = g
        .node("source", |x: i64| -> Result<i64, &'static str> {
            if x < 0 { Err("negative") } else { Ok(x) }
        })
        .unwrap();
    let sink = g
        .node("sink", |result: Result<i64, &'static str>| {
            result.unwrap_or_default()
        })
        .unwrap();
    g.connect(source.output(), sink.inlet()).unwrap();
    let input = g.expose_input("request", source.inlet()).unwrap();
    g.trigger("go", [input]).unwrap();
    let f = fixture(g);
    f.fire("go", vec![Value::local(-1_i64)])
        .unwrap()
        .expect_success()
        .expect_deliveries_from(
            f.output("source", "value"),
            &[f.input("sink", "run", "value")],
        );
}

#[test]
fn mixed_connected_and_unconnected_targets_fail_atomically() {
    let mut g = GraphBuilder::new("atomic-explicit");
    let left = g.node("left", |x: i64| x).unwrap();
    let right = g.node("right", |x: i64| x).unwrap();
    let (l, r) = (left.inlet(), right.inlet());
    let source = g
        .node("source", move |x: i64| Output::to_all([&l, &r], x))
        .unwrap();
    g.connect(source.output(), left.inlet()).unwrap();
    let input = g.expose_input("request", source.inlet()).unwrap();
    g.trigger("go", [input]).unwrap();
    let f = fixture(g);
    f.fire("go", vec![Value::local(1_i64)])
        .unwrap()
        .expect_error(|error| {
            matches!(error, WaveError::OutputRoutingFailed { reason, .. }
            if reason.contains("not connected"))
        })
        .expect_deliveries_from(f.output("source", "value"), &[]);
}
