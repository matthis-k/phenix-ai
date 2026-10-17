//! P2-3ah: source-bound graph preparation proposal, never an authority token.
//!
//! The existing M1 graph must authorize the selected provider and its actual
//! writes independently. A checkpoint citation is untrusted until exact
//! EventJournal source verification, CAS readback and selected Model metering.
use crate::{model_request_sha256, validate_checkpoint, CompactionCheckpoint};
use phenix_basic_contracts::ModelRequest;
use phenix_sdk::Portable;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ModelPreparationInput {
    /// The exact Basic-projected request, captured before Advanced hooks.
    pub original: ModelRequest,
    /// Domain-separated hash of its complete canonical Portable encoding.
    pub original_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ModelPreparationProposal {
    /// Must bind the same original request as the trusted Basic projection.
    pub original_sha256: String,
    /// Untrusted replacement, not executable until independent verification.
    pub candidate: ModelRequest,
    /// Required if any history/context byte differs; not proof by itself.
    pub checkpoint: Option<CompactionCheckpoint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum ModelPreparationProblem {
    SourceChanged,
    IdentityChanged,
    CapabilitiesChanged,
    MissingCheckpoint,
    InvalidCheckpoint,
    ScopeDenied,
    ProviderFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ModelPreparationError {
    pub problem: ModelPreparationProblem,
    pub message: String,
}

/// Selected Advanced preparation is an independently authorized plugin
/// operation. This contract never grants a Model call or checkpoint write.
#[phenix_sdk::contract(id = "phenix.advanced.model-preparation@1")]
pub trait ModelPreparation {
    fn prepare(
        &self,
        input: ModelPreparationInput,
    ) -> Result<ModelPreparationProposal, ModelPreparationError>;
}

/// State-change handler for an independently contributed graph preparation
/// node. Its request schema is exactly the M1 kernel state-change schema.
#[phenix_sdk::contract(id = "phenix.advanced.model-preparation-change@1")]
pub trait ModelPreparationChange {
    fn run(
        &self,
        notification: phenix_basic_contracts::GraphStateChange,
    ) -> Result<String, ModelPreparationError>;
}

/// Structural fail-closed check. This does NOT authorize a graph transition.
/// The caller must also verify live session ownership, original journal
/// bytes, checkpoint CAS/readback and exact final token measurement before
/// allowing a replacement through Model::complete.
pub fn validate_model_preparation_input(
    input: &ModelPreparationInput,
) -> Result<(), ModelPreparationProblem> {
    if input.original.session.is_empty()
        || input.original.run.is_empty()
        || model_request_sha256(&input.original)
            .map_err(|_| ModelPreparationProblem::SourceChanged)?
            != input.original_sha256
    {
        return Err(ModelPreparationProblem::SourceChanged);
    }
    Ok(())
}

pub fn validate_model_preparation_proposal(
    input: &ModelPreparationInput,
    proposal: &ModelPreparationProposal,
) -> Result<(), ModelPreparationProblem> {
    validate_model_preparation_input(input)?;
    if proposal.original_sha256 != input.original_sha256 {
        return Err(ModelPreparationProblem::SourceChanged);
    }
    let original = &input.original;
    let candidate = &proposal.candidate;
    if original.session != candidate.session || original.run != candidate.run {
        return Err(ModelPreparationProblem::IdentityChanged);
    }
    if original.tools != candidate.tools
        || original.skills != candidate.skills
        || original.limits != candidate.limits
    {
        return Err(ModelPreparationProblem::CapabilitiesChanged);
    }
    let changed = original.history != candidate.history || original.context != candidate.context;
    if changed && proposal.checkpoint.is_none() {
        return Err(ModelPreparationProblem::MissingCheckpoint);
    }
    if let Some(checkpoint) = &proposal.checkpoint {
        if checkpoint.session != original.session || validate_checkpoint(checkpoint).is_err() {
            return Err(ModelPreparationProblem::InvalidCheckpoint);
        }
    }
    Ok(())
}
