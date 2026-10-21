//! P2-3z RED: a single pre-Model admission transaction must compose
//! pressure, exact journal validation, selected CAS and exact remeasurement.
use phenix_advanced_contracts::{
    model_request_sha256, CompactionCheckpoint, ModelTokenMeasurement, ModelTokenProblem,
};
use phenix_basic_contracts::{Event, EventKind, Limits, Message, ModelRequest, Role};
use phenix_p2_compaction_replay::{
    admit_compaction, AutomaticCheckpointProblem, CheckpointPlan, CheckpointPublishProblem,
    CompactedAdmissionProblem, CompactionAdmissionInput, CompactionAdmissionProblem,
    PressureProblem,
};
use phenix_sdk::Blob;
use std::cell::{Cell, RefCell};

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
    match role {
        Role::User => Message::user(text),
        Role::Assistant => Message::assistant(text, vec![]),
        Role::Tool => panic!("tool fixtures require a call ID and explicit outcome"),
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
    let request = ModelRequest {
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
    };
    (request, journal)
}
fn meter(request: &ModelRequest, tokens: u64) -> ModelTokenMeasurement {
    ModelTokenMeasurement {
        model: "fixture:selected-model".into(),
        tokenizer_revision: "fixture:v1".into(),
        request_sha256: model_request_sha256(request).unwrap(),
        input_tokens: tokens,
        context_window_tokens: 10_000,
    }
}
fn plan<'a>() -> CheckpointPlan<'a> {
    CheckpointPlan {
        id: "cp-txn",
        revision: 1,
        parent_checkpoint: None,
        summary: "Questions one and two were answered; consult cited original journal.",
        retain_completed_turns: 1,
    }
}

#[test]
fn enough_headroom_admits_original_without_any_store_or_remeasurement() {
    let (request, _journal) = fixture();
    let called = Cell::new(0);
    let ready = admit_compaction(
        CompactionAdmissionInput {
            request: &request,
            original_measurement: &meter(&request, 1200),
            selected_model: "fixture:selected-model",
            high_water_percent: 80,
            journal: &[],
            previous: None,
            policy: plan(),
        },
        || {
            called.set(called.get() + 1);
            Err::<Option<CompactionCheckpoint>, &str>("unexpected store read")
        },
        |_cp| {
            called.set(called.get() + 1);
            Err::<CompactionCheckpoint, &str>("unexpected CAS")
        },
        |_next| {
            called.set(called.get() + 1);
            Err::<ModelTokenMeasurement, &str>("unexpected meter")
        },
    )
    .unwrap();
    assert_eq!(ready.request, request);
    assert_eq!(ready.measurement.input_tokens, 1200);
    assert!(ready.checkpoint.is_none());
    assert!(!ready.mandatory && !ready.created);
    assert_eq!(called.get(), 0);
}

#[test]
fn selected_cas_then_exact_selected_remeasurement_admits_only_source_verified_request() {
    let (request, journal) = fixture();
    let source = request.clone();
    let durable = RefCell::new(None::<CompactionCheckpoint>);
    let writes = Cell::new(0);
    let metering = Cell::new(0);
    let ready = admit_compaction(
        CompactionAdmissionInput {
            request: &request,
            original_measurement: &meter(&request, 9501),
            selected_model: "fixture:selected-model",
            high_water_percent: 80,
            journal: &journal,
            previous: None,
            policy: plan(),
        },
        || Ok::<_, &str>(durable.borrow().clone()),
        |cp| {
            writes.set(writes.get() + 1);
            *durable.borrow_mut() = Some(cp.clone());
            Ok::<_, &str>(cp.clone())
        },
        |rebuilt| {
            metering.set(metering.get() + 1);
            assert_eq!(rebuilt.history, source.history[4..]);
            assert_eq!(rebuilt.context[0].id, "compaction:cp-txn");
            assert!(durable.borrow().is_some(), "CAS must precede remeasurement");
            Ok::<_, &str>(meter(rebuilt, 2000))
        },
    )
    .unwrap();
    assert_eq!(
        request, source,
        "admission never rewrites the Basic session"
    );
    assert_eq!(writes.get(), 1);
    assert_eq!(metering.get(), 1);
    assert!(ready.mandatory && ready.created);
    assert_eq!(ready.checkpoint.unwrap(), durable.borrow().clone().unwrap());
    assert_eq!(
        ready.measurement.request_sha256,
        model_request_sha256(&ready.request).unwrap()
    );
}

#[test]
fn threshold_without_eligible_turns_uses_original_without_store_access() {
    let (request, journal) = fixture();
    let shortened = ModelRequest {
        history: request.history[4..].to_vec(),
        ..request
    };
    let calls = Cell::new(0);
    let ready = admit_compaction(
        CompactionAdmissionInput {
            request: &shortened,
            original_measurement: &meter(&shortened, 7500),
            selected_model: "fixture:selected-model",
            high_water_percent: 80,
            journal: &journal[8..],
            previous: None,
            policy: plan(),
        },
        || {
            calls.set(calls.get() + 1);
            Err::<Option<CompactionCheckpoint>, &str>("unexpected")
        },
        |_cp| {
            calls.set(calls.get() + 1);
            Err::<CompactionCheckpoint, &str>("unexpected")
        },
        |_next| {
            calls.set(calls.get() + 1);
            Err::<ModelTokenMeasurement, &str>("unexpected")
        },
    )
    .unwrap();
    assert_eq!(ready.request, shortened);
    assert!(ready.checkpoint.is_none());
    assert_eq!(calls.get(), 0);
}

#[test]
fn an_overwindow_request_without_safe_checkpoint_never_reads_or_writes_store() {
    let (request, journal) = fixture();
    let shortened = ModelRequest {
        history: request.history[4..].to_vec(),
        ..request
    };
    let writes = Cell::new(0);
    let result = admit_compaction(
        CompactionAdmissionInput {
            request: &shortened,
            original_measurement: &meter(&shortened, 9501),
            selected_model: "fixture:selected-model",
            high_water_percent: 80,
            journal: &journal[8..],
            previous: None,
            policy: plan(),
        },
        || {
            writes.set(writes.get() + 1);
            Ok::<_, &str>(None)
        },
        |_cp| {
            writes.set(writes.get() + 1);
            Err::<CompactionCheckpoint, &str>("unexpected")
        },
        |_next| {
            writes.set(writes.get() + 1);
            Err::<ModelTokenMeasurement, &str>("unexpected")
        },
    );
    assert_eq!(
        result,
        Err(CompactionAdmissionProblem::Decision(
            AutomaticCheckpointProblem::NoSafeCheckpoint
        ))
    );
    assert_eq!(writes.get(), 0);
}

#[test]
fn conflicting_cas_and_model_config_changes_fail_closed_before_model() {
    let (request, journal) = fixture();
    let c = Cell::new(0);
    let denied = admit_compaction(
        CompactionAdmissionInput {
            request: &request,
            original_measurement: &meter(&request, 9501),
            selected_model: "fixture:selected-model",
            high_water_percent: 80,
            journal: &journal,
            previous: None,
            policy: plan(),
        },
        || {
            Ok::<_, &str>(Some(CompactionCheckpoint {
                id: "competing".into(),
                revision: 1,
                ..phenix_p2_compaction_replay::plan_verified_checkpoint(
                    &request,
                    &journal,
                    None,
                    plan(),
                )
                .unwrap()
                .unwrap()
            }))
        },
        |_cp| {
            c.set(c.get() + 1);
            Err::<CompactionCheckpoint, &str>("unexpected CAS")
        },
        |_next| {
            c.set(c.get() + 1);
            Err::<ModelTokenMeasurement, &str>("unexpected meter")
        },
    );
    assert_eq!(
        denied,
        Err(CompactionAdmissionProblem::Publication(
            CheckpointPublishProblem::StaleHead
        ))
    );
    assert_eq!(c.get(), 0);

    let durable = RefCell::new(None);
    let denied = admit_compaction(
        CompactionAdmissionInput {
            request: &request,
            original_measurement: &meter(&request, 9501),
            selected_model: "fixture:selected-model",
            high_water_percent: 80,
            journal: &journal,
            previous: None,
            policy: plan(),
        },
        || Ok::<_, &str>(durable.borrow().clone()),
        |cp| {
            *durable.borrow_mut() = Some(cp.clone());
            Ok::<_, &str>(cp.clone())
        },
        |rebuilt| {
            let mut changed = meter(rebuilt, 2000);
            changed.tokenizer_revision = "new-tokenizer".into();
            Ok::<_, &str>(changed)
        },
    );
    assert_eq!(
        denied,
        Err(CompactionAdmissionProblem::Remeasure(
            CompactedAdmissionProblem::ModelConfigurationChanged
        ))
    );
}

#[test]
fn verified_compaction_cannot_authorize_still_overbudget_model_request() {
    let (request, journal) = fixture();
    let durable = RefCell::new(None);
    let outcome = admit_compaction(
        CompactionAdmissionInput {
            request: &request,
            original_measurement: &meter(&request, 9501),
            selected_model: "fixture:selected-model",
            high_water_percent: 80,
            journal: &journal,
            previous: None,
            policy: plan(),
        },
        || Ok::<_, &str>(durable.borrow().clone()),
        |cp| {
            *durable.borrow_mut() = Some(cp.clone());
            Ok::<_, &str>(cp.clone())
        },
        |rebuilt| Ok::<_, &str>(meter(rebuilt, 9900)),
    );
    assert_eq!(
        outcome,
        Err(CompactionAdmissionProblem::Remeasure(
            CompactedAdmissionProblem::CandidateMeasurement(ModelTokenProblem::OverBudget)
        ))
    );
    assert!(
        durable.borrow().is_some(),
        "a committed source-backed checkpoint remains durable"
    );
}

#[test]
fn forged_original_meter_rejects_before_source_and_store() {
    let (request, journal) = fixture();
    let mut forged = meter(&request, 9501);
    forged.request_sha256 = "f".repeat(64);
    let count = Cell::new(0);
    let result = admit_compaction(
        CompactionAdmissionInput {
            request: &request,
            original_measurement: &forged,
            selected_model: "fixture:selected-model",
            high_water_percent: 80,
            journal: &journal,
            previous: None,
            policy: plan(),
        },
        || {
            count.set(count.get() + 1);
            Err::<Option<CompactionCheckpoint>, &str>("unexpected")
        },
        |_cp| {
            count.set(count.get() + 1);
            Err::<CompactionCheckpoint, &str>("unexpected")
        },
        |_next| {
            count.set(count.get() + 1);
            Err::<ModelTokenMeasurement, &str>("unexpected")
        },
    );
    assert_eq!(
        result,
        Err(CompactionAdmissionProblem::Decision(
            AutomaticCheckpointProblem::Pressure(PressureProblem::Measurement(
                ModelTokenProblem::RequestChanged
            ))
        ))
    );
    assert_eq!(count.get(), 0);
}
