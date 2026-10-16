//! P2-3v: join selected Model pressure to a source-verified checkpoint plan.
//!
//! This function only decides whether a candidate exists. The Advanced graph
//! owns authenticated provider selection, summary quality, journal reads,
//! checkpoint CAS, and a new selected-Model measurement before execution.
use crate::{
    checkpoint_pressure, plan_verified_checkpoint, CheckpointPlan, CheckpointPressure, PlanProblem,
    PressureProblem,
};
use phenix_advanced_contracts::{CompactionCheckpoint, ModelTokenMeasurement};
use phenix_basic_contracts::{Event, ModelRequest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomaticCheckpointDecision {
    /// The original, measured request fits the selected context window.
    UseOriginal,
    /// A source-verified candidate. The caller must commit it through the
    /// selected store, rebuild the Model request, then measure that new request.
    /// A mandatory candidate must never fall back to the over-window original.
    Candidate {
        checkpoint: Box<CompactionCheckpoint>,
        mandatory: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomaticCheckpointProblem {
    Pressure(PressureProblem),
    Plan(PlanProblem),
    /// The original request exceeds its context window and no verified
    /// completed-turn prefix can be removed. Deny before Model execution.
    NoSafeCheckpoint,
}

/// Combine a caller-supplied source-verified history and selected token meter
/// without authorizing or executing any effect.
///
/// The caller authenticates that the meter and Model are the same selected
/// provider. Inputs from journal and checkpoint providers must be retrieved
/// under their own session-scoped grants. Candidate publication is not Model
/// admission: after CAS, the caller must rebuild and meter the exact new
/// request and deny if the new request still exceeds its context window.
pub fn automatic_checkpoint(
    request: &ModelRequest,
    measurement: &ModelTokenMeasurement,
    selected_model: &str,
    high_water_percent: u64,
    journal: &[Event],
    previous: Option<&CompactionCheckpoint>,
    policy: CheckpointPlan<'_>,
) -> Result<AutomaticCheckpointDecision, AutomaticCheckpointProblem> {
    let pressure = checkpoint_pressure(request, measurement, selected_model, high_water_percent)
        .map_err(AutomaticCheckpointProblem::Pressure)?;
    if pressure == CheckpointPressure::EnoughHeadroom {
        return Ok(AutomaticCheckpointDecision::UseOriginal);
    }

    let candidate = plan_verified_checkpoint(request, journal, previous, policy)
        .map_err(AutomaticCheckpointProblem::Plan)?;
    match (pressure, candidate) {
        (
            CheckpointPressure::AtThreshold | CheckpointPressure::OverContextWindow,
            Some(checkpoint),
        ) => Ok(AutomaticCheckpointDecision::Candidate {
            checkpoint: Box::new(checkpoint),
            mandatory: pressure == CheckpointPressure::OverContextWindow,
        }),
        (CheckpointPressure::AtThreshold, None) => Ok(AutomaticCheckpointDecision::UseOriginal),
        (CheckpointPressure::OverContextWindow, None) => {
            Err(AutomaticCheckpointProblem::NoSafeCheckpoint)
        }
        (CheckpointPressure::EnoughHeadroom, _) => unreachable!("handled above"),
    }
}
