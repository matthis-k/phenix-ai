//! Preflight token measurement for the exact selected Model request.
//!
//! The Model provider owns its tokenizer and prompt serialization. The
//! measurement is a claim, not permission; the caller verifies the canonical
//! request fingerprint, selected model identity and reserved output budget.
use phenix_basic_contracts::ModelRequest;
use phenix_sdk::{Failure, Portable};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ModelTokenMeasurement {
    /// Identity of the selected, pinned Model provider and configuration.
    pub model: String,
    /// Tokenizer and template revision used for this model's complete request.
    pub tokenizer_revision: String,
    /// SHA-256 over the Portable encoding of the entire ModelRequest.
    pub request_sha256: String,
    pub input_tokens: u64,
    /// Full context limit, including reserved output.
    pub context_window_tokens: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum ModelTokenProblem {
    Unsupported,
    InvalidMeasurement,
    ModelChanged,
    RequestChanged,
    OverBudget,
    ScopeDenied,
    ProviderFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ModelTokenError {
    pub problem: ModelTokenProblem,
    pub message: String,
}

/// The implementation must render and tokenize the complete request using
/// the same model configuration it will use for Model::complete. It includes
/// history, tool calls and schemas, skills, context and provider templates.
/// A character/byte approximation is not valid for a production model.
///
/// Composition must bind this contract and Basic Model to the same selected
/// provider/configuration; a returned model name alone cannot authenticate it.
#[phenix_sdk::contract(id = "phenix.advanced.model-tokens@1")]
pub trait ModelTokenMeter {
    fn measure(&self, request: ModelRequest) -> Result<ModelTokenMeasurement, ModelTokenError>;
}

const REQUEST_HASH_DOMAIN: &[u8] = b"phenix.model-request.v1\0";

pub fn model_request_sha256(request: &ModelRequest) -> Result<String, Failure> {
    let bytes = request.to_value().encode()?;
    let mut hasher = Sha256::new();
    hasher.update(REQUEST_HASH_DOMAIN);
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

/// Deny changed requests, mismatched models and insufficient context budget.
/// The measured ModelRequest must be submitted unchanged after this check.
/// A token count of zero or an absent tokenizer revision is not evidence.
pub fn validate_model_token_measurement(
    request: &ModelRequest,
    measurement: &ModelTokenMeasurement,
    selected_model: &str,
    reserved_output_tokens: u64,
) -> Result<(), ModelTokenProblem> {
    if measurement.model.trim().is_empty()
        || measurement.tokenizer_revision.trim().is_empty()
        || measurement.input_tokens == 0
        || measurement.context_window_tokens == 0
        || measurement.request_sha256.len() != 64
        || !measurement
            .request_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ModelTokenProblem::InvalidMeasurement);
    }
    if selected_model.is_empty() || measurement.model != selected_model {
        return Err(ModelTokenProblem::ModelChanged);
    }
    if model_request_sha256(request).map_err(|_| ModelTokenProblem::InvalidMeasurement)?
        != measurement.request_sha256
    {
        return Err(ModelTokenProblem::RequestChanged);
    }
    if reserved_output_tokens == 0
        || measurement
            .input_tokens
            .checked_add(reserved_output_tokens)
            .is_none_or(|total| total > measurement.context_window_tokens)
    {
        return Err(ModelTokenProblem::OverBudget);
    }
    Ok(())
}
