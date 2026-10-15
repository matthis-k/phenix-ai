//! RED P2-3c: append-only checkpoint metadata with one Storage CAS per
//! successful revision. Journal source bytes are never replicated here.
use phenix_advanced_contracts::{CheckpointStoreProblem, CompactionCheckpoint, JournalSpan};
use phenix_plugin_compaction_checkpoints::{append_checkpoint, find_checkpoint, validate_ledger};

fn checkpoint(id: &str, revision: u64, parent: Option<&str>) -> CompactionCheckpoint {
    let span = JournalSpan {
        first_sequence: 1,
        last_sequence: 2,
        digest_sha256: "a".repeat(64),
    };
    CompactionCheckpoint {
        id: id.into(),
        session: "s1".into(),
        revision,
        parent_checkpoint: parent.map(str::to_owned),
        input: span.clone(),
        summarized: vec![],
        retained: vec![span],
        excluded: vec![],
        summary: String::new(),
        policy: "test-policy".into(),
        policy_version: 1,
        model: None,
        hydration_sources: vec![],
    }
}

#[test]
fn linear_revisioned_lineage_is_restart_readable() {
    let first = checkpoint("cp1", 1, None);
    let one = append_checkpoint(&[], &first, "s1").unwrap();
    let second = checkpoint("cp2", 2, Some("cp1"));
    let two = append_checkpoint(&one, &second, "s1").unwrap();
    assert_eq!(two.len(), 2);
    assert_eq!(validate_ledger(&two, "s1"), Ok(()));
    assert_eq!(find_checkpoint(&two, "cp1").unwrap(), &first);
    assert_eq!(find_checkpoint(&two, "cp2").unwrap(), &second);
}

#[test]
fn stale_cas_duplicate_ids_and_forked_parent_reject() {
    let one = append_checkpoint(&[], &checkpoint("cp1", 1, None), "s1").unwrap();
    assert_eq!(
        append_checkpoint(&one, &checkpoint("cp2", 1, Some("cp1")), "s1"),
        Err(CheckpointStoreProblem::Conflict),
    );
    assert_eq!(
        append_checkpoint(&one, &checkpoint("cp1", 2, Some("cp1")), "s1"),
        Err(CheckpointStoreProblem::Conflict),
    );
    assert_eq!(
        append_checkpoint(&one, &checkpoint("cp2", 2, None), "s1"),
        Err(CheckpointStoreProblem::Conflict),
    );
    assert_eq!(
        append_checkpoint(&one, &checkpoint("cp2", 2, Some("unknown")), "s1"),
        Err(CheckpointStoreProblem::Conflict),
    );
}

#[test]
fn corrupt_history_rejects_without_silent_repair() {
    let one = append_checkpoint(&[], &checkpoint("cp1", 1, None), "s1").unwrap();
    let mut bad = one.clone();
    bad[0].input.digest_sha256 = "corrupt".into();
    assert_eq!(
        validate_ledger(&bad, "s1"),
        Err(CheckpointStoreProblem::StorageFailed)
    );
    assert_eq!(
        append_checkpoint(&bad, &checkpoint("cp2", 2, Some("cp1")), "s1"),
        Err(CheckpointStoreProblem::StorageFailed),
    );
    let mut bad = one.clone();
    bad[0].revision = 10;
    assert_eq!(
        validate_ledger(&bad, "s1"),
        Err(CheckpointStoreProblem::StorageFailed)
    );
}

#[test]
fn cross_session_read_and_append_reject() {
    let one = append_checkpoint(&[], &checkpoint("cp1", 1, None), "s1").unwrap();
    assert_eq!(
        validate_ledger(&one, "s2"),
        Err(CheckpointStoreProblem::StorageFailed)
    );
    assert_eq!(
        append_checkpoint(&one, &checkpoint("cp2", 2, Some("cp1")), "s2"),
        Err(CheckpointStoreProblem::StorageFailed),
    );
    assert_eq!(
        append_checkpoint(&[], &checkpoint("cp1", 1, None), "s2"),
        Err(CheckpointStoreProblem::ScopeDenied),
    );
    assert_eq!(
        find_checkpoint(&one, "missing"),
        Err(CheckpointStoreProblem::NotFound)
    );
}

#[test]
fn metadata_only_does_not_contain_original_event_bytes() {
    let original = b"distinct-uncompacted-tool-result-payload";
    let cp = checkpoint("cp1", 1, None);
    let ledger = append_checkpoint(&[], &cp, "s1").unwrap();
    let encoded = phenix_basic_contracts::encode(&ledger).unwrap();
    assert!(!encoded
        .0
        .windows(original.len())
        .any(|data| data == original));
}
