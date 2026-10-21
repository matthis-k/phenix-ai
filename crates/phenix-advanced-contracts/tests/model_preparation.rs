//! P2-3ah RED: source-bound, non-authoritative graph preparation handoff.
//! These tests do not treat a cited checkpoint as a grant or proof of storage.
use phenix_advanced_contracts::{
    model_request_sha256, validate_model_preparation_input, validate_model_preparation_proposal,
    ModelPreparationInput, ModelPreparationProblem, ModelPreparationProposal,
};
use phenix_basic_contracts::{
    ContextItem, Limits, Message, ModelRequest, Skill, ToolCall, ToolInfo,
};

fn source() -> ModelRequest {
    ModelRequest {
        session: "s1".into(),
        run: "run-3".into(),
        history: vec![
            Message::user("read this"),
            Message::assistant(
                "running",
                vec![ToolCall {
                    id: "c1".into(),
                    tool: "read".into(),
                    arguments: r#"{"path":"a.rs"}"#.into(),
                }],
            ),
            Message::tool(
                "c1",
                phenix_basic_contracts::ToolOutcome::Succeeded {
                    text: "read result".into(),
                },
            ),
            Message::user("now answer"),
        ],
        tools: vec![ToolInfo {
            id: "read".into(),
            description: "Read".into(),
            arguments_schema: r#"{"type":"object"}"#.into(),
            idempotent: true,
        }],
        skills: vec![Skill {
            id: "code".into(),
            description: "Review".into(),
            instructions: "Be precise".into(),
        }],
        context: vec![ContextItem {
            id: "project".into(),
            text: "evidence".into(),
        }],
        limits: Limits {
            model_turns: Some(2),
            tool_calls: Some(4),
            model_tokens: Some(800),
            http_timeout_ms: Some(30_000),
        },
    }
}
fn fixture() -> (ModelPreparationInput, ModelPreparationProposal) {
    let original = source();
    let digest = model_request_sha256(&original).unwrap();
    (
        ModelPreparationInput {
            original: original.clone(),
            original_sha256: digest.clone(),
        },
        ModelPreparationProposal {
            original_sha256: digest,
            candidate: original,
            checkpoint: None,
        },
    )
}
#[test]
fn unchanged_request_is_a_valid_non_authoritative_graph_handoff() {
    let (input, proposal) = fixture();
    validate_model_preparation_input(&input).unwrap();
    validate_model_preparation_proposal(&input, &proposal).unwrap();
    assert_eq!(proposal.candidate, input.original);
    assert_eq!(
        model_request_sha256(&proposal.candidate).unwrap(),
        input.original_sha256
    );
}

#[test]
fn forged_or_stale_source_never_activates_prepared_model() {
    let (mut input, proposal) = fixture();
    *input.original.history[3].text_mut() = "different turn".into();
    assert_eq!(
        validate_model_preparation_proposal(&input, &proposal).unwrap_err(),
        ModelPreparationProblem::SourceChanged
    );
    let (input, mut proposal) = fixture();
    proposal.original_sha256 = "0".repeat(64);
    assert_eq!(
        validate_model_preparation_proposal(&input, &proposal).unwrap_err(),
        ModelPreparationProblem::SourceChanged
    );
}
#[test]
fn graph_preparer_cannot_change_run_scope_tool_capabilities_or_budgets() {
    let (input, mut proposal) = fixture();
    proposal.candidate.run = "run-other".into();
    assert_eq!(
        validate_model_preparation_proposal(&input, &proposal).unwrap_err(),
        ModelPreparationProblem::IdentityChanged
    );
    let (input, mut proposal) = fixture();
    proposal.candidate.session = "s2".into();
    assert_eq!(
        validate_model_preparation_proposal(&input, &proposal).unwrap_err(),
        ModelPreparationProblem::IdentityChanged
    );
    let (input, mut proposal) = fixture();
    proposal.candidate.tools.clear();
    assert_eq!(
        validate_model_preparation_proposal(&input, &proposal).unwrap_err(),
        ModelPreparationProblem::CapabilitiesChanged
    );
    let (input, mut proposal) = fixture();
    proposal.candidate.skills.clear();
    assert_eq!(
        validate_model_preparation_proposal(&input, &proposal).unwrap_err(),
        ModelPreparationProblem::CapabilitiesChanged
    );
    let (input, mut proposal) = fixture();
    proposal.candidate.limits.model_tokens = Some(9_999);
    assert_eq!(
        validate_model_preparation_proposal(&input, &proposal).unwrap_err(),
        ModelPreparationProblem::CapabilitiesChanged
    );
}
#[test]
fn changed_history_or_context_needs_an_independently_verified_checkpoint() {
    let (input, mut proposal) = fixture();
    proposal.candidate.history.remove(0);
    assert_eq!(
        validate_model_preparation_proposal(&input, &proposal).unwrap_err(),
        ModelPreparationProblem::MissingCheckpoint
    );
    let (input, mut proposal) = fixture();
    proposal.candidate.context.clear();
    assert_eq!(
        validate_model_preparation_proposal(&input, &proposal).unwrap_err(),
        ModelPreparationProblem::MissingCheckpoint
    );
}
