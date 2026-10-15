//! Pure P2-3d context projection over *already authorized* journal events.
//! Neither compaction nor expansion executes tools or changes durable history.
use crate::{restore_original_events, ReplayProblem};
use phenix_advanced_contracts::{CompactionCheckpoint, JournalSpan};
use phenix_basic_contracts::{Event, EventKind};
use std::collections::BTreeMap;

/// A reference-bearing summary or byte-exact original journal event.
/// Consumers must explicitly translate this view to their Model protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextPart {
    Summary {
        checkpoint_id: String,
        text: String,
        sources: Vec<JournalSpan>,
    },
    Original(Event),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextProjection {
    pub checkpoint_id: String,
    pub source_span: JournalSpan,
    pub parts: Vec<ContextPart>,
    pub excluded: Vec<JournalSpan>,
    /// Sum of summary text and visible event text, call IDs and blob sizes.
    /// This is a *byte budget*, not an estimate of Model token usage.
    pub visible_payload_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewProblem {
    Replay(ReplayProblem),
    InvalidBudget,
    OverBudget,
    InvalidToolSequence,
    SplitToolCall,
    UnresolvedToolCall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Visibility {
    Summarized,
    Retained,
    Excluded,
}

fn visibility(checkpoint: &CompactionCheckpoint, sequence: u64) -> Visibility {
    if checkpoint
        .retained
        .iter()
        .any(|span| span.first_sequence <= sequence && sequence <= span.last_sequence)
    {
        Visibility::Retained
    } else if checkpoint
        .summarized
        .iter()
        .any(|span| span.first_sequence <= sequence && sequence <= span.last_sequence)
    {
        Visibility::Summarized
    } else {
        // Structural checkpoint validation already proved exact partition
        // coverage; remaining sequences must be explicitly excluded.
        Visibility::Excluded
    }
}

/// Project a compacted view from source-verified journal bytes. A pending tool,
/// repeated/orphaned tool completion or split tool pair fails closed. No effect
/// is executed and no summary can fill in missing original records.
pub fn compacted_view(
    checkpoint: &CompactionCheckpoint,
    journal: &[Event],
    max_visible_payload_bytes: u64,
) -> Result<ContextProjection, ViewProblem> {
    if max_visible_payload_bytes == 0 {
        return Err(ViewProblem::InvalidBudget);
    }
    // Source digest, partition digests, order and session are checked *before*
    // any potentially sensitive content enters the selected view.
    let original =
        restore_original_events(checkpoint, journal).map_err(ViewProblem::Replay)?;

    let mut active: BTreeMap<&str, Visibility> = BTreeMap::new();
    for event in &original {
        match event.kind {
            EventKind::ToolStarted => {
                if event.call_id.is_empty()
                    || active
                        .insert(&event.call_id, visibility(checkpoint, event.sequence))
                        .is_some()
                {
                    return Err(ViewProblem::InvalidToolSequence);
                }
            }
            EventKind::ToolFinished => {
                let Some(start_visibility) = active.remove(event.call_id.as_str()) else {
                    return Err(ViewProblem::InvalidToolSequence);
                };
                if (start_visibility == Visibility::Retained)
                    != (visibility(checkpoint, event.sequence) == Visibility::Retained)
                {
                    return Err(ViewProblem::SplitToolCall);
                }
            }
            _ => {}
        }
    }
    if !active.is_empty() {
        return Err(ViewProblem::UnresolvedToolCall);
    }

    let mut bytes = 0u64;
    let mut parts = Vec::new();
    let mut summary_emitted = false;
    for event in original {
        match visibility(checkpoint, event.sequence) {
            Visibility::Summarized => {
                if !summary_emitted {
                    let text = checkpoint.summary.clone();
                    let size = u64::try_from(text.len()).map_err(|_| ViewProblem::OverBudget)?;
                    bytes = bytes.checked_add(size).ok_or(ViewProblem::OverBudget)?;
                    parts.push(ContextPart::Summary {
                        checkpoint_id: checkpoint.id.clone(),
                        text,
                        sources: checkpoint.summarized.clone(),
                    });
                    summary_emitted = true;
                }
            }
            Visibility::Retained => {
                let size = event
                    .text
                    .len()
                    .checked_add(event.data.0.len())
                    .and_then(|n| n.checked_add(event.call_id.len()))
                    .and_then(|n| u64::try_from(n).ok())
                    .ok_or(ViewProblem::OverBudget)?;
                bytes = bytes.checked_add(size).ok_or(ViewProblem::OverBudget)?;
                parts.push(ContextPart::Original(event));
            }
            Visibility::Excluded => {}
        }
        if bytes > max_visible_payload_bytes {
            return Err(ViewProblem::OverBudget);
        }
    }
    Ok(ContextProjection {
        checkpoint_id: checkpoint.id.clone(),
        source_span: checkpoint.input.clone(),
        parts,
        excluded: checkpoint.excluded.clone(),
        visible_payload_bytes: bytes,
    })
}
