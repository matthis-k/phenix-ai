//! P2-3c RED: single-session checkpoint ledger, monotonic CAS and lineage.
use phenix_advanced_contracts::{CompactionCheckpoint, CompactionProblem, JournalSpan};
use phenix_plugin_compaction_store::{append_checkpoint, validate_ledger, LedgerProblem};

fn span(first: u64, last: u64) -> JournalSpan {
    JournalSpan {
        first_sequence: first,
        last_sequence: last,
        digest_sha256: "a".repeat(64),
    }
}

fn checkpoint(id: &str, revision: u64, parent: Option<&str>) -> CompactionCheckpoint {
    CompactionCheckpoint {
        id: id.into(),
        session: "s1".into(),
        revision,
        parent_checkpoint: parent.map(str::to_owned),
        input: span(1, 3),
        summarized: vec![span(1, 2)],
        retained: vec![span(3, 3)],
        excluded: vec![],
        summary: "Prior complete turns".into(),
        policy: "budget-128".into(),
        policy_version: 1,
        model: None,
        hydration_sources: vec![],
    }
}

#[test]
fn revision_zero_creates_one_snapshot_and_second_revision_preserves_first() {
    let first = checkpoint("a", 1, None);
    let ledger = append_checkpoint(&[], 0, &first).unwrap();
    assert_eq!(ledger, vec![first.clone()]);
    let second = checkpoint("b", 2, Some("a"));
    let updated = append_checkpoint(&ledger, 1, &second).unwrap();
    assert_eq!(updated, vec![first, second]);
    assert_eq!(validate_ledger(&updated), Ok(()));
}

#[test]
fn stale_revision_cannot_append_or_rewrite_history() {
    let first = checkpoint("a", 1, None);
    let history = append_checkpoint(&[], 0, &first).unwrap();
    assert_eq!(
        append_checkpoint(&history, 0, &checkpoint("b", 2, Some("a"))),
        Err(LedgerProblem::Conflict),
    );
    assert_eq!(
        append_checkpoint(&history, 1, &checkpoint("b", 2, None)),
        Err(LedgerProblem::InvalidLineage),
    );
    assert_eq!(history, vec![first]);
}

#[test]
fn duplicate_identity_skipped_revision_and_wrong_parent_reject() {
    let old = checkpoint("a", 1, None);
    let history = vec![old.clone()];
    for rejected in [
        checkpoint("a", 2, Some("a")),
        checkpoint("b", 3, Some("a")),
        checkpoint("b", 2, Some("missing")),
    ] {
        assert_eq!(
            append_checkpoint(&history, 1, &rejected),
            Err(LedgerProblem::InvalidLineage),
        );
    }
}

#[test]
fn cross_session_or_malformed_checkpoint_reject_without_new_revision() {
    let old = checkpoint("a", 1, None);
    let history = vec![old];
    let mut cross = checkpoint("b", 2, Some("a"));
    cross.session = "s2".into();
    assert_eq!(
        append_checkpoint(&history, 1, &cross),
        Err(LedgerProblem::ScopeDenied),
    );
    let mut malformed = checkpoint("b", 2, Some("a"));
    malformed.retained[0].last_sequence = 4;
    assert_eq!(
        append_checkpoint(&history, 1, &malformed),
        Err(LedgerProblem::InvalidCheckpoint(CompactionProblem::OutsideInput)),
    );
}

#[test]
fn ledger_detects_corrupted_or_truncated_revisions_after_restart() {
    let first = checkpoint("a", 1, None);
    let second = checkpoint("b", 2, Some("a"));
    assert_eq!(validate_ledger(&[second.clone()]), Err(LedgerProblem::InvalidLineage));
    let mut broken = second;
    broken.parent_checkpoint = None;
    assert_eq!(validate_ledger(&[first, broken]), Err(LedgerProblem::InvalidLineage));
}
