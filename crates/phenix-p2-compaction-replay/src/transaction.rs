//! P2-3z: one pre-Model compaction transaction for a selected graph step.
//!
//! This function grants no new authority. A caller binds the selected Model
//! meter, source journal, checkpoint store and summarizer through existing
//! M1 imports. The kernel owns graph scheduling and cancellation. The selected
//! CompactionStore owns source validation and revision CAS. No tool or Model
//! execution occurs inside this policy.
use crate::{
    automatic_checkpoint, publish_verified_checkpoint, remeasure_verified_checkpoint,
    AutomaticCheckpointDecision, AutomaticCheckpointProblem, CheckpointPlan,
    CheckpointPublishProblem, CompactedAdmissionProblem,
};
use phenix_advanced_contracts::{CompactionCheckpoint, ModelTokenMeasurement};
use phenix_basic_contracts::{Event, ModelRequest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionAdmission {
    /// The exact request allowed to reach the already-selected Model.
    pub request: ModelRequest,
    pub measurement: ModelTokenMeasurement,
    /// Source-verified and durably published only on the checkpoint path.
    pub checkpoint: Option<CompactionCheckpoint>,
    pub mandatory: bool,
    /// Whether this admission created a new CAS revision.
    pub created: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactionAdmissionProblem<StoreError, MeterError> {
    Decision(AutomaticCheckpointProblem),
    Publication(CheckpointPublishProblem<StoreError>),
    Remeasure(CompactedAdmissionProblem<MeterError>),
}

#[derive(Debug, Clone, Copy)]
pub struct CompactionAdmissionInput<'a> {
    pub request: &'a ModelRequest,
    pub original_measurement: &'a ModelTokenMeasurement,
    pub selected_model: &'a str,
    pub high_water_percent: u64,
    pub journal: &'a [Event],
    pub previous: Option<&'a CompactionCheckpoint>,
    pub policy: CheckpointPlan<'a>,
}

/// Connect existing verified policy stages without any downstream Model call.
///
/// The graph must obtain a faithful summary, ModelRequest and exact journal
/// from authorized providers before calling this helper. The latest/put
/// callbacks must invoke the same selected CompactionStore; the measure
/// callback must invoke the pinned Model's selected token meter. All
/// imported authority and generation identity are checked by the kernel.
///
/// A valid pressure reading with enough headroom skips all callbacks. A
/// mandatory over-window request never falls back to the original history.
/// A successful CAS followed by failed metering can leave a durable, reversible
/// checkpoint, but never executes the Model or replays tools.
pub fn admit_compaction<StoreError, MeterError>(
    input: CompactionAdmissionInput<'_>,
    latest: impl FnMut() -> Result<Option<CompactionCheckpoint>, StoreError>,
    put: impl FnMut(&CompactionCheckpoint) -> Result<CompactionCheckpoint, StoreError>,
    measure: impl FnOnce(&ModelRequest) -> Result<ModelTokenMeasurement, MeterError>,
) -> Result<CompactionAdmission, CompactionAdmissionProblem<StoreError, MeterError>> {
    let CompactionAdmissionInput {
        request,
        original_measurement,
        selected_model,
        high_water_percent,
        journal,
        previous,
        policy,
    } = input;
    let decision = automatic_checkpoint(
        request,
        original_measurement,
        selected_model,
        high_water_percent,
        journal,
        previous,
        policy,
    )
    .map_err(CompactionAdmissionProblem::Decision)?;

    let AutomaticCheckpointDecision::Candidate {
        checkpoint,
        mandatory,
    } = decision
    else {
        return Ok(CompactionAdmission {
            request: request.clone(),
            measurement: original_measurement.clone(),
            checkpoint: None,
            mandatory: false,
            created: false,
        });
    };

    let published = publish_verified_checkpoint(&checkpoint, latest, put)
        .map_err(CompactionAdmissionProblem::Publication)?;
    let candidate = remeasure_verified_checkpoint(
        request,
        journal,
        &published.checkpoint,
        original_measurement,
        selected_model,
        8 * 1024 * 1024,
        measure,
    )
    .map_err(CompactionAdmissionProblem::Remeasure)?;
    Ok(CompactionAdmission {
        request: candidate.request,
        measurement: candidate.measurement,
        checkpoint: Some(published.checkpoint),
        mandatory,
        created: published.created,
    })
}
