use phenix_advanced_contracts::{
    model_request_sha256, validate_model_preparation_proposal, ModelPreparationChange,
    ModelPreparationInput, ModelPreparationProblem, ModelPreparationProposal,
};
use phenix_basic_contracts::{Limits, Message, ModelRequest, Role};
use phenix_plugin_advanced_model_preparer::{propose_unchanged, GRAPH};

fn input() -> ModelPreparationInput {
    let original = ModelRequest {
        session: "s1".into(),
        run: "run-1".into(),
        history: vec![Message {
            role: Role::User,
            text: "watch this request".into(),
            calls: vec![],
            call_id: String::new(),
            error: None,
        }],
        tools: vec![],
        skills: vec![],
        context: vec![],
        limits: Limits::default(),
    };
    ModelPreparationInput {
        original_sha256: model_request_sha256(&original).unwrap(),
        original,
    }
}

#[test]
fn advanced_hook_is_an_actual_state_triggered_graph_node() {
    let graph = GRAPH.owned().unwrap();
    assert_eq!(graph.nodes.len(), 1);
    assert_eq!(
        graph.nodes[0].triggers,
        ["state:phenix.basic.agent/pending_model_request"]
    );
    assert_eq!(
        graph.nodes[0].operation,
        "phenix.advanced.model-preparation-trigger@1/on_change"
    );
    assert_eq!(
        graph.nodes[0].writes,
        ["phenix.advanced.model-preparer/proposal"]
    );
    assert!(graph.nodes[0].after.is_empty());
}
#[test]
fn graph_change_notice_matches_the_existing_m1_shape() {
    let notice = ModelPreparationChange {
        epoch: 2,
        properties: vec!["phenix.basic.agent/pending_model_request".into()],
    };
    assert_eq!(notice.epoch, 2);
    assert_eq!(notice.properties.len(), 1);
}
#[test]
fn no_op_preparer_preserves_full_canonical_model_request() {
    let input = input();
    let proposal: ModelPreparationProposal = propose_unchanged(input.clone()).unwrap();
    validate_model_preparation_proposal(&input, &proposal).unwrap();
    assert_eq!(proposal.candidate, input.original);
    assert!(proposal.checkpoint.is_none());
}
#[test]
fn no_op_preparer_rejects_modified_authoritative_source() {
    let mut input = input();
    input.original.history[0].text = "forged".into();
    assert_eq!(
        propose_unchanged(input).unwrap_err(),
        ModelPreparationProblem::SourceChanged
    );
}
