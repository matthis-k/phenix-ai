#![cfg(feature = "derive")]

use phenix_graph::{
    GraphBuilder, GraphContract, GraphUnit, InterfacePhase, Layout, LocalPortLayout,
};

#[derive(Clone, LocalPortLayout, phenix_graph::InputFields)]
struct Pair {
    left: i64,
    right: i64,
}

#[test]
fn retained_views_and_projections_follow_layout_changes_and_restoration() {
    let mut graph = GraphBuilder::new("root");
    let node = graph.node("worker", |x: Pair| x.left + x.right).unwrap();
    graph.node("other", |x: Pair| x.left + x.right).unwrap();
    let sink = graph.node("sink", |x: i64| x).unwrap();
    let contract = GraphContract::new(
        "inspection/pair@1",
        graph.inspect("worker").unwrap().inspect().unwrap().shape,
    )
    .unwrap();
    graph.implements("worker", &contract).unwrap();
    graph.implements("other", &contract).unwrap();
    let view = graph.inspect("worker").unwrap();
    let bound = graph.bind_contract("worker", &contract).unwrap();
    let other_bound = graph.bind_contract("other", &contract).unwrap();
    let old_output = bound
        .operation()
        .unwrap()
        .output::<i64, phenix_graph::LocalValue>()
        .unwrap();
    let node = node.input(Layout::fields()).unwrap();

    assert_eq!(view.inspect().unwrap().shape.inputs().count(), 2);
    for error in [
        contract.check_shape(&view).unwrap_err(),
        contract.check_implementation(&view).unwrap_err(),
        bound.operation().err().unwrap(),
    ] {
        assert_eq!(error.phase, InterfacePhase::Authoring);
        assert_eq!(error.path.as_ref(), "worker");
        assert_eq!(error.contract.as_deref(), Some(contract.id()));
    }
    assert!(
        other_bound
            .operation()
            .unwrap()
            .output::<i64, phenix_graph::LocalValue>()
            .is_ok()
    );

    let _node = node.input(Layout::atomic()).unwrap();
    contract.check_shape(&view).unwrap();
    contract.check_implementation(&view).unwrap();
    let fresh_output = bound
        .operation()
        .unwrap()
        .output::<i64, phenix_graph::LocalValue>()
        .unwrap();
    // Restoring a shape does not revive an endpoint from an earlier revision.
    assert!(graph.connect(old_output, sink.inlet()).is_err());
    graph.connect(fresh_output, sink.inlet()).unwrap();
    graph.compile().unwrap();
}

#[test]
fn retained_root_checks_revalidate_pending_public_routing() {
    let mut graph = GraphBuilder::new("root");
    let node = graph.node("worker", |x: Pair| x.left + x.right).unwrap();
    graph.expose_input("request", node.inlet()).unwrap();
    graph.expose_output("value", node.output()).unwrap();
    let contract = GraphContract::new(
        "inspection/root@1",
        graph.inspect("").unwrap().inspect().unwrap().shape,
    )
    .unwrap();
    graph.implements_public(&contract).unwrap();
    let view = graph.inspect("").unwrap();
    contract.check_implementation(&view).unwrap();

    let _node = node.input(Layout::fields()).unwrap();
    assert!(view.inspect().is_err());
    for error in [
        contract.check_shape(&view).unwrap_err(),
        contract.check_implementation(&view).unwrap_err(),
        graph.bind_contract("", &contract).err().unwrap(),
    ] {
        assert_eq!(error.phase, InterfacePhase::Authoring);
        assert_eq!(error.path.as_ref(), "");
        assert_eq!(error.contract.as_deref(), Some(contract.id()));
    }
    assert!(graph.compile().is_err());
}
