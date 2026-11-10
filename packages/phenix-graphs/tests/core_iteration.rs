//! Graph-first acceptance tests: one return-value routing API, one compiler,
//! one executor. No separately configured output selectors.
use phenix_graph::{
    CompileLimits, CompilePhase, Deployment, GraphBuilder, Input, Output, ReportRetention, Value,
    WaveError,
};
use std::sync::Arc;

fn routed_graph<F>(choose: F) -> (Arc<phenix_graph::CompiledGraph>, u16, u16)
where
    F: Fn(i64, &Input<i64>, &Input<i64>) -> Output<i64> + Send + Sync + 'static,
{
    let mut g = GraphBuilder::new("routing");
    let left = g.node("left", |n: i64| n * 2).unwrap();
    let right = g.node("right", |n: i64| n * 3).unwrap();
    let (l, r) = (left.inlet(), right.inlet());
    let source = g
        .node("source", move |n: i64| choose(n + 1, &l, &r))
        .unwrap();
    g.connect(source.output(), left.inlet()).unwrap();
    g.connect(source.output(), right.inlet()).unwrap();
    let request = g.expose_input("request", source.inlet()).unwrap();
    // The root graph is an execution container, not a multi-output operation.
    g.trigger("start", [request]).unwrap();
    let graph = g.compile().unwrap();
    let left_id = match graph.find("left").unwrap() {
        phenix_graph::FindResult::Node(node) => node.id(),
        _ => unreachable!(),
    };
    let right_id = match graph.find("right").unwrap() {
        phenix_graph::FindResult::Node(node) => node.id(),
        _ => unreachable!(),
    };
    (graph, left_id, right_id)
}

#[test]
fn output_selection_activates_only_selected_existing_edge() {
    let (graph, left, right) = routed_graph(|value, l, _| Output::to(l, value));
    let report = Deployment::new(graph)
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire("start", vec![Value::local(4_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.invocation_count(left), 1);
    assert_eq!(report.invocation_count(right), 0);
    assert_eq!(report.publications().count(), 2);
}

#[test]
fn output_none_keeps_publication_without_deliveries() {
    let (graph, left, right) = routed_graph(|value, _, _| Output::to_all([], value));
    let report = Deployment::new(graph)
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire("start", vec![Value::local(4_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report.invocation_count(left) + report.invocation_count(right),
        0
    );
    assert_eq!(report.publications().count(), 1);
    assert!(report.delivery_events().is_empty());
}

#[test]
fn duplicate_target_is_a_wave_error_not_a_partial_delivery() {
    let (graph, left, right) = routed_graph(|value, l, _| Output::to_all([l, l], value));
    let report = Deployment::new(graph)
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire("start", vec![Value::local(4_i64)])
        .unwrap();
    assert!(!report.success());
    assert!(report.errors().iter().any(
        |error| matches!(error, WaveError::OutputRoutingFailed { reason, .. }
            if reason.contains("duplicate"))
    ));
    assert_eq!(
        report.invocation_count(left) + report.invocation_count(right),
        0
    );
    assert!(report.publications().next().is_none());
    assert!(report.delivery_events().is_empty());
}

#[test]
fn filter_panic_is_isolated_and_next_wave_still_works() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let attempts = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&attempts);
    let (graph, left, right) = routed_graph(move |value, _, _| {
        let counter = Arc::clone(&counter);
        Output::to_filter(
            move |_| {
                if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                    panic!("selector failed");
                }
                true
            },
            value,
        )
    });
    let engine = Deployment::new(graph)
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap();
    let failed = engine.fire("start", vec![Value::local(1_i64)]).unwrap();
    assert!(!failed.success());
    assert!(failed.errors().iter().any(
        |error| matches!(error, WaveError::OutputRoutingFailed { reason, .. }
            if reason.contains("panicked"))
    ));
    assert!(failed.delivery_events().is_empty());
    let recovered = engine.fire("start", vec![Value::local(2_i64)]).unwrap();
    assert!(recovered.success(), "{:?}", recovered.error_messages());
    assert_eq!(recovered.invocation_count(left), 1);
    assert_eq!(recovered.invocation_count(right), 1);
}

#[test]
fn stable_trigger_name_and_handle_share_the_same_entry() {
    let mut g = GraphBuilder::new("trigger-names");
    let n = g.node("increment", |x: i64| x + 1).unwrap();
    let input = g.expose_input("request", n.inlet()).unwrap();
    let id = g.trigger("run", [input]).unwrap();
    let graph = g.compile().unwrap();
    assert_eq!(graph.trigger("run"), Some(id));
    let engine = Deployment::new(graph)
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap();
    assert!(
        engine
            .fire("run", vec![Value::local(41_i64)])
            .unwrap()
            .success()
    );
    assert!(
        engine
            .fire(id, vec![Value::local(42_i64)])
            .unwrap()
            .success()
    );
    assert!(
        engine
            .fire("missing", vec![Value::local(1_i64)])
            .unwrap_err()
            .contains("unknown trigger")
    );
}

#[test]
fn compiler_limits_reject_oversized_graphs_before_execution() {
    let mut g = GraphBuilder::new("limited").limits(CompileLimits {
        max_work: 0,
        ..CompileLimits::default()
    });
    g.node("increment", |x: i64| x + 1).unwrap();
    let error = match g.compile() {
        Ok(_) => panic!("unexpected compile success"),
        Err(e) => e,
    };
    assert_eq!(error.phase, CompilePhase::Expansion);
    assert!(error.message.contains("expansion work"));
}

#[test]
fn compiler_limits_apply_to_nested_instances() {
    let mut child = GraphBuilder::new("child");
    child.node("work", |x: i64| x).unwrap();
    let child = child.module();
    let mut parent = GraphBuilder::new("parent").limits(CompileLimits {
        max_instances: 0,
        ..CompileLimits::default()
    });
    parent.instance("nested", &child).unwrap();
    let error = match parent.compile() {
        Ok(_) => panic!("unexpected compile success"),
        Err(e) => e,
    };
    assert_eq!(error.phase, CompilePhase::Expansion);
    assert!(error.message.contains("instances"));
}

#[test]
fn selected_unconnected_target_cannot_forge_an_edge() {
    let mut g = GraphBuilder::new("disconnected");
    let connected = g.node("connected", |x: i64| x).unwrap();
    let disconnected = g.node("disconnected", |x: i64| x).unwrap();
    let target = disconnected.inlet();
    let source = g
        .node("source", move |x: i64| Output::to(&target, x))
        .unwrap();
    g.connect(source.output(), connected.inlet()).unwrap();
    let input = g.expose_input("request", source.inlet()).unwrap();
    g.trigger("start", [input]).unwrap();
    let report = Deployment::new(g.compile().unwrap())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire("start", vec![Value::local(4_i64)])
        .unwrap();
    assert!(!report.success());
    assert!(report.errors().iter().any(
        |error| matches!(error, WaveError::OutputRoutingFailed { reason, .. }
            if reason.contains("not connected"))
    ));
    assert!(report.delivery_events().is_empty());
    assert!(report.publications().next().is_none());
}

#[test]
fn invalid_numeric_trigger_does_not_start_a_wave() {
    let (graph, _, _) = routed_graph(|value, _, _| Output::all(value));
    let engine = Deployment::new(graph).build().unwrap();
    let error = match engine.fire(-1, vec![Value::local(1_i64)]) {
        Ok(_) => panic!("negative trigger admitted"),
        Err(error) => error,
    };
    assert!(error.contains("invalid trigger ID"));
}

#[test]
fn default_routing_broadcasts_to_every_declared_connection() {
    let mut graph = GraphBuilder::new("broadcast");
    let source = graph.node("source", |n: i64| n + 1).unwrap();
    let a = graph.node("a", |n: i64| n * 2).unwrap();
    let b = graph.node("b", |n: i64| n * 3).unwrap();
    graph.connect(source.output(), a.inlet()).unwrap();
    graph.connect(source.output(), b.inlet()).unwrap();
    let request = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("start", [request]).unwrap();
    let compiled = graph.compile().unwrap();
    let a_id = match compiled.find("a").unwrap() {
        phenix_graph::FindResult::Node(n) => n.id(),
        _ => unreachable!(),
    };
    let b_id = match compiled.find("b").unwrap() {
        phenix_graph::FindResult::Node(n) => n.id(),
        _ => unreachable!(),
    };
    let report = Deployment::new(compiled)
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire("start", vec![Value::local(3_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.invocation_count(a_id), 1);
    assert_eq!(report.invocation_count(b_id), 1);
}

#[test]
fn foreign_typed_output_cannot_be_connected_into_another_builder() {
    let mut first = GraphBuilder::new("first");
    let node = first.node("a", |n: i64| n).unwrap();
    let mut other = GraphBuilder::new("other");
    let sink = other.node("sink", |n: i64| n).unwrap();
    let error = match other.connect(node.output(), sink.inlet()) {
        Ok(_) => panic!("foreign graph output was accepted"),
        Err(error) => error,
    };
    assert!(error.contains("another graph builder"));
}
