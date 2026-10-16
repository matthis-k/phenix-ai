//! P2-3g: verify append-only Basic journal continuation after an immutable,
//! source-verified compaction checkpoint. No tool effect is ever replayed.
use crate::{compacted_view, span_for_events, ContextProjection, ViewProblem};
use phenix_advanced_contracts::{CompactionCheckpoint, JournalSpan};
use phenix_basic_contracts::{Event, EventKind};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtendedProjection {
    /// Historical events are represented by their verified checkpoint view.
    pub base: ContextProjection,
    /// Byte-exact newly read EventJournal records following the checkpoint.
    pub trailing: Vec<Event>,
    /// Source citation for the new, uncheckpointed, contiguous suffix.
    pub trailing_span: Option<JournalSpan>,
    /// Summary + visible base events + all trailing payload bytes. Not tokens.
    pub visible_payload_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TailProblem {
    Checkpoint(ViewProblem),
    InvalidBudget,
    OverBudget,
    CapacityExceeded,
    InvalidSequence,
    WrongSession,
    InvalidToolSequence,
    UnresolvedToolCall,
}

/// The caller must supply separately authorized, live EventJournal reads.
/// Checkpoint bytes are revalidated before any historical summary can enter a
/// prompt. The tail is contiguous after the checkpoint, unchanged and cited.
/// This helper does not claim model-message completeness or model token usage.
pub fn extend_verified_view(
    checkpoint: &CompactionCheckpoint,
    original_checkpoint_events: &[Event],
    appended_events: &[Event],
    max_visible_payload_bytes: u64,
) -> Result<ExtendedProjection, TailProblem> {
    if max_visible_payload_bytes == 0 {
        return Err(TailProblem::InvalidBudget);
    }
    if appended_events.len() > 8192 {
        return Err(TailProblem::CapacityExceeded);
    }
    let base = compacted_view(
        checkpoint,
        original_checkpoint_events,
        max_visible_payload_bytes,
    )
    .map_err(TailProblem::Checkpoint)?;
    let mut sequence = checkpoint.input.last_sequence;
    let mut active = BTreeSet::<&str>::new();
    let mut visible_payload_bytes = base.visible_payload_bytes;
    for event in appended_events {
        sequence = sequence
            .checked_add(1)
            .ok_or(TailProblem::InvalidSequence)?;
        if event.sequence != sequence {
            return Err(TailProblem::InvalidSequence);
        }
        if event.session != checkpoint.session {
            return Err(TailProblem::WrongSession);
        }
        match event.kind {
            EventKind::ToolStarted => {
                if event.call_id.is_empty() || !active.insert(event.call_id.as_str()) {
                    return Err(TailProblem::InvalidToolSequence);
                }
            }
            EventKind::ToolFinished if !active.remove(event.call_id.as_str()) => {
                return Err(TailProblem::InvalidToolSequence);
            }
            _ => {}
        }
        let size = event
            .text
            .len()
            .checked_add(event.data.0.len())
            .and_then(|bytes| bytes.checked_add(event.call_id.len()))
            .and_then(|bytes| u64::try_from(bytes).ok())
            .ok_or(TailProblem::OverBudget)?;
        visible_payload_bytes = visible_payload_bytes
            .checked_add(size)
            .ok_or(TailProblem::OverBudget)?;
        if visible_payload_bytes > max_visible_payload_bytes {
            return Err(TailProblem::OverBudget);
        }
    }
    if !active.is_empty() {
        return Err(TailProblem::UnresolvedToolCall);
    }
    let trailing_span = if appended_events.is_empty() {
        None
    } else {
        Some(span_for_events(appended_events).map_err(|_| TailProblem::InvalidSequence)?)
    };
    Ok(ExtendedProjection {
        base,
        trailing: appended_events.to_vec(),
        trailing_span,
        visible_payload_bytes,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TailAssemblyProblem {
    Tail(TailProblem),
    Replay(crate::ReplayProblem),
    Projection(ViewProblem),
    Transcript(crate::TranscriptProblem),
}

/// Assemble the exact ModelRequest using a durable checkpoint and a later,
/// independently authorized EventJournal suffix. The checkpoint is never
/// mutated or committed. An in-memory extension retains all suffix events,
/// then verifies the complete SessionSnapshot against every source event.
///
/// The caller must read the original and suffix through the selected Journal,
/// check checkpoint ancestry and source-erasure policy, and meter the exact
/// returned request with the selected Model before executing it. This pure
/// helper grants no authority and never retries tool effects.
pub fn assemble_verified_tail_request(
    authoritative: &phenix_basic_contracts::ModelRequest,
    checkpoint: &CompactionCheckpoint,
    original_checkpoint_events: &[Event],
    appended_events: &[Event],
    max_visible_payload_bytes: u64,
) -> Result<phenix_basic_contracts::ModelRequest, TailAssemblyProblem> {
    let extended = extend_verified_view(
        checkpoint,
        original_checkpoint_events,
        appended_events,
        max_visible_payload_bytes,
    )
    .map_err(TailAssemblyProblem::Tail)?;
    if appended_events.is_empty() {
        return crate::assemble_verified_basic_request(
            authoritative,
            original_checkpoint_events,
            checkpoint,
            &extended.base,
        )
        .map_err(TailAssemblyProblem::Transcript);
    }

    let mut source = Vec::with_capacity(original_checkpoint_events.len() + appended_events.len());
    source.extend_from_slice(original_checkpoint_events);
    source.extend_from_slice(appended_events);
    let mut effective = checkpoint.clone();
    effective.input = span_for_events(&source).map_err(TailAssemblyProblem::Replay)?;
    effective
        .retained
        .push(extended.trailing_span.ok_or(TailAssemblyProblem::Tail(
            TailProblem::InvalidSequence,
        ))?);
    let view = compacted_view(&effective, &source, max_visible_payload_bytes)
        .map_err(TailAssemblyProblem::Projection)?;
    crate::assemble_verified_basic_request(authoritative, &source, &effective, &view)
        .map_err(TailAssemblyProblem::Transcript)
}
