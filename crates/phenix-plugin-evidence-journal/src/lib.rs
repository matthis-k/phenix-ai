//! Verify immutable journal events as P2 source evidence.
//! The provider does not give read authority merely because a caller knows
//! a reference. The selected Journal and the kernel authorize actual reads.
use phenix_advanced_contracts::{
    compare_source, validate_reference, EvidenceError, EvidenceProblem, EvidenceScope,
    EvidenceScopeKind, SourceMatch, SourceObservation, SourceRef,
};
use phenix_basic_contracts::{encode, Event};
use sha2::{Digest, Sha256};

const JOURNAL_OWNER: &str = "phenix.basic.event-journal";
const MAX_EVENT_BYTES: usize = 1024 * 1024;

fn problem(problem: EvidenceProblem, message: &str) -> EvidenceError {
    EvidenceError {
        problem,
        message: message.into(),
    }
}

fn validate_event(event: &Event) -> Result<(), EvidenceError> {
    if event.sequence == 0 || !phenix_sdk::identifier(&event.session) || event.run.is_empty() {
        return Err(problem(
            EvidenceProblem::InvalidIdentity,
            "invalid journal event identity",
        ));
    }
    Ok(())
}

pub fn journal_reference(event: &Event) -> Result<SourceRef, EvidenceError> {
    validate_event(event)?;
    // Reject clearly oversized payloads before attempting the bounded wire
    // encoder, so size failures are distinguishable from malformed evidence.
    if event.text.len() > MAX_EVENT_BYTES || event.data.0.len() > MAX_EVENT_BYTES {
        return Err(problem(
            EvidenceProblem::InvalidRange,
            "journal event too large",
        ));
    }
    let raw = encode(event).map_err(|_| {
        problem(
            EvidenceProblem::InvalidIdentity,
            "cannot encode source event",
        )
    })?;
    if raw.0.len() > MAX_EVENT_BYTES {
        return Err(problem(
            EvidenceProblem::InvalidRange,
            "journal event too large",
        ));
    }
    let digest = Sha256::digest(&raw.0);
    Ok(SourceRef {
        owner: JOURNAL_OWNER.into(),
        resource: format!("phenix.session.{}", event.session),
        revision: event.sequence.to_string(),
        digest_sha256: format!("{digest:x}"),
        range_start: 0,
        range_end: 0,
        scope: EvidenceScope {
            kind: EvidenceScopeKind::Session,
            id: event.session.clone(),
        },
    })
}

pub fn validate_journal_reference(reference: &SourceRef) -> Result<u64, EvidenceError> {
    validate_reference(reference).map_err(|p| problem(p, "malformed source reference"))?;
    let revision = reference.revision.parse::<u64>().map_err(|_| {
        problem(
            EvidenceProblem::InvalidIdentity,
            "non-numeric journal revision",
        )
    })?;
    if revision == 0
        || reference.revision != revision.to_string()
        || reference.owner != JOURNAL_OWNER
        || reference.scope.kind != EvidenceScopeKind::Session
        || !phenix_sdk::identifier(&reference.scope.id)
        || reference.resource != format!("phenix.session.{}", reference.scope.id)
        || reference.range_start != 0
        || reference.range_end != 0
    {
        return Err(problem(
            EvidenceProblem::InvalidIdentity,
            "invalid journal source identity",
        ));
    }
    Ok(revision)
}

/// Returns the actual observed revision and hash; comparison stays explicit.
/// A journal record is never mutated by this verification.
pub fn journal_observation(
    reference: &SourceRef,
    event: &Event,
) -> Result<SourceObservation, EvidenceError> {
    let revision = validate_journal_reference(reference)?;
    validate_event(event)?;
    if event.sequence != revision || event.session != reference.scope.id {
        return Err(problem(
            EvidenceProblem::InvalidIdentity,
            "source event identity mismatch",
        ));
    }
    let actual = journal_reference(event)?;
    let observation = SourceObservation {
        owner: actual.owner,
        resource: actual.resource,
        revision: actual.revision,
        digest_sha256: actual.digest_sha256,
    };
    debug_assert!(matches!(
        compare_source(reference, &observation),
        SourceMatch::Current | SourceMatch::Changed
    ));
    Ok(observation)
}

#[phenix_sdk::plugin(
    id = "phenix.advanced.evidence-journal",
    version = "1.0.0",
    // The journal delegates read to Storage. Ancestor ceilings must admit it.
    // A definition adds no import binding or effective grant.
    define(phenix_basic_contracts::Storage),
    provide(phenix_advanced_contracts::SourceEvidence, slot = "Evidence"),
    require(phenix_basic_contracts::EventJournal, slot = "Journal")
)]
mod plugin {
    use super::{journal_observation, problem, validate_journal_reference};
    use phenix_advanced_contracts::{
        EvidenceError, EvidenceProblem, SourceEvidence, SourceObservation, SourceRef,
    };
    use phenix_basic_contracts::{EventJournal, JournalQuery};
    use phenix_sdk::{CallError, CallResult, Host};

    #[provide(SourceEvidence::observe)]
    fn observe(
        host: &Host<'_>,
        reference: SourceRef,
    ) -> CallResult<SourceObservation, EvidenceError> {
        let sequence = validate_journal_reference(&reference).map_err(CallError::Domain)?;
        phenix_basic_contracts::check_session(host, &reference.scope.id).map_err(|error| {
            match error {
                CallError::Domain(_) => CallError::Domain(problem(
                    EvidenceProblem::InvalidIdentity,
                    "source session is not authorized",
                )),
                CallError::Kernel(failure) => CallError::Kernel(failure),
            }
        })?;
        let query = JournalQuery {
            session: reference.scope.id.clone(),
            after: sequence - 1,
            limit: 1,
        };
        let page = host
            .invoke::<EventJournal::read>("Journal", &query)
            .map_err(|error| match error {
                CallError::Domain(_) => CallError::Domain(problem(
                    EvidenceProblem::MissingSource,
                    "journal source unavailable",
                )),
                CallError::Kernel(failure) => CallError::Kernel(failure),
            })?;
        let event = page.events.first().ok_or_else(|| {
            CallError::Domain(problem(
                EvidenceProblem::MissingSource,
                "journal source was not found",
            ))
        })?;
        journal_observation(&reference, event).map_err(CallError::Domain)
    }
}
