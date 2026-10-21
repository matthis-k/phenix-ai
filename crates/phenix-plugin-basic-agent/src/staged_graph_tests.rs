//! P2-3aj RED: a *second* state-triggered Basic node admits Model only
//! after an independently contributed preparer has committed the handoff.
use super::{verify_staged_model_candidate, GRAPH};
use phenix_basic_contracts::{Limits, Message, ModelRequest};

fn source() -> ModelRequest {
    ModelRequest {
        session: "s1".into(),
        run: "run-1".into(),
        history: vec![Message::user("answer")],
        tools: vec![],
        skills: vec![],
        context: vec![],
        limits: Limits::default(),
    }
}

#[test]
fn staged_graph_requires_distinct_prepare_then_model_advance_nodes() {
    let graph = GRAPH.owned().unwrap();
    assert_eq!(graph.nodes.len(), 2);
    let first = graph
        .nodes
        .iter()
        .find(|node| node.id == "prepare")
        .unwrap();
    assert_eq!(first.id, "prepare");
    assert_eq!(first.triggers, ["phenix.basic.prompt"]);
    assert_eq!(first.operation, "phenix.basic.turn-prepare@1/prepare");
    assert_eq!(first.writes, ["phenix.basic.agent/pending_model_request"]);

    let next = graph
        .nodes
        .iter()
        .find(|node| node.id == "model-advance")
        .unwrap();
    assert_eq!(next.id, "model-advance");
    assert_eq!(
        next.triggers,
        ["state:phenix.basic.agent/prepared_model_request"]
    );
    assert_eq!(next.operation, "phenix.basic.turn-advance@1/advance");
    assert_eq!(next.after, ["phenix.basic.agent/prepare"]);
    assert_eq!(next.writes, ["phenix.basic.agent/pending_model_request"]);
}

#[test]
fn candidate_admission_rejects_every_modification_without_durable_proof() {
    let original = source();
    assert!(verify_staged_model_candidate(&original, &original).is_ok());
    let mut modified = original.clone();
    modified.run = "run-other".into();
    assert!(verify_staged_model_candidate(&original, &modified).is_err());
    let mut modified = original.clone();
    *modified.history[0].text_mut() = "forged".into();
    assert!(verify_staged_model_candidate(&original, &modified).is_err());
    let mut modified = original.clone();
    modified.limits.model_tokens = Some(10_000);
    assert!(verify_staged_model_candidate(&original, &modified).is_err());
    let mut modified = original.clone();
    modified.session = "s2".into();
    assert!(verify_staged_model_candidate(&original, &modified).is_err());
}

#[test]
fn staged_graph_has_no_extra_public_action() {
    let graph = GRAPH.owned().unwrap();
    assert!(graph.actions.is_empty());
    assert!(graph.events.is_empty());
}
