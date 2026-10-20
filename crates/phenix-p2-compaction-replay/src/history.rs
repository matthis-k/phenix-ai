//! Conservative ModelRequest projection: byte-match Basic's authoritative
//! SessionSnapshot messages to live EventJournal before eliding old turns.
//!
//! EventJournal is *not* a complete Model transcript: tool arguments and some
//! assistant messages exist only in SessionSnapshot. Tool-bearing history may be retained byte-for-byte from SessionSnapshot only
//! after its call IDs, selected tool names and results align with EventJournal.
//! Tool arguments and assistant tool-call text are absent from legacy journal
//! events. Only new typed AssistantReply receipts allow that older tool turn
//! to be summarized; legacy tool-bearing turns must remain fully retained.
use crate::{compacted_view, span_for_events, ContextPart, ContextProjection};
use phenix_advanced_contracts::CompactionCheckpoint;
use phenix_basic_contracts::{
    decode, ContextItem, Event, EventKind, Message, ModelRequest, Role, ToolCompletion,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

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
/// Model message from journal records. Tool transactions may be elided only
/// when every assistant call is independently journaled with its exact typed
/// Message, matched to tool start/completion events and source-verified.
pub fn assemble_verified_basic_request(
    authoritative: &ModelRequest,
    journal: &[Event],
    checkpoint: &CompactionCheckpoint,
    view: &ContextProjection,
) -> Result<ModelRequest, TranscriptProblem> {
    if journal
        .iter()
        .any(|event| event.session != authoritative.session)
    {
        return Err(TranscriptProblem::WrongSession);
    }
    if span_for_events(journal).ok().as_ref() != Some(&view.source_span) {
        return Err(TranscriptProblem::UnalignedSource);
    }
    // The view is a serializable/public structure, not an authority token.
    // Independently rederive it from the checkpoint and exact source bytes;
    // a caller must not inject forged summary text or missing provenance.
    let verified = compacted_view(checkpoint, journal, u64::MAX)
        .map_err(|_| TranscriptProblem::UnalignedSource)?;
    if &verified != view {
        return Err(TranscriptProblem::UnalignedSource);
    }
    let message_tools = authoritative.history.iter().any(|message| {
        message.role() == Role::Tool || !message.calls().is_empty() || !message.call_id().is_empty()
    });
    let journal_tools = journal
        .iter()
        .any(|event| matches!(event.kind, EventKind::ToolStarted | EventKind::ToolFinished));
    if message_tools != journal_tools {
        return Err(TranscriptProblem::UnsupportedToolHistory);
    }
    let tool_runs: BTreeSet<&str> = journal
        .iter()
        .filter(|event| matches!(event.kind, EventKind::ToolStarted | EventKind::ToolFinished))
        .map(|event| event.run.as_str())
        .collect();

    let mut retained_sequences = BTreeSet::new();
    let mut summary = None;
    for part in &view.parts {
        match part {
            ContextPart::Summary {
                checkpoint_id,
                text,
                ..
            } => {
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

    let mut tool_run_visibility: BTreeMap<&str, bool> = BTreeMap::new();
    let mut retained_history = Vec::<Message>::new();
    let mut matched = 0usize;
    let mut pending_prompt: Option<bool> = None;
    let mut current_run: Option<&str> = None;
    let mut queued_calls = VecDeque::new();
    let mut active_tool: Option<String> = None;
    let mut seen_ids = BTreeSet::new();
    for event in journal {
        let retained = retained_sequences.contains(&event.sequence);
        // A tool-bearing run must occupy one complete retained or summarized
        // span. Exclusion or a mixed visibility would drop source messages.
        // Eliding a tool run is permitted only if each Model tool-call reply
        // has an exact, separately journaled AssistantReply receipt below.
        if tool_runs.contains(event.run.as_str()) {
            let summarized = checkpoint.summarized.iter().any(|span| {
                span.first_sequence <= event.sequence && event.sequence <= span.last_sequence
            });
            if !retained && !summarized {
                return Err(TranscriptProblem::UnsupportedToolHistory);
            }
            if let Some(previous) = tool_run_visibility.insert(event.run.as_str(), retained) {
                if previous != retained {
                    return Err(TranscriptProblem::SplitTurn);
                }
            }
        }
        match event.kind {
            EventKind::PromptAccepted => {
                if pending_prompt.is_some() || active_tool.is_some() || !queued_calls.is_empty() {
                    return Err(TranscriptProblem::UnalignedHistory);
                }
                let message = authoritative
                    .history
                    .get(matched)
                    .ok_or(TranscriptProblem::UnalignedHistory)?;
                if message.role() != Role::User
                    || message.text() != event.text
                    || !message.calls().is_empty()
                    || !message.call_id().is_empty()
                    || message.error().is_some()
                {
                    return Err(TranscriptProblem::UnalignedHistory);
                }
                pending_prompt = Some(retained);
                current_run = Some(&event.run);
                seen_ids.clear();
                if retained {
                    retained_history.push(message.clone());
                }
                matched += 1;
            }
            _ if pending_prompt.is_some() && current_run != Some(event.run.as_str()) => {
                return Err(TranscriptProblem::UnalignedHistory);
            }
            EventKind::AssistantReply => {
                // The optional Advanced journal receipt contains the exact
                // assistant message, including every tool name and argument.
                // It is verified against independently persisted Session
                // history and immutable source event bytes before elision.
                if pending_prompt != Some(retained)
                    || active_tool.is_some()
                    || !queued_calls.is_empty()
                {
                    return Err(TranscriptProblem::UnalignedHistory);
                }
                let receipt = decode::<Message>(&event.data)
                    .map_err(|_| TranscriptProblem::UnalignedSource)?;
                let assistant = authoritative
                    .history
                    .get(matched)
                    .ok_or(TranscriptProblem::UnalignedHistory)?;
                if receipt != *assistant
                    || receipt.role() != Role::Assistant
                    || receipt.text() != event.text
                    || receipt.calls().is_empty()
                    || !receipt.call_id().is_empty()
                    || receipt.error().is_some()
                    || receipt
                        .calls
                        .iter()
                        .any(|call| call.id.is_empty() || !seen_ids.insert(call.id.clone()))
                {
                    return Err(TranscriptProblem::UnalignedHistory);
                }
                queued_calls.extend(receipt.calls());
                if retained {
                    retained_history.push(assistant.clone());
                }
                matched += 1;
            }
            EventKind::ToolStarted => {
                if pending_prompt != Some(retained) || active_tool.is_some() {
                    return Err(TranscriptProblem::UnsupportedToolHistory);
                }
                if queued_calls.is_empty() {
                    // Legacy journals have no arguments. Such a reply may
                    // remain visible, but can never be placed in a summary.
                    if !retained {
                        return Err(TranscriptProblem::UnsupportedToolHistory);
                    }
                    let assistant = authoritative
                        .history
                        .get(matched)
                        .ok_or(TranscriptProblem::UnalignedHistory)?;
                    if assistant.role() != Role::Assistant
                        || assistant.calls().is_empty()
                        || !assistant.call_id().is_empty()
                        || assistant.error().is_some()
                        || assistant
                            .calls
                            .iter()
                            .any(|call| call.id.is_empty() || !seen_ids.insert(call.id.clone()))
                    {
                        return Err(TranscriptProblem::UnalignedHistory);
                    }
                    queued_calls.extend(assistant.calls().iter().cloned());
                    retained_history.push(assistant.clone());
                    matched += 1;
                }
                let call = queued_calls
                    .pop_front()
                    .ok_or(TranscriptProblem::UnalignedHistory)?;
                if call.id != event.call_id || call.tool != event.text {
                    return Err(TranscriptProblem::UnalignedHistory);
                }
                active_tool = Some(call.id);
            }
            EventKind::ToolFinished => {
                if pending_prompt != Some(retained) {
                    return Err(TranscriptProblem::UnsupportedToolHistory);
                }
                let id = active_tool
                    .take()
                    .ok_or(TranscriptProblem::UnalignedHistory)?;
                let message = authoritative
                    .history
                    .get(matched)
                    .ok_or(TranscriptProblem::UnalignedHistory)?;
                if message.role() != Role::Tool
                    || message.call_id() != id
                    || event.call_id != id
                    || !message.calls().is_empty()
                {
                    return Err(TranscriptProblem::UnalignedHistory);
                }
                let failed = matches!(
                    decode::<ToolCompletion>(&event.data)
                        .map_err(|_| TranscriptProblem::UnalignedSource)?,
                    ToolCompletion::Failed
                );
                if failed {
                    if !message.text().is_empty()
                        || message.error() != Some(event.text.as_str())
                    {
                        return Err(TranscriptProblem::UnalignedHistory);
                    }
                } else if message.text() != event.text || message.error().is_some() {
                    return Err(TranscriptProblem::UnalignedHistory);
                }
                if retained {
                    retained_history.push(message.clone());
                }
                matched += 1;
            }
            EventKind::Answer => {
                let Some(user_retained) = pending_prompt.take() else {
                    return Err(TranscriptProblem::UnalignedHistory);
                };
                if user_retained != retained {
                    return Err(TranscriptProblem::SplitTurn);
                }
                if active_tool.is_some() || !queued_calls.is_empty() {
                    return Err(TranscriptProblem::UnalignedHistory);
                }
                let message = authoritative
                    .history
                    .get(matched)
                    .ok_or(TranscriptProblem::UnalignedHistory)?;
                if message.role() != Role::Assistant
                    || message.text() != event.text
                    || !message.calls().is_empty()
                    || !message.call_id().is_empty()
                    || message.error().is_some()
                {
                    return Err(TranscriptProblem::UnalignedHistory);
                }
                if retained {
                    retained_history.push(message.clone());
                }
                matched += 1;
                current_run = None;
            }
            EventKind::ModelStarted | EventKind::TextDelta => {
                if pending_prompt.is_none() {
                    return Err(TranscriptProblem::UnalignedHistory);
                }
            }
            // Failed/incomplete historical turns are not valid compacted
            // conversation history. Require complete journal+snapshot turns.
            EventKind::Failure | EventKind::Cancelled => {
                return Err(TranscriptProblem::UnalignedHistory);
            }
        }
    }
    if matched != authoritative.history.len() {
        return Err(TranscriptProblem::UnalignedHistory);
    }
    if current_run != Some(authoritative.run.as_str()) {
        return Err(TranscriptProblem::UnalignedHistory);
    }
    if pending_prompt != Some(true)
        || active_tool.is_some()
        || !queued_calls.is_empty()
        || retained_history
            .last()
            .is_none_or(|msg| msg.role() != Role::User && msg.role() != Role::Tool)
    {
        return Err(TranscriptProblem::MissingCurrentPrompt);
    }
    // A Model continuation after a tool result legitimately has no final
    // Answer yet. Its last journal record must be a new ModelStarted for the
    // same run, and the complete tool transaction remains in the request.
    if retained_history
        .last()
        .is_some_and(|msg| msg.role() == Role::Tool)
        && journal
            .last()
            .is_none_or(|event| event.kind != EventKind::ModelStarted)
    {
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
