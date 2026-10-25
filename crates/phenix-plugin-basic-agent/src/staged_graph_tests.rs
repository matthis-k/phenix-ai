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
fn staged_graph_exposes_only_the_basic_owned_prepare_node() {
    // The opt-in Advanced graph contributes the resume node and owns its
    // trigger property. Basic-only staging must remain independently usable.
    let graph = GRAPH.owned().unwrap();
    assert_eq!(graph.nodes.len(), 1);
    let first = graph
        .nodes
        .iter()
        .find(|node| node.id == "prepare")
        .unwrap();
    assert_eq!(first.id, "prepare");
    assert_eq!(first.triggers, ["phenix.basic.prompt"]);
    assert_eq!(first.operation, "phenix.basic.turn-prepare@1/prepare");
    assert_eq!(first.writes, ["phenix.basic.agent/pending_model_request"]);
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
    assert_eq!(graph.events.len(), 1);
    assert_eq!(graph.events[0].id, "phenix.agent.run-finished");
    assert!(graph.events[0].payload.fields.iter().all(|field| {
        field.name != "answer" && field.name != "text" && field.name != "credentials"
    }));
}

#[test]
fn pending_model_request_is_owned_and_exposes_only_get_and_watch() {
    let metadata = phenix_sdk::ContractMetadata::decode(super::plugin::METADATA.bytes()).unwrap();
    let pending = metadata
        .properties
        .iter()
        .find(|property| property.id == "pending_model_request")
        .unwrap();
    assert!(pending.get);
    assert!(pending.watch);
    assert!(
        !pending.set,
        "consumers must not mutate the pending request"
    );

    assert!(
        metadata
            .properties
            .iter()
            .all(|property| property.id != "prepared_model_request"),
        "Basic Agent no longer exports or owns a prepared request"
    );
}
