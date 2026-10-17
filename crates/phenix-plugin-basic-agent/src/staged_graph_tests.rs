//! P2-3ai RED: preparation is its own graph node, not a side-effect of Model.
use super::GRAPH;
use phenix_sdk::StaticGraphContribution;

#[test]
fn staged_graph_prepares_the_request_without_a_model_executor() {
    let graph: StaticGraphContribution = GRAPH;
    let compiled = graph.owned().unwrap();
    assert_eq!(compiled.nodes.len(), 1);
    let prepare = &compiled.nodes[0];
    assert_eq!(prepare.id, "prepare");
    assert_eq!(prepare.operation, "phenix.basic.turn-prepare@1/prepare");
    assert_eq!(prepare.triggers, ["phenix.basic.prompt"]);
    assert_eq!(prepare.writes, ["phenix.basic.agent/pending_model_request"]);
    assert!(!prepare.operation.contains("model@1/complete"));
}

#[test]
fn staged_graph_retains_the_existing_basic_action_identity() {
    let graph = GRAPH.owned().unwrap();
    assert!(graph.actions.is_empty());
    assert!(graph.events.is_empty());
    assert_eq!(graph.nodes[0].selector, "");
    assert!(graph.nodes[0].after.is_empty());
}
