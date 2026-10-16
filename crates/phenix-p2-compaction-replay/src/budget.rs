//! P2-3u: pure compaction pressure for a selected, measured ModelRequest.
//!
//! The caller MUST authenticate co-binding of its selected Model and token
//! meter through the existing host slot selection. A model name alone does not
//! authenticate a provider. This policy never calls Model, reads journal
//! sources, writes a checkpoint or mutates Basic session history.
use phenix_advanced_contracts::{
    validate_model_token_measurement, ModelTokenMeasurement, ModelTokenProblem,
};
use phenix_basic_contracts::ModelRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointPressure {
    EnoughHeadroom,
    AtThreshold,
    OverContextWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressureProblem {
    InvalidThreshold,
    Measurement(ModelTokenProblem),
}

/// Determine if a graph-owned checkpoint policy should run before attempting
/// Model admission. The selected provider measures the complete original
/// ModelRequest including tool schemas, skills and existing Context.
///
/// A valid measurement that exceeds the window *still* triggers compaction;
/// forged digests or mismatched Model identities never do. The caller must
/// separately verify selected provider co-binding and submit the exact request
/// through its canonical measured/verified pipeline after compaction.
///
/// high_water_percent is 1..=100; all multiplication uses u128 so large
/// context limits do not wrap and incorrectly suppress compaction.
pub fn checkpoint_pressure(
    request: &ModelRequest,
    measurement: &ModelTokenMeasurement,
    selected_model: &str,
    output_reserve: u64,
    high_water_percent: u64,
) -> Result<CheckpointPressure, PressureProblem> {
    if !(1..=100).contains(&high_water_percent) {
        return Err(PressureProblem::InvalidThreshold);
    }
    let checked = validate_model_token_measurement(
        request,
        measurement,
        selected_model,
        output_reserve,
    );
    match checked {
        Ok(()) => {}
        Err(ModelTokenProblem::OverBudget) if output_reserve > 0 => {
            // Identity, tokenizer and request digest were validated before
            // capacity by the canonical measurement verifier.
            return Ok(CheckpointPressure::OverContextWindow);
        }
        Err(problem) => return Err(PressureProblem::Measurement(problem)),
    }
    let total_tokens =
        u128::from(measurement.input_tokens) + u128::from(output_reserve);
    let used = total_tokens * 100;
    let threshold =
        u128::from(measurement.context_window_tokens) * u128::from(high_water_percent);
    Ok(if used >= threshold {
        CheckpointPressure::AtThreshold
    } else {
        CheckpointPressure::EnoughHeadroom
    })
}
