//! P2-3 checkpoint metadata. Checkpoints select prompt views; they do not
//! replace the Basic journal or grant access to its stored event bytes.
use crate::{valid_digest, valid_identity, validate_reference, EvidenceProblem, EvidenceScopeKind, SourceRef};
use phenix_sdk::Portable;
use serde::{Deserialize, Serialize};

/// Inclusive, non-empty journal sequence range. The digest binds the range to
/// canonical event bytes, which the selected journal provider must recheck.
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct JournalSpan {
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub digest_sha256: String,
}

/// A derived view of one contiguous input range. Summarized, retained, and
/// excluded ranges must partition every input event without duplication.
/// Reversibility depends on the original journal remaining available.
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct CompactionCheckpoint {
    pub id: String,
    pub session: String,
    pub revision: u64,
    pub parent_checkpoint: Option<String>,
    pub input: JournalSpan,
    pub summarized: Vec<JournalSpan>,
    pub retained: Vec<JournalSpan>,
    pub excluded: Vec<JournalSpan>,
    pub summary: String,
    pub policy: String,
    pub policy_version: u64,
    pub model: Option<String>,
    pub hydration_sources: Vec<SourceRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum CompactionProblem {
    InvalidIdentity,
    InvalidLineage,
    InvalidRange,
    InvalidDigest,
    MissingCitation,
    IncompleteCoverage,
    OverlappingRanges,
    OutsideInput,
    ScopeDenied,
}

fn validate_span(span: &JournalSpan) -> Result<(), CompactionProblem> {
    if span.first_sequence == 0 || span.first_sequence > span.last_sequence {
        return Err(CompactionProblem::InvalidRange);
    }
    if !valid_digest(&span.digest_sha256) {
        return Err(CompactionProblem::InvalidDigest);
    }
    Ok(())
}

/// Structural validation only. The storage and source providers must verify
/// event existence, canonical bytes, revision, authority and deletion status.
pub fn validate_checkpoint(checkpoint: &CompactionCheckpoint) -> Result<(), CompactionProblem> {
    if !valid_identity(&checkpoint.id)
        || !valid_identity(&checkpoint.session)
        || !valid_identity(&checkpoint.policy)
        || checkpoint.model.as_ref().is_some_and(|model| !valid_identity(model))
    {
        return Err(CompactionProblem::InvalidIdentity);
    }
    if checkpoint.revision == 0
        || checkpoint.policy_version == 0
        || checkpoint.parent_checkpoint.as_ref().is_some_and(|parent| {
            !valid_identity(parent) || parent == &checkpoint.id
        })
    {
        return Err(CompactionProblem::InvalidLineage);
    }
    if checkpoint.summary.len() > 65_536 {
        return Err(CompactionProblem::InvalidRange);
    }
    if checkpoint.summary.trim().is_empty() != checkpoint.summarized.is_empty() {
        // A summary requires at least one referenced journal span. A set of
        // summarized spans requires an actual summary in the selected view.
        return Err(CompactionProblem::MissingCitation);
    }
    validate_span(&checkpoint.input)?;
    let partitions = [
        checkpoint.summarized.as_slice(),
        checkpoint.retained.as_slice(),
        checkpoint.excluded.as_slice(),
    ];
    let count = partitions.iter().map(|items| items.len()).sum::<usize>();
    if count == 0 || count > 1024 || checkpoint.hydration_sources.len() > 256 {
        return Err(CompactionProblem::InvalidRange);
    }
    let mut coverage = Vec::with_capacity(count);
    for span in partitions.into_iter().flatten() {
        validate_span(span)?;
        if span.first_sequence < checkpoint.input.first_sequence
            || span.last_sequence > checkpoint.input.last_sequence
        {
            return Err(CompactionProblem::OutsideInput);
        }
        coverage.push((span.first_sequence, span.last_sequence));
    }
    coverage.sort_unstable();
    let mut next = u128::from(checkpoint.input.first_sequence);
    for (first, last) in coverage {
        if u128::from(first) < next {
            return Err(CompactionProblem::OverlappingRanges);
        }
        if u128::from(first) > next {
            return Err(CompactionProblem::IncompleteCoverage);
        }
        next = u128::from(last) + 1;
    }
    if next != u128::from(checkpoint.input.last_sequence) + 1 {
        return Err(CompactionProblem::IncompleteCoverage);
    }
    for source in &checkpoint.hydration_sources {
        if source.scope.kind != EvidenceScopeKind::Session || source.scope.id != checkpoint.session {
            return Err(CompactionProblem::ScopeDenied);
        }
        validate_reference(source).map_err(|error| match error {
            EvidenceProblem::InvalidDigest => CompactionProblem::InvalidDigest,
            EvidenceProblem::InvalidRange => CompactionProblem::InvalidRange,
            _ => CompactionProblem::InvalidIdentity,
        })?;
    }
    Ok(())
}
