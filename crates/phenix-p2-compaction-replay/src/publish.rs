//! P2-3y: checkpoint publication through the existing selected CAS store.
//!
//! The graph owns execution, grants, generation identity and cancellation.
//! The selected CompactionStore owns revision CAS and source digest checks.
//! This helper verifies selected-store acknowledgements; it never accesses
//! Storage or the EventJournal independently, nor calls a Model or a tool.
use phenix_advanced_contracts::{validate_checkpoint, CompactionCheckpoint, CompactionProblem};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedCheckpoint {
    pub checkpoint: CompactionCheckpoint,
    /// True only after a new CAS write and an exact selected-store readback.
    pub created: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckpointPublishProblem<E> {
    InvalidCandidate(CompactionProblem),
    Store(E),
    StaleHead,
    StoreEchoMismatch,
    MissingReadback,
    ReadbackMismatch,
}

/// Publish one already source-verified candidate with the selected store.
///
/// The latest and put callbacks must be session-scoped imports to the same
/// authorized CompactionStore provider. Its latest revalidates checkpoint
/// ancestry and source bytes; put performs revision CAS and integrity checks.
/// The caller must generate the candidate through the selected summarizer
/// and a source-verified plan, not from arbitrary text.
///
/// On an ambiguous write error (including cancellation after durable CAS),
/// return the error and do not retry inside the same Model call. A newly
/// authorized graph root may recover an exact persisted candidate through
/// the selected latest operation without issuing another write.
/// Different current heads always fail closed; replan on fresh source data.
pub fn publish_verified_checkpoint<E>(
    candidate: &CompactionCheckpoint,
    mut latest: impl FnMut() -> Result<Option<CompactionCheckpoint>, E>,
    mut put: impl FnMut(&CompactionCheckpoint) -> Result<CompactionCheckpoint, E>,
) -> Result<PublishedCheckpoint, CheckpointPublishProblem<E>> {
    validate_checkpoint(candidate).map_err(CheckpointPublishProblem::InvalidCandidate)?;
    let current = latest().map_err(CheckpointPublishProblem::Store)?;
    if current.as_ref() == Some(candidate) {
        return Ok(PublishedCheckpoint {
            checkpoint: candidate.clone(),
            created: false,
        });
    }
    match current.as_ref() {
        Some(current)
            if current.session == candidate.session
                && candidate.parent_checkpoint.as_deref() == Some(current.id.as_str())
                && current.revision.checked_add(1) == Some(candidate.revision) => {}
        None if candidate.revision == 1 && candidate.parent_checkpoint.is_none() => {}
        _ => return Err(CheckpointPublishProblem::StaleHead),
    }
    let stored = put(candidate).map_err(CheckpointPublishProblem::Store)?;
    if &stored != candidate {
        return Err(CheckpointPublishProblem::StoreEchoMismatch);
    }
    // An acknowledged write without a source-authenticated readback cannot
    // authorize compacted Model input. Never substitute the caller's bytes.
    let reread = latest().map_err(CheckpointPublishProblem::Store)?;
    match reread {
        None => Err(CheckpointPublishProblem::MissingReadback),
        Some(checkpoint) if &checkpoint != candidate => {
            Err(CheckpointPublishProblem::ReadbackMismatch)
        }
        Some(checkpoint) => Ok(PublishedCheckpoint {
            checkpoint,
            created: true,
        }),
    }
}
