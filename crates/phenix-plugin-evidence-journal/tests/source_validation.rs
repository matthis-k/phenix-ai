//! Tests-first acceptance for source evidence rooted in the Basic journal.
use phenix_advanced_contracts::{
    EvidenceProblem, EvidenceScope, EvidenceScopeKind, SourceMatch, SourceRef, compare_source,
};
use phenix_basic_contracts::{Event, EventKind};
use phenix_plugin_evidence_journal::{
    journal_observation, journal_reference, validate_journal_reference,
};

fn event() -> Event {
    Event {
        sequence: 9,
        session: "s1".into(),
        run: "run-1".into(),
        kind: EventKind::Answer,
        call_id: String::new(),
        text: "important detail".into(),
        data: phenix_sdk::Blob(vec![]),
    }
}
fn reference() -> SourceRef {
    journal_reference(&event()).unwrap()
}

#[test]
fn immutable_journal_event_is_exactly_referenced() {
    let value = event();
    let source = journal_reference(&value).unwrap();
    assert_eq!(source.owner, "phenix.basic.event-journal");
    assert_eq!(source.resource, "phenix.session.s1");
    assert_eq!(source.scope.kind, EvidenceScopeKind::Session);
    assert_eq!(source.scope.id, "s1");
    assert_eq!(source.revision, "9");
    assert_eq!(source.range_start, 0);
    assert_eq!(source.range_end, 0);
    assert_eq!(source.digest_sha256.len(), 64);
    assert_eq!(journal_observation(&source, &value).unwrap().revision, "9");
    assert_eq!(
        compare_source(&source, &journal_observation(&source, &value).unwrap()),
        SourceMatch::Current
    );
}

#[test]
fn changed_event_bytes_cannot_forge_the_original_source() {
    let source = reference();
    let mut modified = event();
    modified.text = "fabricated detail".into();
    let observed = journal_observation(&source, &modified).unwrap();
    assert_ne!(observed.digest_sha256, source.digest_sha256);
    assert_eq!(compare_source(&source, &observed), SourceMatch::Changed);
}

#[test]
fn mismatched_journal_identity_is_denied_not_silently_rewritten() {
    let mut changed = reference();
    changed.scope = EvidenceScope {
        kind: EvidenceScopeKind::Session,
        id: "another-session".into(),
    };
    assert_eq!(
        validate_journal_reference(&changed).unwrap_err().problem,
        EvidenceProblem::InvalidIdentity
    );
    changed = reference();
    changed.resource = "phenix.session.s2".into();
    assert_eq!(
        validate_journal_reference(&changed).unwrap_err().problem,
        EvidenceProblem::InvalidIdentity
    );
    changed = reference();
    changed.owner = "untrusted.import".into();
    assert_eq!(
        validate_journal_reference(&changed).unwrap_err().problem,
        EvidenceProblem::InvalidIdentity
    );
}

#[test]
fn source_revision_is_not_a_guessable_non_numeric_identifier() {
    let mut changed = reference();
    changed.revision = "not-an-event".into();
    assert_eq!(
        validate_journal_reference(&changed).unwrap_err().problem,
        EvidenceProblem::InvalidIdentity
    );
    changed = reference();
    changed.revision = "0".into();
    assert_eq!(
        validate_journal_reference(&changed).unwrap_err().problem,
        EvidenceProblem::InvalidIdentity
    );
}

#[test]
fn replay_with_wrong_sequence_or_session_cannot_be_used_as_evidence() {
    let source = reference();
    let mut wrong = event();
    wrong.sequence = 10;
    assert_eq!(
        journal_observation(&source, &wrong).unwrap_err().problem,
        EvidenceProblem::InvalidIdentity
    );
    wrong = event();
    wrong.session = "s2".into();
    assert_eq!(
        journal_observation(&source, &wrong).unwrap_err().problem,
        EvidenceProblem::InvalidIdentity
    );
}

#[test]
fn invalid_or_unbounded_journal_events_fail_without_effects() {
    let mut bad = event();
    bad.sequence = 0;
    assert_eq!(journal_reference(&bad).unwrap_err().problem, EvidenceProblem::InvalidIdentity);
    bad = event();
    bad.session = "../escape".into();
    assert_eq!(journal_reference(&bad).unwrap_err().problem, EvidenceProblem::InvalidIdentity);
    bad = event();
    bad.text = "x".repeat(1_048_577);
    assert_eq!(journal_reference(&bad).unwrap_err().problem, EvidenceProblem::InvalidRange);
}
