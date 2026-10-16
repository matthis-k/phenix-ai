//! RED P2-3g: a persisted checkpoint covers historical source bytes, while
//! the authorized Basic EventJournal can advance without mutating that checkpoint.
use phenix_advanced_contracts::CompactionCheckpoint;
use phenix_basic_contracts::{Event, EventKind};
use phenix_p2_compaction_replay::{
    extend_verified_view, span_for_events, ContextPart, ReplayProblem, TailProblem, ViewProblem,
};
use phenix_sdk::Blob;

fn event(sequence: u64, kind: EventKind, text: &str, id: &str) -> Event {
    Event {
        sequence,
        session: "s1".into(),
        run: format!("run-{}", (sequence + 1) / 2),
        kind,
        text: text.into(),
        call_id: id.into(),
        data: Blob(Vec::new()),
    }
}
fn history() -> Vec<Event> {
    vec![
        event(1, EventKind::PromptAccepted, "old question", ""),
        event(2, EventKind::Answer, "old answer", ""),
        event(3, EventKind::PromptAccepted, "newer question", ""),
        event(4, EventKind::Answer, "newer answer", ""),
    ]
}
fn checkpoint(events: &[Event]) -> CompactionCheckpoint {
    CompactionCheckpoint {
        id: "cp-1".into(), session: "s1".into(), revision: 1,
        parent_checkpoint: None, input: span_for_events(events).unwrap(),
        summarized: vec![span_for_events(&events[..2]).unwrap()],
        retained: vec![span_for_events(&events[2..]).unwrap()],
        excluded: vec![], summary: "source-backed older turn".into(),
        policy: "live-journal".into(), policy_version: 1, model: None,
        hydration_sources: vec![],
    }
}

#[test]
fn appended_events_are_exact_and_newly_cited_without_rewriting_checkpoint() {
    let prefix = history();
    let cp = checkpoint(&prefix);
    let tail = vec![
        event(5, EventKind::PromptAccepted, "latest question", ""),
        event(6, EventKind::Answer, "latest answer", ""),
    ];
    let projection = extend_verified_view(&cp, &prefix, &tail, 4096).unwrap();
    assert_eq!(projection.base.checkpoint_id, cp.id);
    assert_eq!(projection.base.source_span, cp.input);
    assert_eq!(projection.base.parts.len(), 3);
    assert_eq!(projection.trailing, tail);
    assert_eq!(projection.trailing_span, Some(span_for_events(&tail).unwrap()));
    assert!(matches!(projection.base.parts[0], ContextPart::Summary { .. }));
    assert!(projection.visible_payload_bytes >= projection.base.visible_payload_bytes);
    assert_eq!(prefix, history());
}

#[test]
fn empty_tail_and_exact_zero_length_extension_do_not_invent_history() {
    let prefix = history();
    let cp = checkpoint(&prefix);
    let projection = extend_verified_view(&cp, &prefix, &[], 4096).unwrap();
    assert!(projection.trailing.is_empty());
    assert_eq!(projection.trailing_span, None);
    assert_eq!(projection.visible_payload_bytes, projection.base.visible_payload_bytes);
}

#[test]
fn tampered_checkpoint_source_rejects_even_when_tail_is_intact() {
    let prefix = history();
    let cp = checkpoint(&prefix);
    let mut forged = prefix.clone();
    forged[1].text = "rewritten older answer".into();
    assert_eq!(
        extend_verified_view(&cp, &forged, &[event(5, EventKind::PromptAccepted, "new", "")], 4096),
        Err(TailProblem::Checkpoint(ViewProblem::Replay(ReplayProblem::DigestChanged))),
    );
}

#[test]
fn tail_requires_contiguous_sequences_and_exact_session_scope() {
    let prefix = history();
    let cp = checkpoint(&prefix);
    assert_eq!(
        extend_verified_view(&cp, &prefix, &[event(6, EventKind::PromptAccepted, "gap", "")], 4096),
        Err(TailProblem::InvalidSequence),
    );
    let mut wrong = event(5, EventKind::PromptAccepted, "other", "");
    wrong.session = "s2".into();
    assert_eq!(
        extend_verified_view(&cp, &prefix, &[wrong], 4096),
        Err(TailProblem::WrongSession),
    );
    assert_eq!(
        extend_verified_view(
            &cp, &prefix,
            &[event(5, EventKind::PromptAccepted, "ok", ""),
              event(5, EventKind::Answer, "duplicate", "")],
            4096
        ),
        Err(TailProblem::InvalidSequence),
    );
}

#[test]
fn in_flight_and_orphaned_tail_tool_effects_fail_closed() {
    let prefix = history();
    let cp = checkpoint(&prefix);
    assert_eq!(
        extend_verified_view(&cp, &prefix, &[event(5, EventKind::ToolStarted, "uppercase", "call-1")], 4096),
        Err(TailProblem::UnresolvedToolCall),
    );
    assert_eq!(
        extend_verified_view(&cp, &prefix, &[event(5, EventKind::ToolFinished, "output", "call-1")], 4096),
        Err(TailProblem::InvalidToolSequence),
    );
    let paired = vec![
        event(5, EventKind::ToolStarted, "uppercase", "call-1"),
        event(6, EventKind::ToolFinished, "output", "call-1"),
    ];
    assert_eq!(extend_verified_view(&cp, &prefix, &paired, 4096).unwrap().trailing, paired);
}

#[test]
fn total_byte_budget_applies_to_checkpoint_and_new_tail_together() {
    let prefix = history();
    let cp = checkpoint(&prefix);
    let tail = vec![event(5, EventKind::PromptAccepted, "a long new turn", "")];
    let base = extend_verified_view(&cp, &prefix, &[], 4096).unwrap();
    assert_eq!(
        extend_verified_view(&cp, &prefix, &tail, base.visible_payload_bytes),
        Err(TailProblem::OverBudget),
    );
    assert_eq!(
        extend_verified_view(&cp, &prefix, &[], 0),
        Err(TailProblem::InvalidBudget),
    );
}
