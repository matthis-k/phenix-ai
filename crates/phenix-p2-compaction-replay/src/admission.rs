//! P2-3w: source-verified, remeasured Model admission after compaction.
//!
//! A checkpoint policy proposal does not itself establish Model capacity.
//! The caller fetches authorized journal bytes and a selected checkpoint,
//! authenticates Model/meter co-binding and commits the checkpoint through
//! the existing CAS provider. This helper rebuilds the complete request from
//! authoritative history and exact journal sources, then independently meters
//! that same request. It performs no persistence or upstream Model effects.
use crate::{
    assemble_verified_basic_request, checkpoint_pressure, compacted_view, PressureProblem,
    TranscriptProblem, ViewProblem,
};
use phenix_advanced_contracts::{
    validate_model_token_measurement, CompactionCheckpoint, ModelTokenMeasurement,
    ModelTokenProblem,
};
use phenix_basic_contracts::{Event, ModelRequest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeasuredCompactedRequest {
    /// Submit this exact request through the selected Model binding unchanged.
    pub request: ModelRequest,
    /// Selected Model measurement of the returned exact request.
    pub measurement: ModelTokenMeasurement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactedAdmissionProblem<E> {
    OriginalMeasurement(PressureProblem),
    Projection(ViewProblem),
    History(TranscriptProblem),
    NoHistoryElided,
    Meter(E),
    ModelConfigurationChanged,
    CandidateMeasurement(ModelTokenProblem),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactedMeterProblem {
    ModelConfigurationChanged,
    CandidateMeasurement(ModelTokenProblem),
}

/// Verify that the same selected Model configuration measured the source
/// request and the exact compacted request. The caller separately checks
/// original pressure and authenticates the two Model/meter slot bindings.
/// This accepts no fallback to the original request after compaction fails.
pub fn validate_compacted_meter_transition(
    original: &ModelRequest,
    rebuilt: &ModelRequest,
    original_measurement: &ModelTokenMeasurement,
    rebuilt_measurement: &ModelTokenMeasurement,
    selected_model: &str,
) -> Result<(), CompactedMeterProblem> {
    if rebuilt_measurement.tokenizer_revision != original_measurement.tokenizer_revision
        || rebuilt_measurement.context_window_tokens != original_measurement.context_window_tokens
    {
        return Err(CompactedMeterProblem::ModelConfigurationChanged);
    }
    let reserve = original.limits.model_tokens.unwrap_or(0);
    validate_model_token_measurement(rebuilt, rebuilt_measurement, selected_model, reserve)
        .map_err(CompactedMeterProblem::CandidateMeasurement)
}

/// The original and rebuilt requests must use one pinned Model configuration.
///
/// The original measurement must come from the authenticated selected provider.
/// The measure callback invokes that same selected provider for the exact
/// rebuilt request. Even an over-window original can be verified here, but
/// the rebuilt request must fit after reserving the original output budget.
/// The caller must submit the returned request unchanged and prevent an
/// upstream Model call if this helper rejects it.
///
/// Source integrity cannot be inferred from the returned measurement.
/// The journal comes from authorized reads and the checkpoint's ancestor
/// lineage must be checked separately by the selected CompactionStore.
pub fn remeasure_verified_checkpoint<E>(
    original: &ModelRequest,
    journal: &[Event],
    checkpoint: &CompactionCheckpoint,
    original_measurement: &ModelTokenMeasurement,
    selected_model: &str,
    max_visible_payload_bytes: u64,
    measure: impl FnOnce(&ModelRequest) -> Result<ModelTokenMeasurement, E>,
) -> Result<MeasuredCompactedRequest, CompactedAdmissionProblem<E>> {
    // Reuse the canonical exact-request verifier, accepting an original over
    // its context window only as a compaction candidate, never as Model input.
    checkpoint_pressure(original, original_measurement, selected_model, 100)
        .map_err(CompactedAdmissionProblem::OriginalMeasurement)?;

    let view = compacted_view(checkpoint, journal, max_visible_payload_bytes)
        .map_err(CompactedAdmissionProblem::Projection)?;
    let request = assemble_verified_basic_request(original, journal, checkpoint, &view)
        .map_err(CompactedAdmissionProblem::History)?;
    if checkpoint.summarized.is_empty() || request.history.len() >= original.history.len() {
        return Err(CompactedAdmissionProblem::NoHistoryElided);
    }

    let measurement = measure(&request).map_err(CompactedAdmissionProblem::Meter)?;
    validate_compacted_meter_transition(
        original,
        &request,
        original_measurement,
        &measurement,
        selected_model,
    )
    .map_err(|problem| match problem {
        CompactedMeterProblem::ModelConfigurationChanged => {
            CompactedAdmissionProblem::ModelConfigurationChanged
        }
        CompactedMeterProblem::CandidateMeasurement(reason) => {
            CompactedAdmissionProblem::CandidateMeasurement(reason)
        }
    })?;
    Ok(MeasuredCompactedRequest {
        request,
        measurement,
    })
}
