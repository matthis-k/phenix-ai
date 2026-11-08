//! Type-checked, declaration-owned layout configuration (Iteration 15).
//! Tests use the real GraphBuilder, ModuleCatalog, Deployment and executor.
#![cfg(feature = "derive")]

use phenix_graph::{
    Deployment, GraphBuildError, GraphBuilder, Layout, LocalPortLayout, ReportRetention, Store,
    Value,
};

#[derive(Clone, LocalPortLayout)]
struct AddArgs {
    left: i64,
    right: i64,
}

#[test]
fn field_layout_is_inferred_from_handler_arguments() {
    let mut graph = GraphBuilder::new("field-inference");
    let d = graph
        .node("d", |x: AddArgs| x.left + x.right)
        .unwrap()
        .input(Layout::fields())
        .unwrap();
    let left = graph
        .expose_input("left", d.field(AddArgs::SLOT_LEFT))
        .unwrap();
    let right = graph
        .expose_input("right", d.field(AddArgs::SLOT_RIGHT))
        .unwrap();
    graph.trigger("run", [left, right]).unwrap();
    let engine = Deployment::new(graph.compile().unwrap())
        .report_retention(ReportRetention::RetainPayloads)
        .store(Store::new())
        .build()
        .unwrap();
    let report = engine
        .fire("run", vec![Value::local(12_i64), Value::local(30_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report.publications().last().unwrap().borrow::<i64>(),
        Some(&42)
    );
}

#[test]
fn default_layout_remains_atomic_without_customization() {
    let mut graph = GraphBuilder::new("atomic-inference");
    let d = graph.node("d", |x: AddArgs| x.left + x.right).unwrap();
    let request = graph.expose_input("request", d.inlet()).unwrap();
    graph.trigger("run", [request]).unwrap();
    let engine = Deployment::new(graph.compile().unwrap())
        .report_retention(ReportRetention::RetainPayloads)
        .store(Store::new())
        .build()
        .unwrap();
    let report = engine
        .fire("run", vec![Value::local(AddArgs { left: 3, right: 4 })])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report.publications().last().unwrap().borrow::<i64>(),
        Some(&7)
    );
}

#[test]
fn a_late_layout_change_is_rejected_by_the_graph_compiler() {
    let mut graph = GraphBuilder::new("invalidated-input");
    let d = graph.node("d", |x: AddArgs| x.left + x.right).unwrap();
    // Initially this is a valid atomic input. Changing the declaration later
    // removes that slot; existing wiring must not silently be reinterpreted.
    graph.expose_input("request", d.inlet()).unwrap();
    let d = d.input(Layout::fields()).unwrap();
    assert_eq!(d.name(), "d");
    assert!(graph.compile().is_err());
}

#[test]
fn a_configured_layout_is_visible_to_later_connections() {
    let mut graph = GraphBuilder::new("configured-before-connect");
    let source = graph.node("source", |x: i64| x * 2).unwrap();
    let sum = graph
        .node("sum", |x: AddArgs| x.left + x.right)
        .unwrap()
        .input(Layout::fields())
        .unwrap();
    graph
        .connect(source.output(), sum.field(AddArgs::SLOT_LEFT))
        .unwrap();
    let req = graph.expose_input("request", source.inlet()).unwrap();
    let other = graph
        .expose_input("other", sum.field(AddArgs::SLOT_RIGHT))
        .unwrap();
    graph.trigger("run", [req, other]).unwrap();
    // Compilation proves that the pending layout and symbolic connections
    // are validated together rather than using the original atomic port.
    graph.compile().unwrap();
}

#[test]
fn independent_graph_clones_do_not_share_pending_layout_edits() {
    let mut graph = GraphBuilder::new("snapshot");
    let d = graph.node("d", |x: AddArgs| x.left + x.right).unwrap();
    let other = graph.clone();
    let configured = d.input(Layout::fields()).unwrap();
    assert_eq!(configured.name(), "d");
    // Snapshot taken before the edit still has the original atomic slot.
    assert!(other.local_input::<AddArgs>("d", "run", "value").is_ok());
    assert!(graph.local_input::<AddArgs>("d", "run", "value").is_err());
    graph.compile().unwrap();
    other.compile().unwrap();
}

#[test]
fn a_consumed_graph_cannot_be_reconfigured() {
    let mut graph = GraphBuilder::new("consumed");
    let handle = graph.node("d", |x: AddArgs| x.left + x.right).unwrap();
    let _compiled = graph.compile().unwrap();
    let err = handle
        .input(Layout::fields())
        .err()
        .expect("consumed graph");
    assert!(matches!(err, GraphBuildError::Configuration(_)));
}

#[test]
fn failed_replacement_preserves_the_configurable_declaration() {
    use phenix_graph::NodeTemplate;
    let mut graph = GraphBuilder::new("failed-replacement");
    let handle = graph.node("d", |x: AddArgs| x.left + x.right).unwrap();
    // Incorrect input and output types; failure must not revoke the handle.
    assert!(
        graph
            .replace(
                "d",
                NodeTemplate::local_function::<i64, i64, i64, _>(|x: i64| x + 1,)
            )
            .is_err()
    );
    handle.input(Layout::fields()).unwrap();
    graph.compile().unwrap();
}

#[test]
fn successful_replacement_revokes_old_declaration_handle() {
    use phenix_graph::NodeTemplate;
    let mut graph = GraphBuilder::new("replacement-invalidates-handle");
    let handle = graph.node("d", |x: AddArgs| x.left + x.right).unwrap();
    graph
        .replace(
            "d",
            NodeTemplate::local_function::<AddArgs, i64, i64, _>(|x: AddArgs| x.left * x.right),
        )
        .unwrap();
    assert!(matches!(
        handle.input(Layout::fields()),
        Err(GraphBuildError::Configuration(_))
    ));
}

#[test]
fn layout_revision_endpoints_cannot_bind_an_unchanged_clone() {
    let mut graph = GraphBuilder::new("layout-lineage");
    let node = graph.node("n", |x: i64| x).unwrap();
    let old_input = node.inlet();
    let old_output = node.output();
    let mut clone = graph.clone();
    // Even a same-shape reconfiguration creates a new declaration revision.
    let node = node.input(Layout::atomic()).unwrap();
    assert!(graph.expose_input("stale", old_input.clone()).is_err());
    assert!(graph.expose_output("stale", old_output).is_err());
    assert!(clone.expose_input("divergent", node.inlet()).is_err());
    assert!(clone.expose_output("divergent", node.output()).is_err());
    clone.expose_input("inherited", old_input).unwrap();
    graph.expose_input("current", node.inlet()).unwrap();
    clone.compile().unwrap();
    graph.compile().unwrap();
}
