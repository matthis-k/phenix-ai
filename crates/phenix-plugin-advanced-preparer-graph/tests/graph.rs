use phenix_plugin_advanced_preparer_graph::GRAPH;

#[test]
fn selected_advanced_graph_owns_prepared_output_and_imports_basic_advance() {
    let graph = GRAPH.owned().unwrap();
    assert_eq!(graph.nodes.len(), 2);

    let prepare = graph
        .nodes
        .iter()
        .find(|node| node.id == "prepare")
        .unwrap();
    assert_eq!(
        prepare.triggers,
        ["state:phenix.basic.agent/pending_model_request"]
    );
    assert_eq!(
        prepare.writes,
        ["phenix.advanced.graph-preparer/prepared_model_request"]
    );
    assert_eq!(
        prepare.operation,
        "phenix.advanced.model-preparation-change@1/run"
    );

    let advance = graph
        .nodes
        .iter()
        .find(|node| node.id == "advance")
        .unwrap();
    assert_eq!(
        advance.triggers,
        ["state:phenix.advanced.graph-preparer/prepared_model_request"]
    );
    assert_eq!(advance.writes, ["phenix.basic.agent/pending_model_request"]);
    assert_eq!(advance.selector, "Advance");
    assert_eq!(
        advance.after,
        [
            "phenix.advanced.graph-preparer/prepare",
            "phenix.basic.agent/prepare",
        ]
    );
    assert_eq!(advance.operation, "phenix.basic.turn-advance@1/advance");
    assert!(graph.actions.is_empty());
}
