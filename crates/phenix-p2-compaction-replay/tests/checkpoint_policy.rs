//! P2-3t RED: only source-verified complete turns may enter a proposed
//! checkpoint. Proposal is pure and never commits the journal or runs tools.
use phenix_basic_contracts::{Event, EventKind, Limits, Message, ModelRequest, Role};
use phenix_p2_compaction_replay::{
    assemble_verified_basic_request, compacted_view, plan_verified_checkpoint, span_for_events,
    CheckpointPlan, PlanProblem,
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
    match role {
        Role::User => Message::user(text),
        Role::Assistant => Message::assistant(text, vec![]),
        Role::Tool => panic!("tool fixtures require a call ID and explicit outcome"),
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
            context: vec![],
            limits: Limits::default(),
        },
        journal,
    )
}
fn policy<'a>() -> CheckpointPlan<'a> {
    CheckpointPlan {
        id: "cp-auto",
        revision: 1,
        parent_checkpoint: None,
        summary: "Verified older questions; full originals available in journal.",
        retain_completed_turns: 1,
    }
}

#[test]
fn proposes_complete_cited_prefix_without_modifying_authoritative_history() {
    let (request, journal) = source();
    let original = request.clone();
    let candidate = plan_verified_checkpoint(&request, &journal, None, policy())
        .unwrap()
        .expect("old turns ready");
    assert_eq!(
        candidate.summarized,
        vec![span_for_events(&journal[..8]).unwrap()]
    );
    assert_eq!(
        candidate.retained,
        vec![span_for_events(&journal[8..]).unwrap()]
    );
    assert_eq!(candidate.input, span_for_events(&journal).unwrap());
    assert_eq!(candidate.parent_checkpoint, None);
    assert_eq!(candidate.model, None);
    let view = compacted_view(&candidate, &journal, 8 * 1024 * 1024).unwrap();
    let submitted = assemble_verified_basic_request(&request, &journal, &candidate, &view).unwrap();
    assert_eq!(submitted.history, request.history[4..]);
    assert_eq!(submitted.context[0].id, "compaction:cp-auto");
    assert_eq!(request, original);
}

#[test]
fn refuses_incomplete_turns_and_unverified_history() {
    let (request, mut journal) = source();
    let mut forged = request.clone();
    forged.history[0].text = "invented".into();
    assert!(matches!(
        plan_verified_checkpoint(&forged, &journal, None, policy()),
        Err(PlanProblem::UnverifiedHistory),
    ));
    journal[6].sequence = 42;
    assert!(matches!(
        plan_verified_checkpoint(&request, &journal, None, policy()),
        Err(PlanProblem::InvalidSource),
    ));
    let (_, mut journal) = source();
    journal[7].kind = EventKind::Failure;
    assert!(matches!(
        plan_verified_checkpoint(&request, &journal, None, policy()),
        Err(PlanProblem::UnfinishedTurn),
    ));
}

#[test]
fn insufficient_completed_history_is_a_noop_and_stays_uncommitted() {
    let (request, journal) = source();
    let short_request = ModelRequest {
        history: request.history[4..].to_vec(),
        ..request.clone()
    };
    let short_journal = [journal[8..].to_vec()].concat();
    assert_eq!(
        plan_verified_checkpoint(&short_request, &short_journal, None, policy()),
        Ok(None),
    );
    assert_eq!(
        plan_verified_checkpoint(
            &request,
            &journal,
            None,
            CheckpointPlan {
                retain_completed_turns: 3,
                ..policy()
            },
        ),
        Ok(None),
    );
}

#[test]
fn rejects_forged_previous_checkpoint_and_bad_revision_or_summary() {
    let (request, journal) = source();
    let first = plan_verified_checkpoint(&request, &journal, None, policy())
        .unwrap()
        .unwrap();
    let mut bad = first.clone();
    bad.input.digest_sha256 = "f".repeat(64);
    assert!(matches!(
        plan_verified_checkpoint(
            &request,
            &journal,
            Some(&bad),
            CheckpointPlan {
                id: "cp-next",
                revision: 2,
                parent_checkpoint: Some("cp-auto"),
                ..policy()
            }
        ),
        Err(PlanProblem::InvalidSource),
    ));
    assert!(matches!(
        plan_verified_checkpoint(
            &request,
            &journal,
            Some(&first),
            CheckpointPlan {
                id: "cp-next",
                revision: 2,
                parent_checkpoint: None,
                ..policy()
            }
        ),
        Err(PlanProblem::InvalidLineage),
    ));
    assert!(matches!(
        plan_verified_checkpoint(
            &request,
            &journal,
            None,
            CheckpointPlan {
                summary: " ",
                ..policy()
            }
        ),
        Err(PlanProblem::InvalidPolicy),
    ));
}

#[test]
fn repeated_source_proposal_is_noop_without_spending_a_revision() {
    let (request, journal) = source();
    let first = plan_verified_checkpoint(&request, &journal, None, policy())
        .unwrap()
        .unwrap();
    let repeated = plan_verified_checkpoint(
        &request,
        &journal,
        Some(&first),
        CheckpointPlan {
            id: "cp-auto-next",
            revision: 2,
            parent_checkpoint: Some("cp-auto"),
            ..policy()
        },
    );
    assert_eq!(repeated, Ok(None));
    let mut changed_session = request.clone();
    changed_session.history[0].text = "forged earlier turn".into();
    assert!(matches!(
        plan_verified_checkpoint(
            &changed_session,
            &journal,
            Some(&first),
            CheckpointPlan {
                id: "cp-auto-next",
                revision: 2,
                parent_checkpoint: Some("cp-auto"),
                ..policy()
            },
        ),
        Err(PlanProblem::UnverifiedHistory),
    ));
}

#[test]
fn never_summarizes_unattested_legacy_tool_turns() {
    let (mut request, mut journal) = source();
    // Convert the second completed turn into a tool-bearing turn with no
    // AssistantReply receipt. Retained source cannot attest tool arguments.
    journal.insert(7, event(8, 2, EventKind::ToolStarted, "uppercase"));
    journal.insert(8, event(9, 2, EventKind::ToolFinished, "HELLO"));
    journal[7].call_id = "t1".into();
    journal[8].call_id = "t1".into();
    for (index, entry) in journal.iter_mut().enumerate() {
        entry.sequence = index as u64 + 1;
    }
    request.history.insert(
        3,
        Message::assistant(String::new(), vec![phenix_basic_contracts::ToolCall {
                id: "t1".into(),
                tool: "uppercase".into(),
                arguments: r#""secret""#.into(),
            }]),
    );
    request.history.insert(
        4,
        Message::tool("t1", phenix_basic_contracts::ToolOutcome::Succeeded { text: "HELLO".into() }),
    );
    let planned = plan_verified_checkpoint(&request, &journal, None, policy());
    assert!(matches!(planned, Err(PlanProblem::UnverifiedHistory)));
}
