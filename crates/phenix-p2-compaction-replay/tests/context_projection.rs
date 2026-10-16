//! RED P2-3d acceptance: bounded context selection is a derived, reversible
//! view over verified journal bytes, never a mutation of Basic history.
use phenix_advanced_contracts::{CompactionCheckpoint, JournalSpan};
use phenix_basic_contracts::{Event, EventKind};
use phenix_p2_compaction_replay::{
    compacted_view, restore_original_events, span_for_events, ContextPart, ReplayProblem,
    ViewProblem,
};
use phenix_sdk::Blob;

fn event(number: u64, kind: EventKind, text: &str, call_id: &str) -> Event {
    Event {
        sequence: number,
        session: "s1".into(),
        run: "run-1".into(),
        kind,
        call_id: call_id.into(),
        text: text.into(),
        data: Blob(if number == 5 {
            b"exact-result".to_vec()
        } else {
            vec![]
        }),
    }
}

fn journal() -> Vec<Event> {
    vec![
        event(1, EventKind::PromptAccepted, "original question", ""),
        event(2, EventKind::Answer, "original answer", ""),
        event(3, EventKind::PromptAccepted, "new question", ""),
        event(4, EventKind::ToolStarted, "tool launched", "call-1"),
        event(5, EventKind::ToolFinished, "tool finished", "call-1"),
        event(6, EventKind::Answer, "new answer", ""),
    ]
}

fn checkpoint(source: &[Event], summary_end: usize, retained_end: usize) -> CompactionCheckpoint {
    let summarized = vec![span_for_events(&source[..summary_end]).unwrap()];
    let retained = if retained_end > summary_end {
        vec![span_for_events(&source[summary_end..retained_end]).unwrap()]
    } else {
        vec![]
    };
    let excluded = if retained_end < source.len() {
        vec![span_for_events(&source[retained_end..]).unwrap()]
    } else {
        vec![]
    };
    CompactionCheckpoint {
        id: "cp-1".into(),
        session: "s1".into(),
        revision: 1,
        parent_checkpoint: None,
        input: span_for_events(source).unwrap(),
        summarized,
        retained,
        excluded,
        summary: "Old exchange (source-cited)".into(),
        policy: "recent-tool-context".into(),
        policy_version: 1,
        model: None,
        hydration_sources: vec![],
    }
}

#[test]
fn compacted_view_is_bounded_ordered_and_reversible_without_executing_tools() {
    let originals = journal();
    let cp = checkpoint(&originals, 2, 6);
    let view = compacted_view(&cp, &originals, 256).unwrap();
    assert_eq!(view.checkpoint_id, "cp-1");
    assert_eq!(view.source_span, cp.input);
    assert_eq!(view.parts.len(), 5);
    let ContextPart::Summary { text, sources, .. } = &view.parts[0] else {
        panic!("the older exchange must be a cited summary");
    };
    assert_eq!(text, "Old exchange (source-cited)");
    assert_eq!(sources, &cp.summarized);
    for (index, source) in originals[2..].iter().enumerate() {
        assert_eq!(view.parts[index + 1], ContextPart::Original(source.clone()));
    }
    assert!(view.visible_payload_bytes <= 256);
    assert_eq!(restore_original_events(&cp, &originals).unwrap(), originals);
    assert_eq!(originals[4].data.0, b"exact-result");
}

#[test]
fn a_tool_call_cannot_be_split_between_summary_and_visible_events() {
    let originals = journal();
    let cp = checkpoint(&originals, 4, 6);
    assert_eq!(
        compacted_view(&cp, &originals, 512),
        Err(ViewProblem::SplitToolCall),
    );
}

#[test]
fn in_flight_tool_calls_block_compaction() {
    let originals = journal();
    let interrupted = &originals[..4];
    let cp = checkpoint(interrupted, 2, 4);
    assert_eq!(
        compacted_view(&cp, interrupted, 512),
        Err(ViewProblem::UnresolvedToolCall),
    );
}

#[test]
fn a_tight_budget_refuses_truncation_instead_of_losing_provenance() {
    let originals = journal();
    let cp = checkpoint(&originals, 2, 6);
    assert_eq!(
        compacted_view(&cp, &originals, 0),
        Err(ViewProblem::InvalidBudget)
    );
    assert_eq!(
        compacted_view(&cp, &originals, 12),
        Err(ViewProblem::OverBudget)
    );
    assert_eq!(originals.len(), 6);
}

#[test]
fn excluded_ranges_are_audited_not_reconstructed() {
    let originals = journal();
    let cp = checkpoint(&originals, 2, 5);
    let view = compacted_view(&cp, &originals, 512).unwrap();
    assert_eq!(view.excluded, cp.excluded);
    assert_eq!(view.parts.len(), 4);
    assert!(view.parts.iter().all(|part| match part {
        ContextPart::Original(event) => event.sequence != 6,
        _ => true,
    }));
}

#[test]
fn changed_source_or_forged_ranges_cannot_be_projected() {
    let originals = journal();
    let cp = checkpoint(&originals, 2, 6);
    let mut changed = originals.clone();
    changed[4].text = "forged tool result".into();
    assert_eq!(
        compacted_view(&cp, &changed, 512),
        Err(ViewProblem::Replay(ReplayProblem::DigestChanged)),
    );
    let mut forged = cp.clone();
    forged.excluded.push(JournalSpan {
        first_sequence: 5,
        last_sequence: 6,
        digest_sha256: "f".repeat(64),
    });
    assert_eq!(
        compacted_view(&forged, &originals, 512),
        Err(ViewProblem::Replay(ReplayProblem::InvalidCheckpoint)),
    );
}

#[test]
fn single_summary_must_be_an_initial_contiguous_prefix() {
    let originals = journal();
    let mut cp = checkpoint(&originals, 2, 6);
    // Structural partitioning alone accepts these spans, but emitting one
    // summary before retained text would rewrite the original chronology.
    cp.summarized = vec![span_for_events(&originals[2..4]).unwrap()];
    cp.retained = vec![
        span_for_events(&originals[..2]).unwrap(),
        span_for_events(&originals[4..]).unwrap(),
    ];
    assert_eq!(
        compacted_view(&cp, &originals, 512),
        Err(ViewProblem::NonPrefixSummary),
    );
}

#[test]
fn tool_start_and_finish_must_share_the_same_visibility_class() {
    let originals = journal();
    let mut cp = checkpoint(&originals, 3, 6);
    // Preserve chronological coverage, but split the pair between excluded
    // and retained. Neither may be silently hidden by the other.
    cp.summarized = vec![span_for_events(&originals[..4]).unwrap()];
    cp.excluded = vec![span_for_events(&originals[4..5]).unwrap()];
    cp.retained = vec![span_for_events(&originals[5..]).unwrap()];
    assert_eq!(
        compacted_view(&cp, &originals, 512),
        Err(ViewProblem::SplitToolCall),
    );
}
