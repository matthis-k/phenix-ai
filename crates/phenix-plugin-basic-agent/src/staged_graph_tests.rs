//! P2-3aj RED: a *second* state-triggered Basic node admits Model only
//! after an independently contributed preparer has committed the handoff.
use super::{graph_patch, verify_staged_model_candidate};
use phenix_basic_contracts::{Limits, Message, ModelRequest};
use phenix_sdk::Edit;

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
    let patch = graph_patch().unwrap();
    assert_eq!(patch.id, "phenix.basic.agent");
    assert_eq!(patch.edits.len(), 2);
    let Edit::AddNode(node) = &patch.edits[0] else {
        panic!("expected node")
    };
    assert_eq!(node.path, "phenix.basic.agent/prepare");
    assert_eq!(node.action, "phenix.basic.turn-prepare@1/prepare");
    let Edit::AddTrigger(trigger) = &patch.edits[1] else {
        panic!("expected trigger")
    };
    assert_eq!(trigger.name, "phenix.basic.prompt");
    assert_eq!(trigger.to.node, node.path);
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
fn staged_graph_has_no_second_execution_path() {
    let patch = graph_patch().unwrap();
    assert!(patch.after.is_empty());
    assert!(matches!(
        patch.edits.as_slice(),
        [Edit::AddNode(_), Edit::AddTrigger(_)]
    ));
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
