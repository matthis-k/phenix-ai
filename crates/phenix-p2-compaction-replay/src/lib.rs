//! Deterministic P2-3 original-event verification and expansion.
//!
//! This is a pure verifier, **not** a journal reader or authorization oracle.
//! A caller must fetch the events through its authorized EventJournal binding,
//! enforce source-erasure policy, and then verify the bytes with this module.
//! Summaries cannot replace missing events and no tool effect is executed.
use phenix_advanced_contracts::{validate_checkpoint, CompactionCheckpoint, JournalSpan};
use phenix_basic_contracts::Event;
use phenix_sdk::Portable;
use sha2::{Digest, Sha256};

pub mod history;
pub mod model;
pub mod tail;
pub mod view;
pub use history::{assemble_verified_basic_request, TranscriptProblem};
pub use model::{select_model_view, ModelSelectionError, SelectedModelView};
pub use tail::{extend_verified_view, ExtendedProjection, TailProblem};
pub use view::{compacted_view, ContextPart, ContextProjection, ViewProblem};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayProblem {
    InvalidCheckpoint,
    InvalidSequence,
    IncompleteJournal,
    WrongSession,
    DigestChanged,
    InvalidEncoding,
    CapacityExceeded,
}

const HASH_DOMAIN: &[u8] = b"phenix.compaction.journal-span.v1\0";
const MAX_EVENTS_PER_SPAN: usize = 65_536;
const MAX_ENCODED_EVENT_BYTES: usize = 8 * 1024 * 1024;

/// Hash canonical Portable event bytes with length framing. This avoids a
/// separate bespoke event serializer and binds tool result bytes and call IDs.
pub fn span_for_events(events: &[Event]) -> Result<JournalSpan, ReplayProblem> {
    if events.is_empty() {
        return Err(ReplayProblem::IncompleteJournal);
    }
    if events.len() > MAX_EVENTS_PER_SPAN {
        return Err(ReplayProblem::CapacityExceeded);
    }
    let session = &events[0].session;
    let first = events[0].sequence;
    if first == 0 {
        return Err(ReplayProblem::InvalidSequence);
    }
    let mut previous = first;
    let mut has_gap = false;
    let mut hasher = Sha256::new();
    hasher.update(HASH_DOMAIN);
    for (index, event) in events.iter().enumerate() {
        if &event.session != session {
            return Err(ReplayProblem::WrongSession);
        }
        if index != 0 {
            if event.sequence <= previous {
                return Err(ReplayProblem::InvalidSequence);
            }
            if previous.checked_add(1) != Some(event.sequence) {
                // Scan the complete ordered stream before reporting gaps:
                // an out-of-order event may follow the first apparent gap.
                has_gap = true;
            }
        }
        previous = event.sequence;
        let bytes = event
            .to_value()
            .encode()
            .map_err(|_| ReplayProblem::InvalidEncoding)?;
        if bytes.len() > MAX_ENCODED_EVENT_BYTES {
            return Err(ReplayProblem::CapacityExceeded);
        }
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(bytes);
    }
    if has_gap {
        return Err(ReplayProblem::IncompleteJournal);
    }
    Ok(JournalSpan {
        first_sequence: first,
        last_sequence: previous,
        digest_sha256: format!("{:x}", hasher.finalize()),
    })
}

/// Verify one complete, authorized journal range and its checkpoint partition.
/// Return the *original* events, including tool bytes, not reconstructed text.
/// A changed or intentionally deleted source fails closed.
pub fn restore_original_events(
    checkpoint: &CompactionCheckpoint,
    events: &[Event],
) -> Result<Vec<Event>, ReplayProblem> {
    validate_checkpoint(checkpoint).map_err(|_| ReplayProblem::InvalidCheckpoint)?;
    let entire = span_for_events(events)?;
    if events
        .iter()
        .any(|event| event.session != checkpoint.session)
    {
        return Err(ReplayProblem::WrongSession);
    }
    if entire.first_sequence != checkpoint.input.first_sequence
        || entire.last_sequence != checkpoint.input.last_sequence
    {
        return Err(ReplayProblem::IncompleteJournal);
    }
    if entire.digest_sha256 != checkpoint.input.digest_sha256 {
        return Err(ReplayProblem::DigestChanged);
    }
    for expected in checkpoint
        .summarized
        .iter()
        .chain(&checkpoint.retained)
        .chain(&checkpoint.excluded)
    {
        let first = expected.first_sequence - checkpoint.input.first_sequence;
        let end = expected.last_sequence - checkpoint.input.first_sequence + 1;
        let start = usize::try_from(first).map_err(|_| ReplayProblem::IncompleteJournal)?;
        let end = usize::try_from(end).map_err(|_| ReplayProblem::IncompleteJournal)?;
        let actual = events
            .get(start..end)
            .ok_or(ReplayProblem::IncompleteJournal)?;
        if span_for_events(actual)?.digest_sha256 != expected.digest_sha256 {
            return Err(ReplayProblem::DigestChanged);
        }
    }
    Ok(events.to_vec())
}
