//! P2-1 semantic acceptance, written before the evidence contract implementation.
use phenix_advanced_contracts::{
    validate_record, validate_reference, EvidenceFreshness, EvidenceKind, EvidenceProblem,
    EvidenceRecord, EvidenceScope, EvidenceScopeKind, SourceMatch, SourceObservation, SourceRef,
    compare_source, decode, encode,
};

fn scope() -> EvidenceScope {
    EvidenceScope {
        kind: EvidenceScopeKind::Session,
        id: "session-1".into(),
    }
}
fn reference() -> SourceRef {
    SourceRef {
        owner: "phenix.basic.event-journal".into(),
        resource: "phenix.session.session-1".into(),
        revision: "42".into(),
        digest_sha256: "a".repeat(64),
        range_start: 4,
        range_end: 16,
        scope: scope(),
    }
}
fn record() -> EvidenceRecord {
    EvidenceRecord {
        id: "fact-1".into(),
        scope: scope(),
        kind: EvidenceKind::Fact,
        statement: "The color is violet".into(),
        sources: vec![reference()],
        revision: 1,
        observed_at: 100,
        valid_from: 80,
        valid_until: None,
        confidence_basis_points: 10_000,
        freshness: EvidenceFreshness::Current,
        supersedes: vec![],
    }
}

#[test]
fn portable_reference_round_trip_preserves_identity_and_authorization_scope() {
    let original = reference();
    validate_reference(&original).unwrap();
    assert_eq!(decode::<SourceRef>(&encode(&original).unwrap()).unwrap(), original);
    let original = record();
    validate_record(&original).unwrap();
    assert_eq!(decode::<EvidenceRecord>(&encode(&original).unwrap()).unwrap(), original);
}

#[test]
fn rejects_unverifiable_and_malformed_source_identity() {
    let mut candidate = reference();
    candidate.digest_sha256 = "not-a-source-hash".into();
    assert_eq!(validate_reference(&candidate), Err(EvidenceProblem::InvalidDigest));
    candidate = reference();
    candidate.digest_sha256 = "A".repeat(64);
    assert_eq!(validate_reference(&candidate), Err(EvidenceProblem::InvalidDigest));
    candidate = reference();
    candidate.range_start = 17;
    assert_eq!(validate_reference(&candidate), Err(EvidenceProblem::InvalidRange));
    candidate = reference();
    candidate.scope.id.clear();
    assert_eq!(validate_reference(&candidate), Err(EvidenceProblem::InvalidIdentity));
}

#[test]
fn rejects_unbacked_claims_bad_confidence_and_inconsistent_lineage() {
    let mut candidate = record();
    candidate.sources.clear();
    assert_eq!(validate_record(&candidate), Err(EvidenceProblem::MissingSource));
    candidate = record();
    candidate.confidence_basis_points = 10_001;
    assert_eq!(validate_record(&candidate), Err(EvidenceProblem::InvalidConfidence));
    candidate = record();
    candidate.valid_until = Some(80);
    assert_eq!(validate_record(&candidate), Err(EvidenceProblem::InvalidValidity));
    candidate = record();
    candidate.supersedes.push("fact-1".into());
    assert_eq!(validate_record(&candidate), Err(EvidenceProblem::SelfSupersession));
    candidate = record();
    candidate.sources.push(reference());
    assert_eq!(validate_record(&candidate), Err(EvidenceProblem::DuplicateSource));
}

#[test]
fn source_revision_mismatch_never_counts_as_current() {
    let old = reference();
    let mut actual = SourceObservation {
        owner: old.owner.clone(),
        resource: old.resource.clone(),
        revision: old.revision.clone(),
        digest_sha256: old.digest_sha256.clone(),
    };
    assert_eq!(compare_source(&old, &actual), SourceMatch::Current);
    actual.revision = "43".into();
    assert_eq!(compare_source(&old, &actual), SourceMatch::Changed);
    actual.revision = old.revision.clone();
    actual.digest_sha256 = "b".repeat(64);
    assert_eq!(compare_source(&old, &actual), SourceMatch::Changed);
    actual.owner = "other-owner".into();
    assert_eq!(compare_source(&old, &actual), SourceMatch::Unrelated);
}

#[test]
fn as_of_query_preserves_historical_fact_validity() {
    let mut earlier = record();
    earlier.valid_until = Some(120);
    earlier.freshness = EvidenceFreshness::Historical;
    assert!(earlier.valid_at(100));
    assert!(!earlier.valid_at(125));
    let mut corrected = record();
    corrected.id = "fact-2".into();
    corrected.valid_from = 120;
    corrected.observed_at = 121;
    corrected.supersedes = vec!["fact-1".into()];
    assert!(!corrected.valid_at(100));
    assert!(corrected.valid_at(125));
    validate_record(&corrected).unwrap();
}

#[test]
fn source_identity_does_not_itself_grant_read_access() {
    let source = reference();
    let foreign = EvidenceScope {
        kind: EvidenceScopeKind::Session,
        id: "session-2".into(),
    };
    assert!(!source.scope.authorizes(&foreign));
    assert!(source.scope.authorizes(&scope()));
}
