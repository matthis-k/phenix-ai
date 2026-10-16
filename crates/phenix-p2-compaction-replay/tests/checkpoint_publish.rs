//! P2-3y RED: publish only source-backed CAS checkpoints, without a second
//! scheduler. The caller's selected CompactionStore authenticates source bytes.
use phenix_advanced_contracts::{CompactionCheckpoint, JournalSpan};
use phenix_p2_compaction_replay::{publish_verified_checkpoint, CheckpointPublishProblem};
use std::cell::{Cell, RefCell};

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
        input: span(1, 4),
        summarized: vec![span(1, 2)],
        retained: vec![span(3, 4)],
        excluded: vec![],
        summary: "Exact earlier source retained in the journal.".into(),
        policy: "p2-verified-completed-turns".into(),
        policy_version: 1,
        model: None,
        hydration_sources: vec![],
    }
}

#[test]
fn first_publication_uses_existing_cas_and_verifies_the_selected_store_readback() {
    let candidate = checkpoint("cp1", 1, None);
    let latest = RefCell::new(None::<CompactionCheckpoint>);
    let writes = Cell::new(0);
    let reads = Cell::new(0);
    let accepted = publish_verified_checkpoint(
        &candidate,
        || {
            reads.set(reads.get() + 1);
            Ok::<_, &'static str>(latest.borrow().clone())
        },
        |next| {
            writes.set(writes.get() + 1);
            *latest.borrow_mut() = Some(next.clone());
            Ok::<_, &'static str>(next.clone())
        },
    )
    .unwrap();
    assert!(accepted.created);
    assert_eq!(accepted.checkpoint, candidate);
    assert_eq!(writes.get(), 1);
    assert_eq!(reads.get(), 2);
}

#[test]
fn exact_restart_retry_reuses_durable_checkpoint_without_another_write() {
    let candidate = checkpoint("cp1", 1, None);
    let latest = RefCell::new(None::<CompactionCheckpoint>);
    let writes = Cell::new(0);
    let first = publish_verified_checkpoint(
        &candidate,
        || Ok::<_, &'static str>(latest.borrow().clone()),
        |next| {
            *latest.borrow_mut() = Some(next.clone());
            writes.set(writes.get() + 1);
            Err::<CompactionCheckpoint, _>("lost CAS acknowledgement")
        },
    );
    assert_eq!(
        first,
        Err(CheckpointPublishProblem::Store("lost CAS acknowledgement"))
    );
    let restarted = publish_verified_checkpoint(
        &candidate,
        || Ok::<_, &'static str>(latest.borrow().clone()),
        |_next| {
            writes.set(writes.get() + 1);
            Err::<CompactionCheckpoint, _>("unexpected duplicate commit")
        },
    )
    .unwrap();
    assert!(!restarted.created);
    assert_eq!(restarted.checkpoint, candidate);
    assert_eq!(writes.get(), 1);
}

#[test]
fn competing_revision_or_parent_is_rejected_before_any_storage_write() {
    let old = checkpoint("cp1", 1, None);
    let candidate = checkpoint("cp2", 2, Some("cp1"));
    let writes = Cell::new(0);
    let new_writer = checkpoint("other", 2, Some("cp1"));
    let conflict = publish_verified_checkpoint(
        &candidate,
        || Ok::<_, &'static str>(Some(new_writer.clone())),
        |_next| {
            writes.set(writes.get() + 1);
            Ok::<_, &'static str>(candidate.clone())
        },
    );
    assert_eq!(conflict, Err(CheckpointPublishProblem::StaleHead));
    assert_eq!(writes.get(), 0);
    let accepted = publish_verified_checkpoint(
        &candidate,
        || Ok::<_, &'static str>(Some(old.clone())),
        |next| Ok::<_, &'static str>(next.clone()),
    );
    // No durable readback means Model admission must remain blocked.
    assert_eq!(accepted, Err(CheckpointPublishProblem::MissingReadback));
}

#[test]
fn counterfeit_store_acknowledgement_or_changed_readback_denies_model_admission() {
    let candidate = checkpoint("cp1", 1, None);
    let forged = checkpoint("forged", 1, None);
    let echo = publish_verified_checkpoint(
        &candidate,
        || Ok::<_, &'static str>(None),
        |_next| Ok::<_, &'static str>(forged.clone()),
    );
    assert_eq!(echo, Err(CheckpointPublishProblem::StoreEchoMismatch));

    let reads = Cell::new(0);
    let mismatch = publish_verified_checkpoint(
        &candidate,
        || {
            reads.set(reads.get() + 1);
            Ok::<_, &'static str>(if reads.get() == 1 {
                None
            } else {
                Some(forged.clone())
            })
        },
        |next| Ok::<_, &'static str>(next.clone()),
    );
    assert_eq!(mismatch, Err(CheckpointPublishProblem::ReadbackMismatch));
}

#[test]
fn invalid_candidate_and_failed_store_reads_fail_closed_without_writes() {
    let mut candidate = checkpoint("cp1", 1, None);
    candidate.summary.clear();
    let reads = Cell::new(0);
    let writes = Cell::new(0);
    let invalid = publish_verified_checkpoint(
        &candidate,
        || {
            reads.set(reads.get() + 1);
            Ok::<_, &'static str>(None)
        },
        |next| {
            writes.set(writes.get() + 1);
            Ok::<_, &'static str>(next.clone())
        },
    );
    assert!(matches!(
        invalid,
        Err(CheckpointPublishProblem::InvalidCandidate(_))
    ));
    assert_eq!(reads.get(), 0);
    assert_eq!(writes.get(), 0);

    let valid = checkpoint("cp1", 1, None);
    let denied = publish_verified_checkpoint(
        &valid,
        || Err::<Option<CompactionCheckpoint>, _>("selected store unavailable"),
        |_next| {
            writes.set(writes.get() + 1);
            Ok::<_, &'static str>(valid.clone())
        },
    );
    assert_eq!(
        denied,
        Err(CheckpointPublishProblem::Store(
            "selected store unavailable"
        ))
    );
    assert_eq!(writes.get(), 0);
}
