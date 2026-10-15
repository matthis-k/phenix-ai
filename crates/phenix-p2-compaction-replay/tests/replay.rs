//! RED P2-3b acceptance: original Basic journal events are the only source
//! for an exact compaction expansion; summaries never fabricate event bytes.
use phenix_advanced_contracts::{CompactionCheckpoint, JournalSpan};
use phenix_basic_contracts::{Event, EventKind};
use phenix_p2_compaction_replay::{restore_original_events, span_for_events, ReplayProblem};
use phenix_sdk::Blob;

fn event(sequence: u64, kind: EventKind, text: &str, bytes: &[u8]) -> Event {
    Event {
        sequence,
        session: "s1".into(),
        run: "r1".into(),
        kind,
        call_id: if sequence == 2 || sequence == 3 { "call-1" } else { "" }.into(),
        text: text.into(),
        data: Blob(bytes.to_vec()),
    }
}

fn history() -> Vec<Event> {
    vec![
        event(1, EventKind::PromptAccepted, "question", b""),
        event(2, EventKind::ToolStarted, "execute", b"command"),
        event(3, EventKind::ToolFinished, "result", b"exact\0result"),
        event(4, EventKind::Answer, "answer", b""),
    ]
}

fn checkpoint(events: &[Event]) -> CompactionCheckpoint {
    let input = span_for_events(events).unwrap();
    CompactionCheckpoint {
        id: "cp1".into(),
        session: "s1".into(),
        revision: 1,
        parent_checkpoint: None,
        input,
        summarized: vec![span_for_events(&events[..2]).unwrap()],
        retained: vec![span_for_events(&events[2..3]).unwrap()],
        excluded: vec![span_for_events(&events[3..]).unwrap()],
        summary: "Earlier question and tool initiation".into(),
        policy: "fixed-test-policy".into(),
        policy_version: 1,
        model: None,
        hydration_sources: vec![],
    }
}

#[test]
fn expands_every_original_event_byte_without_replaying_tool_effects() {
    let original = history();
    let cp = checkpoint(&original);
    let restored = restore_original_events(&cp, &original).unwrap();
    assert_eq!(restored, original);
    assert_eq!(restored[2].data.0, b"exact\0result");
    assert_eq!(restored[2].call_id, "call-1");
}

#[test]
fn changed_message_or_tool_payload_invalidates_checkpoint() {
    let original = history();
    let cp = checkpoint(&original);
    let mut modified = original.clone();
    modified[0].text = "forged".into();
    assert_eq!(
        restore_original_events(&cp, &modified),
        Err(ReplayProblem::DigestChanged),
    );
    modified = original.clone();
    modified[2].data = Blob(b"different result".to_vec());
    assert_eq!(
        restore_original_events(&cp, &modified),
        Err(ReplayProblem::DigestChanged),
    );
    let mut altered = cp.clone();
    altered.retained[0].digest_sha256 = "f".repeat(64);
    assert_eq!(
        restore_original_events(&altered, &original),
        Err(ReplayProblem::DigestChanged),
    );
}

#[test]
fn missing_reordered_or_repeated_events_cannot_pass_expansion() {
    let original = history();
    let cp = checkpoint(&original);
    let mut missing = original.clone();
    missing.remove(1);
    assert_eq!(
        restore_original_events(&cp, &missing),
        Err(ReplayProblem::IncompleteJournal),
    );
    let mut reordered = original.clone();
    reordered.swap(1, 2);
    assert_eq!(
        restore_original_events(&cp, &reordered),
        Err(ReplayProblem::InvalidSequence),
    );
    let mut duplicated = original.clone();
    duplicated[2].sequence = 2;
    assert_eq!(
        restore_original_events(&cp, &duplicated),
        Err(ReplayProblem::InvalidSequence),
    );
}

#[test]
fn wrong_session_or_unanchored_ranges_are_denied() {
    let original = history();
    let cp = checkpoint(&original);
    let mut foreign = original.clone();
    foreign[1].session = "s2".into();
    assert_eq!(
        restore_original_events(&cp, &foreign),
        Err(ReplayProblem::WrongSession),
    );
    let mut forged = cp.clone();
    forged.input = JournalSpan {
        first_sequence: 2,
        ..cp.input.clone()
    };
    assert_eq!(
        restore_original_events(&forged, &original),
        Err(ReplayProblem::InvalidCheckpoint),
    );
}

#[test]
fn source_erasure_is_not_hidden_by_a_stored_summary() {
    let original = history();
    let cp = checkpoint(&original);
    assert_eq!(
        restore_original_events(&cp, &[]),
        Err(ReplayProblem::IncompleteJournal),
    );
    assert!(span_for_events(&[]).is_err());
}
