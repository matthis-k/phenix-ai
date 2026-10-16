//! P2-3w RED: a checkpoint candidate is not Model admission until the exact
//! source-rebuilt request has been metered by the same selected Model.
use phenix_advanced_contracts::{model_request_sha256, ModelTokenMeasurement, ModelTokenProblem};
use phenix_basic_contracts::{ContextItem, Event, EventKind, Limits, Message, ModelRequest, Role};
use phenix_p2_compaction_replay::{
    plan_verified_checkpoint, remeasure_verified_checkpoint, CheckpointPlan,
    CompactedAdmissionProblem, PressureProblem,
};
use phenix_sdk::Blob;

fn event(sequence: u64, run: u64, kind: EventKind, text: &str) -> Event {
    Event {
        sequence,
        session: "s1".into(),
        run: format!("run-{run}"),
        kind,
        call_id: String::new(),
        text: text.into(),
        data: Blob(vec![]),
    }
}

fn message(role: Role, text: &str) -> Message {
    Message {
        role,
        text: text.into(),
        calls: vec![],
        call_id: String::new(),
        error: None,
    }
}

fn source() -> (ModelRequest, Vec<Event>) {
    let mut journal = Vec::new();
    let mut history = Vec::new();
    for i in 1..=3 {
        let question = format!("question {i}");
        let answer = format!("answer {i}");
        journal.push(event(
            (i - 1) * 4 + 1,
            i,
            EventKind::PromptAccepted,
            &question,
        ));
        journal.push(event((i - 1) * 4 + 2, i, EventKind::ModelStarted, ""));
        journal.push(event((i - 1) * 4 + 3, i, EventKind::TextDelta, &answer));
        journal.push(event((i - 1) * 4 + 4, i, EventKind::Answer, &answer));
        history.push(message(Role::User, &question));
        history.push(message(Role::Assistant, &answer));
    }
    journal.push(event(13, 4, EventKind::PromptAccepted, "now"));
    journal.push(event(14, 4, EventKind::ModelStarted, ""));
    history.push(message(Role::User, "now"));
    (
        ModelRequest {
            session: "s1".into(),
            run: "run-4".into(),
            history,
            tools: vec![],
            skills: vec![],
            context: vec![ContextItem {
                id: "project-policy".into(),
                text: "Keep citations.".into(),
            }],
            limits: Limits {
                model_tokens: Some(500),
                ..Limits::default()
            },
        },
        journal,
    )
}

fn measurement(
    request: &ModelRequest,
    input_tokens: u64,
    window_tokens: u64,
) -> ModelTokenMeasurement {
    ModelTokenMeasurement {
        model: "fixture:selected-model".into(),
        tokenizer_revision: "fixture:tokenizer-v1".into(),
        request_sha256: model_request_sha256(request).unwrap(),
        input_tokens,
        context_window_tokens: window_tokens,
    }
}

fn checkpoint(
    request: &ModelRequest,
    journal: &[Event],
) -> phenix_advanced_contracts::CompactionCheckpoint {
    plan_verified_checkpoint(
        request,
        journal,
        None,
        CheckpointPlan {
            id: "cp-next",
            revision: 1,
            parent_checkpoint: None,
            summary: "Questions one and two were answered, with journal citations.",
            retain_completed_turns: 1,
        },
    )
    .unwrap()
    .expect("two complete older turns")
}

#[test]
fn rebuilt_request_is_remeasured_and_keeps_unmodified_authoritative_fields() {
    let (original, journal) = source();
    let checkpoint = checkpoint(&original, &journal);
    let original_before = original.clone();
    let mut calls = 0;
    let measured = remeasure_verified_checkpoint(
        &original,
        &journal,
        &checkpoint,
        &measurement(&original, 9_501, 10_000),
        "fixture:selected-model",
        8 * 1024 * 1024,
        |rebuilt| {
            calls += 1;
            assert_eq!(rebuilt.history, original.history[4..]);
            assert_eq!(rebuilt.session, original.session);
            assert_eq!(rebuilt.run, original.run);
            assert_eq!(rebuilt.limits, original.limits);
            assert_eq!(rebuilt.tools, original.tools);
            assert_eq!(rebuilt.skills, original.skills);
            assert_eq!(rebuilt.context[0], original.context[0]);
            assert_eq!(rebuilt.context[1].id, "compaction:cp-next");
            Ok::<_, ()>(measurement(rebuilt, 2_400, 10_000))
        },
    )
    .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(measured.request.history.len(), 3);
    assert_eq!(measured.measurement.input_tokens, 2_400);
    assert_eq!(
        measured.measurement.request_sha256,
        model_request_sha256(&measured.request).unwrap()
    );
    assert_eq!(original, original_before);
}

#[test]
fn refuses_unmodified_over_window_request_if_compacted_request_still_does_not_fit() {
    let (original, journal) = source();
    let checkpoint = checkpoint(&original, &journal);
    let result = remeasure_verified_checkpoint(
        &original,
        &journal,
        &checkpoint,
        &measurement(&original, 9_501, 10_000),
        "fixture:selected-model",
        8 * 1024 * 1024,
        |rebuilt| Ok::<_, ()>(measurement(rebuilt, 9_501, 10_000)),
    );
    assert_eq!(
        result,
        Err(CompactedAdmissionProblem::CandidateMeasurement(
            ModelTokenProblem::OverBudget
        ))
    );
}

#[test]
fn refuses_forged_compacted_request_digest_and_provider_revision_drift() {
    let (original, journal) = source();
    let checkpoint = checkpoint(&original, &journal);
    let forged = remeasure_verified_checkpoint(
        &original,
        &journal,
        &checkpoint,
        &measurement(&original, 9_501, 10_000),
        "fixture:selected-model",
        8 * 1024 * 1024,
        |rebuilt| {
            let mut fake = measurement(rebuilt, 2_400, 10_000);
            fake.request_sha256 = "f".repeat(64);
            Ok::<_, ()>(fake)
        },
    );
    assert_eq!(
        forged,
        Err(CompactedAdmissionProblem::CandidateMeasurement(
            ModelTokenProblem::RequestChanged
        ))
    );

    let changed_tokenizer = remeasure_verified_checkpoint(
        &original,
        &journal,
        &checkpoint,
        &measurement(&original, 9_501, 10_000),
        "fixture:selected-model",
        8 * 1024 * 1024,
        |rebuilt| {
            let mut next = measurement(rebuilt, 2_400, 10_000);
            next.tokenizer_revision = "fixture:tokenizer-v2".into();
            Ok::<_, ()>(next)
        },
    );
    assert_eq!(
        changed_tokenizer,
        Err(CompactedAdmissionProblem::ModelConfigurationChanged)
    );
    let changed_window = remeasure_verified_checkpoint(
        &original,
        &journal,
        &checkpoint,
        &measurement(&original, 9_501, 10_000),
        "fixture:selected-model",
        8 * 1024 * 1024,
        |rebuilt| Ok::<_, ()>(measurement(rebuilt, 2_400, 9_000)),
    );
    assert_eq!(
        changed_window,
        Err(CompactedAdmissionProblem::ModelConfigurationChanged)
    );
}

#[test]
fn forged_original_and_deleted_journal_source_block_remeasurement() {
    let (original, mut journal) = source();
    let checkpoint = checkpoint(&original, &journal);
    let mut wrong_model = measurement(&original, 9_501, 10_000);
    wrong_model.model = "fixture:other-model".into();
    let mut calls = 0;
    let result = remeasure_verified_checkpoint(
        &original,
        &journal,
        &checkpoint,
        &wrong_model,
        "fixture:selected-model",
        8 * 1024 * 1024,
        |_rebuilt| {
            calls += 1;
            Ok::<_, ()>(measurement(&original, 2_400, 10_000))
        },
    );
    assert_eq!(
        result,
        Err(CompactedAdmissionProblem::OriginalMeasurement(
            PressureProblem::Measurement(ModelTokenProblem::ModelChanged)
        ))
    );
    assert_eq!(calls, 0);

    journal.remove(0);
    let result = remeasure_verified_checkpoint(
        &original,
        &journal,
        &checkpoint,
        &measurement(&original, 9_501, 10_000),
        "fixture:selected-model",
        8 * 1024 * 1024,
        |_rebuilt| {
            calls += 1;
            Ok::<_, ()>(measurement(&original, 2_400, 10_000))
        },
    );
    assert!(result.is_err());
    assert_eq!(calls, 0);
}

#[test]
fn selected_meter_failure_is_not_interpreted_as_token_capacity() {
    let (original, journal) = source();
    let checkpoint = checkpoint(&original, &journal);
    let result = remeasure_verified_checkpoint(
        &original,
        &journal,
        &checkpoint,
        &measurement(&original, 9_501, 10_000),
        "fixture:selected-model",
        8 * 1024 * 1024,
        |_rebuilt| -> Result<ModelTokenMeasurement, &'static str> {
            Err("selected meter unavailable")
        },
    );
    assert_eq!(
        result,
        Err(CompactedAdmissionProblem::Meter(
            "selected meter unavailable"
        ))
    );
}
