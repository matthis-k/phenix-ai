//! P2-3a RED acceptance for lossless, source-linked compaction checkpoints.
//! Checkpoints describe a derived prompt view. The Basic journal is authoritative.
use phenix_advanced_contracts::{
    decode, encode, validate_checkpoint, CompactionCheckpoint, CompactionProblem, EvidenceScope,
    EvidenceScopeKind, JournalSpan, SourceRef,
};

fn span(first: u64, last: u64) -> JournalSpan {
    JournalSpan {
        first_sequence: first,
        last_sequence: last,
        digest_sha256: "a".repeat(64),
    }
}

fn checkpoint() -> CompactionCheckpoint {
    CompactionCheckpoint {
        id: "checkpoint-2".into(),
        session: "s1".into(),
        revision: 2,
        parent_checkpoint: Some("checkpoint-1".into()),
        input: span(1, 12),
        summarized: vec![span(1, 8)],
        retained: vec![span(9, 11)],
        excluded: vec![span(12, 12)],
        summary: "Earlier events are retained by journal reference.".into(),
        policy: "budgeted-recent".into(),
        policy_version: 1,
        model: None,
        hydration_sources: vec![SourceRef {
            owner: "phenix.basic.event-journal".into(),
            resource: "phenix.session.s1".into(),
            revision: "5".into(),
            digest_sha256: "b".repeat(64),
            range_start: 0,
            range_end: 0,
            scope: EvidenceScope {
                kind: EvidenceScopeKind::Session,
                id: "s1".into(),
            },
        }],
    }
}

#[test]
fn checkpoint_has_a_portable_exact_event_partition() {
    let original = checkpoint();
    assert_eq!(validate_checkpoint(&original), Ok(()));
    assert_eq!(
        decode::<CompactionCheckpoint>(&encode(&original).unwrap()).unwrap(),
        original
    );
    assert_eq!(original.summarized[0].first_sequence, 1);
    assert_eq!(original.excluded[0].last_sequence, 12);
}

#[test]
fn missing_or_overlapping_journal_events_reject() {
    let mut candidate = checkpoint();
    candidate.retained[0].first_sequence = 10;
    assert_eq!(validate_checkpoint(&candidate), Err(CompactionProblem::IncompleteCoverage));
    candidate = checkpoint();
    candidate.retained[0].first_sequence = 8;
    assert_eq!(validate_checkpoint(&candidate), Err(CompactionProblem::OverlappingRanges));
    candidate = checkpoint();
    candidate.excluded[0].last_sequence = 13;
    assert_eq!(validate_checkpoint(&candidate), Err(CompactionProblem::OutsideInput));
}

#[test]
fn false_source_digests_and_foreign_hydration_reject() {
    let mut candidate = checkpoint();
    candidate.summarized[0].digest_sha256 = "not-sha256".into();
    assert_eq!(validate_checkpoint(&candidate), Err(CompactionProblem::InvalidDigest));
    candidate = checkpoint();
    candidate.hydration_sources[0].scope.id = "s2".into();
    assert_eq!(validate_checkpoint(&candidate), Err(CompactionProblem::ScopeDenied));
}

#[test]
fn summary_must_be_cited_and_checkpoint_lineage_stable() {
    let mut candidate = checkpoint();
    candidate.summarized.clear();
    candidate.retained = vec![span(1, 11)];
    assert_eq!(validate_checkpoint(&candidate), Err(CompactionProblem::MissingCitation));
    candidate = checkpoint();
    candidate.parent_checkpoint = Some(candidate.id.clone());
    assert_eq!(validate_checkpoint(&candidate), Err(CompactionProblem::InvalidLineage));
    candidate = checkpoint();
    candidate.revision = 0;
    assert_eq!(validate_checkpoint(&candidate), Err(CompactionProblem::InvalidLineage));
    candidate = checkpoint();
    candidate.summary.clear();
    assert_eq!(validate_checkpoint(&candidate), Err(CompactionProblem::MissingCitation));
}
