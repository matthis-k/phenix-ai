use phenix_advanced_contracts::{
    model_request_sha256, validate_model_preparation_proposal, ModelPreparationInput,
};
use phenix_basic_contracts::{Limits, Message, ModelRequest};
use phenix_plugin_advanced_preparer_noop::unchanged_proposal;

#[test]
fn selected_noop_proposal_keeps_authoritative_source_bytes() {
    let request = ModelRequest {
        session: "s1".into(),
        run: "run-1".into(),
        history: vec![Message::user("hello")],
        tools: vec![],
        skills: vec![],
        context: vec![],
        limits: Limits::default(),
    };
    let input = ModelPreparationInput {
        original_sha256: model_request_sha256(&request).unwrap(),
        original: request.clone(),
    };
    let proposal = unchanged_proposal(input.clone()).unwrap();
    validate_model_preparation_proposal(&input, &proposal).unwrap();
    assert_eq!(proposal.candidate, request);
    assert!(proposal.checkpoint.is_none());
}

#[test]
fn spoofed_source_digest_cannot_produce_noop_proposal() {
    let request = ModelRequest {
        session: "s1".into(),
        run: "run-1".into(),
        history: vec![],
        tools: vec![],
        skills: vec![],
        context: vec![],
        limits: Limits::default(),
    };
    let result = unchanged_proposal(ModelPreparationInput {
        original: request,
        original_sha256: "0".repeat(64),
    });
    assert!(result.is_err());
}
