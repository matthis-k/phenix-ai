use phenix_plugin_advanced_preparer_graph::GRAPH;

#[test]
fn selected_graph_hook_has_only_one_watched_trigger_and_canonical_write() {
    let graph = GRAPH.owned().unwrap();
    assert_eq!(graph.nodes.len(), 1);
    let node = &graph.nodes[0];
    assert_eq!(node.id, "prepare");
    assert_eq!(node.triggers, ["state:phenix.basic.agent/pending_model_request"]);
    assert_eq!(node.writes, ["phenix.basic.agent/prepared_model_request"]);
    assert_eq!(node.operation, "phenix.advanced.model-preparation-change@1/run");
    assert!(graph.actions.is_empty());
}
