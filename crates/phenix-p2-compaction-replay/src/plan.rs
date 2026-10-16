//! Pure P2-3t checkpoint proposal policy.
//!
//! The selected graph owns trigger, summarization and authorization. A plan
//! is a candidate only: the selected CompactionStore performs source checks,
//! lineage CAS and durable commit, and no journal bytes are overwritten.
use crate::{
    assemble_verified_basic_request, compacted_view, restore_original_events, span_for_events,
};
use phenix_advanced_contracts::{validate_checkpoint, CompactionCheckpoint};
use phenix_basic_contracts::{Event, EventKind, ModelRequest};

const MAX_SOURCE_EVENTS: usize = 1024;
const MAX_VISIBLE_PAYLOAD_BYTES: u64 = 8 * 1024 * 1024;

/// Caller-selected policy inputs are data, not execution privileges. A
/// separately selected summarizer must supply source-grounded summary text.
#[derive(Debug, Clone, Copy)]
pub struct CheckpointPlan<'a> {
    pub id: &'a str,
    pub revision: u64,
    pub parent_checkpoint: Option<&'a str>,
    pub summary: &'a str,
    pub retain_completed_turns: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanProblem {
    InvalidPolicy,
    InvalidLineage,
    InvalidSource,
    UnfinishedTurn,
    UnverifiedHistory,
}

/// Produce a source-cited candidate only when enough complete older turns
/// exist. Every input byte and the authoritative full ModelRequest must agree
/// before a proposed summary may elide any message. A legacy tool-bearing
/// turn without an exact AssistantReply receipt can never be summarized.
///
/// The caller must obtain events and any previous checkpoint through the
/// selected, authorized providers. The summary's factual quality cannot be
/// certified by this structural planner and remains a separate policy gate.
pub fn plan_verified_checkpoint(
    request: &ModelRequest,
    journal: &[Event],
    previous: Option<&CompactionCheckpoint>,
    policy: CheckpointPlan<'_>,
) -> Result<Option<CompactionCheckpoint>, PlanProblem> {
    if policy.id.trim().is_empty()
        || policy.summary.trim().is_empty()
        || policy.summary.len() > 65_536
        || policy.retain_completed_turns == 0
        || journal.is_empty()
        || journal.len() > MAX_SOURCE_EVENTS
    {
        return Err(PlanProblem::InvalidPolicy);
    }
    let expected_revision = previous
        .map(|checkpoint| checkpoint.revision.checked_add(1))
        .unwrap_or(Some(1))
        .ok_or(PlanProblem::InvalidLineage)?;
    if policy.revision != expected_revision
        || policy.parent_checkpoint != previous.map(|checkpoint| checkpoint.id.as_str())
        || previous.is_some_and(|checkpoint| checkpoint.id == policy.id)
    {
        return Err(PlanProblem::InvalidLineage);
    }
    let source_span = span_for_events(journal).map_err(|_| PlanProblem::InvalidSource)?;
    if request.session != journal[0].session
        || journal
            .last()
            .is_none_or(|event| event.kind != EventKind::ModelStarted || event.run != request.run)
    {
        return Err(PlanProblem::InvalidSource);
    }
    if let Some(previous) = previous {
        if previous.session != request.session
            || previous.input.first_sequence < source_span.first_sequence
            || previous.input.last_sequence > source_span.last_sequence
        {
            return Err(PlanProblem::InvalidSource);
        }
        let from = usize::try_from(previous.input.first_sequence - source_span.first_sequence)
            .map_err(|_| PlanProblem::InvalidSource)?;
        let to = usize::try_from(previous.input.last_sequence - source_span.first_sequence + 1)
            .map_err(|_| PlanProblem::InvalidSource)?;
        let historical = journal.get(from..to).ok_or(PlanProblem::InvalidSource)?;
        restore_original_events(previous, historical).map_err(|_| PlanProblem::InvalidSource)?;
        if previous.input == source_span {
            // A retry with the same attested source does not create a new
            // revision or trigger another summarization effect.
            return Ok(None);
        }
    }
    // A safe split is allowed only after a terminal Answer of an entire
    // run. Never summarize a pending, failed, cancelled or partial tool turn.
    let mut active_run: Option<&str> = None;
    let mut completed = Vec::new();
    for (index, event) in journal.iter().enumerate() {
        match event.kind {
            EventKind::PromptAccepted => {
                if active_run.is_some() {
                    return Err(PlanProblem::UnfinishedTurn);
                }
                active_run = Some(event.run.as_str());
            }
            EventKind::Answer => {
                if active_run != Some(event.run.as_str()) {
                    return Err(PlanProblem::UnfinishedTurn);
                }
                completed.push(index + 1);
                active_run = None;
            }
            EventKind::Failure | EventKind::Cancelled => {
                return Err(PlanProblem::UnfinishedTurn);
            }
            _ if active_run != Some(event.run.as_str()) => {
                return Err(PlanProblem::UnfinishedTurn);
            }
            _ => {}
        }
    }
    if active_run != Some(request.run.as_str()) {
        return Err(PlanProblem::InvalidSource);
    }
    if completed.len() <= policy.retain_completed_turns {
        // No checkpoint is warranted. The caller still holds its original
        // full ModelRequest, without any new storage mutation.
        return Ok(None);
    }
    // Dry-run a full retained projection to check alignment of the complete
    // actual Basic session before permitting elision. This catches forged
    // text, tool responses, missing receipts and mismatched history lengths.
    let mut candidate = CompactionCheckpoint {
        id: policy.id.into(),
        session: request.session.clone(),
        revision: policy.revision,
        parent_checkpoint: policy.parent_checkpoint.map(str::to_owned),
        input: source_span,
        summarized: vec![],
        retained: vec![span_for_events(journal).map_err(|_| PlanProblem::InvalidSource)?],
        excluded: vec![],
        summary: String::new(),
        policy: "p2-verified-completed-turns".into(),
        policy_version: 1,
        model: None,
        hydration_sources: vec![],
    };
    let full_view = compacted_view(&candidate, journal, MAX_VISIBLE_PAYLOAD_BYTES)
        .map_err(|_| PlanProblem::InvalidSource)?;
    assemble_verified_basic_request(request, journal, &candidate, &full_view)
        .map_err(|_| PlanProblem::UnverifiedHistory)?;

    let split_at = completed[completed.len() - policy.retain_completed_turns - 1];
    candidate.summarized =
        vec![span_for_events(&journal[..split_at]).map_err(|_| PlanProblem::InvalidSource)?];
    candidate.retained =
        vec![span_for_events(&journal[split_at..]).map_err(|_| PlanProblem::InvalidSource)?];
    candidate.summary = policy.summary.into();
    validate_checkpoint(&candidate).map_err(|_| PlanProblem::InvalidPolicy)?;
    let view = compacted_view(&candidate, journal, MAX_VISIBLE_PAYLOAD_BYTES)
        .map_err(|_| PlanProblem::InvalidSource)?;
    assemble_verified_basic_request(request, journal, &candidate, &view)
        .map_err(|_| PlanProblem::UnverifiedHistory)?;
    Ok(Some(candidate))
}
