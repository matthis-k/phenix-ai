//! RED P2-3c: append-only checkpoint metadata with one Storage CAS per
//! successful revision. Journal source bytes are never replicated here.
use phenix_advanced_contracts::{CheckpointStoreProblem, CompactionCheckpoint, JournalSpan};
use phenix_plugin_compaction_checkpoints::{
    append_checkpoint, find_checkpoint, newest_checkpoint, validate_ledger,
    verify_checkpoint_ancestry, AncestryVerificationError,
};

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

#[test]
fn latest_checkpoint_is_monotone_and_session_scoped() {
    assert_eq!(
        newest_checkpoint(&[], "s1"),
        Err(CheckpointStoreProblem::NotFound),
    );
    let one = append_checkpoint(&[], &checkpoint("cp1", 1, None), "s1").unwrap();
    assert_eq!(newest_checkpoint(&one, "s1").unwrap().id, "cp1");
    let two = append_checkpoint(&one, &checkpoint("cp2", 2, Some("cp1")), "s1").unwrap();
    assert_eq!(newest_checkpoint(&two, "s1").unwrap().id, "cp2");
    assert_eq!(newest_checkpoint(&two, "s1").unwrap().revision, 2);
    assert_eq!(
        newest_checkpoint(&two, "s2"),
        Err(CheckpointStoreProblem::StorageFailed),
    );
    let mut corrupt = two;
    corrupt[1].parent_checkpoint = Some("forked".into());
    assert_eq!(
        newest_checkpoint(&corrupt, "s1"),
        Err(CheckpointStoreProblem::StorageFailed),
    );
}

#[test]
fn checkpoint_ancestry_verifies_every_source_in_parent_order() {
    let one = append_checkpoint(&[], &checkpoint("cp1", 1, None), "s1").unwrap();
    let two = append_checkpoint(&one, &checkpoint("cp2", 2, Some("cp1")), "s1").unwrap();
    let mut visited = Vec::new();
    assert_eq!(
        verify_checkpoint_ancestry(&two, "cp2", |cp| {
            visited.push(cp.id.clone());
            Ok::<(), &'static str>(())
        }),
        Ok(()),
    );
    assert_eq!(visited, ["cp1", "cp2"]);

    let mut visited = Vec::new();
    assert_eq!(
        verify_checkpoint_ancestry(&two, "cp1", |cp| {
            visited.push(cp.id.clone());
            Ok::<(), &'static str>(())
        }),
        Ok(()),
    );
    assert_eq!(visited, ["cp1"]);
}

#[test]
fn stale_ancestor_denies_descendant_instead_of_using_latest_source_only() {
    let one = append_checkpoint(&[], &checkpoint("cp1", 1, None), "s1").unwrap();
    let two = append_checkpoint(&one, &checkpoint("cp2", 2, Some("cp1")), "s1").unwrap();
    let mut visited = Vec::new();
    assert_eq!(
        verify_checkpoint_ancestry(&two, "cp2", |cp| {
            visited.push(cp.id.clone());
            if cp.id == "cp1" {
                Err("ancestor source erased")
            } else {
                Ok(())
            }
        }),
        Err(AncestryVerificationError::Source("ancestor source erased")),
    );
    assert_eq!(visited, ["cp1"]);
    assert_eq!(
        verify_checkpoint_ancestry(&two, "missing", |_| Ok::<(), &'static str>(())),
        Err(AncestryVerificationError::MissingCheckpoint),
    );
}
