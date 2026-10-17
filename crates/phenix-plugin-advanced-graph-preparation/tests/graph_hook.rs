use phenix_advanced_contracts::{
    model_request_sha256, validate_model_preparation_proposal, ModelPreparationInput,
    ModelPreparationProblem,
};
use phenix_basic_contracts::{Limits, Message, ModelRequest, Role};
use phenix_plugin_advanced_graph_preparation::{pass_through, GRAPH};

#[test]
fn selected_graph_is_triggered_only_by_basic_preparation() {
    let graph = GRAPH.owned().unwrap();
    assert_eq!(graph.nodes.len(), 1);
    let node = &graph.nodes[0];
    assert_eq!(node.id, "prepare");
    assert_eq!(
        node.triggers,
        ["state:phenix.basic.agent/pending_model_request"]
    );
    assert_eq!(node.operation, "phenix.advanced.graph-preparation@1/run");
    assert_eq!(node.writes, ["phenix.advanced.graph-preparation/prepared"]);
    assert!(node.after.is_empty());
}

#[test]
fn no_compaction_is_an_exact_source_bound_proposal_not_an_authority_grant() {
    let original = ModelRequest {
        session: "s1".into(),
        run: "run-1".into(),
        history: vec![Message {
            role: Role::User,
            text: "Hello".into(),
            calls: vec![],
            call_id: String::new(),
            error: None,
        }],
        tools: vec![],
        skills: vec![],
        context: vec![],
        limits: Limits::default(),
    };
    let input = ModelPreparationInput {
        original_sha256: model_request_sha256(&original).unwrap(),
        original,
    };
    let proposal = pass_through(&input).unwrap();
    validate_model_preparation_proposal(&input, &proposal).unwrap();
    assert_eq!(proposal.candidate, input.original);
    assert!(proposal.checkpoint.is_none());
    let mut stale = input;
    stale.original.history[0].text = "Forger".into();
    assert_eq!(
        validate_model_preparation_proposal(&stale, &proposal).unwrap_err(),
        ModelPreparationProblem::SourceChanged
    );
}
