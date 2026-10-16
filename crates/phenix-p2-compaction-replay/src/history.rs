//! Conservative ModelRequest projection: byte-match Basic's authoritative
//! SessionSnapshot messages to live EventJournal before eliding old turns.
//!
//! EventJournal is *not* a complete Model transcript: tool arguments and some
//! assistant messages exist only in SessionSnapshot. Tool-bearing histories
//! therefore fail closed until explicit message-to-source lineage is available.
use crate::{span_for_events, ContextPart, ContextProjection};
use phenix_basic_contracts::{ContextItem, Event, EventKind, Message, ModelRequest, Role};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptProblem {
    WrongSession,
    UnalignedSource,
    UnalignedHistory,
    UnsupportedToolHistory,
    SplitTurn,
    ContextConflict,
    MissingCurrentPrompt,
}

/// Assemble an input for an opt-in text-only Advanced Model policy from a
/// source-verified ContextProjection and independently persisted Session
/// snapshot. The caller must obtain the view via `compacted_view` or
/// `select_model_view` (with authorization + source digest validation), and
/// submit the returned request to the same selected Model adapter.
///
/// Does *not* modify the Basic session or journal. Never fabricate a missing
/// Model message from journal records. Non-text/tool histories are rejected.
pub fn assemble_verified_basic_request(
    authoritative: &ModelRequest,
    journal: &[Event],
    view: &ContextProjection,
) -> Result<ModelRequest, TranscriptProblem> {
    if journal.iter().any(|event| event.session != authoritative.session) {
        return Err(TranscriptProblem::WrongSession);
    }
    if span_for_events(journal).ok().as_ref() != Some(&view.source_span) {
        return Err(TranscriptProblem::UnalignedSource);
    }
    if authoritative.history.iter().any(|message| {
        message.role == Role::Tool
            || !message.calls.is_empty()
            || !message.call_id.is_empty()
            || message.error.is_some()
    }) || journal.iter().any(|event| {
        matches!(event.kind, EventKind::ToolStarted | EventKind::ToolFinished)
    }) {
        return Err(TranscriptProblem::UnsupportedToolHistory);
    }

    let mut retained_sequences = BTreeSet::new();
    let mut summary = None;
    for part in &view.parts {
        match part {
            ContextPart::Summary { checkpoint_id, text, .. } => {
                if checkpoint_id != &view.checkpoint_id || summary.replace(text).is_some() {
                    return Err(TranscriptProblem::UnalignedSource);
                }
            }
            ContextPart::Original(event) => {
                if !retained_sequences.insert(event.sequence)
                    || !journal.iter().any(|source| source == event)
                {
                    return Err(TranscriptProblem::UnalignedSource);
                }
            }
        }
    }

    let mut retained_history = Vec::<Message>::new();
    let mut matched = 0usize;
    let mut pending_prompt: Option<bool> = None;
    for event in journal {
        let role = match event.kind {
            EventKind::PromptAccepted => Role::User,
            EventKind::Answer => Role::Assistant,
            _ => continue,
        };
        let message = authoritative
            .history
            .get(matched)
            .ok_or(TranscriptProblem::UnalignedHistory)?;
        if message.role != role || message.text != event.text {
            return Err(TranscriptProblem::UnalignedHistory);
        }
        let retained = retained_sequences.contains(&event.sequence);
        match role {
            Role::User => {
                if pending_prompt.is_some() {
                    return Err(TranscriptProblem::UnalignedHistory);
                }
                pending_prompt = Some(retained);
            }
            Role::Assistant => {
                let Some(user_retained) = pending_prompt.take() else {
                    return Err(TranscriptProblem::UnalignedHistory);
                };
                if user_retained != retained {
                    return Err(TranscriptProblem::SplitTurn);
                }
            }
            Role::Tool => unreachable!("tool messages rejected above"),
        }
        if retained {
            retained_history.push(message.clone());
        }
        matched += 1;
    }
    if matched != authoritative.history.len() {
        return Err(TranscriptProblem::UnalignedHistory);
    }
    if pending_prompt != Some(true)
        || retained_history.last().is_none_or(|msg| msg.role != Role::User)
    {
        // Only the in-progress current user prompt may end without an Answer.
        // It must not disappear behind a compacted or excluded range.
        return Err(TranscriptProblem::MissingCurrentPrompt);
    }
    let mut request = authoritative.clone();
    request.history = retained_history;
    if let Some(text) = summary {
        let id = format!("compaction:{}", view.checkpoint_id);
        if request.context.iter().any(|item| item.id == id) {
            return Err(TranscriptProblem::ContextConflict);
        }
        request.context.push(ContextItem {
            id,
            text: text.clone(),
        });
    }
    Ok(request)
}
