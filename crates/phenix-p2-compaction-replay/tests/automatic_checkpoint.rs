//! P2-3v RED: pressure must not authorize an unverifiable checkpoint or an
//! over-window Model request. This policy is pure and has no storage effects.
use phenix_advanced_contracts::{model_request_sha256, ModelTokenMeasurement, ModelTokenProblem};
use phenix_basic_contracts::{Event, EventKind, Limits, Message, ModelRequest, Role};
use phenix_p2_compaction_replay::{
    automatic_checkpoint, AutomaticCheckpointDecision, AutomaticCheckpointProblem, CheckpointPlan,
    PlanProblem, PressureProblem,
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
fn fixture() -> (ModelRequest, Vec<Event>) {
    let mut journal = Vec::new();
    let mut history = Vec::new();
    for i in 1..=3 {
        let q = format!("question {i}");
        let a = format!("answer {i}");
        journal.push(event((i - 1) * 4 + 1, i, EventKind::PromptAccepted, &q));
        journal.push(event((i - 1) * 4 + 2, i, EventKind::ModelStarted, ""));
        journal.push(event((i - 1) * 4 + 3, i, EventKind::TextDelta, &a));
        journal.push(event((i - 1) * 4 + 4, i, EventKind::Answer, &a));
        history.push(message(Role::User, &q));
        history.push(message(Role::Assistant, &a));
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
            context: vec![],
            limits: Limits {
                model_tokens: Some(500),
                ..Limits::default()
            },
        },
        journal,
    )
}
fn meter(request: &ModelRequest, input_tokens: u64) -> ModelTokenMeasurement {
    ModelTokenMeasurement {
        model: "fixture:selected-model".into(),
        tokenizer_revision: "fixture:tokenizer-v1".into(),
        request_sha256: model_request_sha256(request).unwrap(),
        input_tokens,
        context_window_tokens: 10_000,
    }
}
fn plan<'a>() -> CheckpointPlan<'a> {
    CheckpointPlan {
        id: "cp-auto",
        revision: 1,
        parent_checkpoint: None,
        summary: "Verified earlier questions; originals remain in journal.",
        retain_completed_turns: 1,
    }
}

#[test]
fn valid_threshold_creates_only_a_candidate_and_does_not_mutate_history() {
    let (request, journal) = fixture();
    let original = request.clone();
    let outcome = automatic_checkpoint(
        &request,
        &meter(&request, 7_500),
        "fixture:selected-model",
        80,
        &journal,
        None,
        plan(),
    )
    .unwrap();
    match outcome {
        AutomaticCheckpointDecision::Candidate {
            checkpoint,
            mandatory,
        } => {
            assert!(!mandatory);
            assert_eq!(checkpoint.session, "s1");
            assert_eq!(checkpoint.summarized[0].first_sequence, 1);
            assert_eq!(checkpoint.summarized[0].last_sequence, 8);
            assert_eq!(checkpoint.retained[0].first_sequence, 9);
            assert_eq!(checkpoint.retained[0].last_sequence, 14);
        }
        other => panic!("expected verified candidate, got {other:?}"),
    }
    assert_eq!(request, original);
}

#[test]
fn enough_headroom_skips_checkpoint_sources_without_a_spurious_write() {
    let (request, _journal) = fixture();
    assert_eq!(
        automatic_checkpoint(
            &request,
            &meter(&request, 1_000),
            "fixture:selected-model",
            80,
            &[], // Not required for a request that already fits.
            None,
            plan(),
        ),
        Ok(AutomaticCheckpointDecision::UseOriginal),
    );
}

#[test]
fn at_threshold_without_complete_old_turns_can_continue_uncompacted() {
    let (request, journal) = fixture();
    let shortened = ModelRequest {
        history: request.history[4..].to_vec(),
        ..request
    };
    assert_eq!(
        automatic_checkpoint(
            &shortened,
            &meter(&shortened, 7_500),
            "fixture:selected-model",
            80,
            &journal[8..],
            None,
            plan(),
        ),
        Ok(AutomaticCheckpointDecision::UseOriginal),
    );
}

#[test]
fn over_window_requires_a_candidate_or_must_deny_model_admission() {
    let (request, journal) = fixture();
    let chosen = automatic_checkpoint(
        &request,
        &meter(&request, 9_501),
        "fixture:selected-model",
        80,
        &journal,
        None,
        plan(),
    )
    .unwrap();
    assert!(matches!(
        chosen,
        AutomaticCheckpointDecision::Candidate {
            mandatory: true,
            ..
        }
    ));
    let shortened = ModelRequest {
        history: request.history[4..].to_vec(),
        ..request
    };
    assert_eq!(
        automatic_checkpoint(
            &shortened,
            &meter(&shortened, 9_501),
            "fixture:selected-model",
            80,
            &journal[8..],
            None,
            plan(),
        ),
        Err(AutomaticCheckpointProblem::NoSafeCheckpoint),
    );
}

#[test]
fn forged_meter_and_invalid_journal_fail_closed() {
    let (request, mut journal) = fixture();
    let mut forged = meter(&request, 7_500);
    forged.request_sha256 = "f".repeat(64);
    assert_eq!(
        automatic_checkpoint(
            &request,
            &forged,
            "fixture:selected-model",
            80,
            &journal,
            None,
            plan(),
        ),
        Err(AutomaticCheckpointProblem::Pressure(
            PressureProblem::Measurement(ModelTokenProblem::RequestChanged)
        )),
    );
    journal[3].text = "tampered answer".into();
    assert_eq!(
        automatic_checkpoint(
            &request,
            &meter(&request, 7_500),
            "fixture:selected-model",
            80,
            &journal,
            None,
            plan(),
        ),
        Err(AutomaticCheckpointProblem::Plan(
            PlanProblem::UnverifiedHistory
        )),
    );
}

#[test]
fn impossible_output_reserve_does_not_trigger_futile_compaction() {
    let (request, journal) = fixture();
    let mut impossible = meter(&request, 1);
    impossible.context_window_tokens = 500;
    assert_eq!(
        automatic_checkpoint(
            &request,
            &impossible,
            "fixture:selected-model",
            80,
            &journal,
            None,
            plan(),
        ),
        Err(AutomaticCheckpointProblem::Pressure(
            PressureProblem::Measurement(ModelTokenProblem::OverBudget)
        )),
    );
}
