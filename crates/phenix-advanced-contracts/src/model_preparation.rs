//! P2-3ah: source-bound graph preparation proposal, never an authority token.
//!
//! The existing M1 graph must authorize the selected provider and its actual
//! writes independently. A checkpoint citation is untrusted until exact
//! EventJournal source verification, CAS readback and selected Model metering.
use crate::{model_request_sha256, validate_checkpoint, CompactionCheckpoint};
use phenix_basic_contracts::ModelRequest;
use phenix_sdk::{Code, Failure, Portable, StaticField, StaticSchema, Value};
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
/// A structurally valid source receipt, not proof of source authenticity or
/// authority to invoke Model. Host admission must still verify the source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "UncheckedPreparationReceipt")]
pub struct PreparationReceipt {
    source_epoch: u64,
    request_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UncheckedPreparationReceipt {
    source_epoch: u64,
    request_sha256: String,
}

impl TryFrom<UncheckedPreparationReceipt> for PreparationReceipt {
    type Error = &'static str;

    fn try_from(value: UncheckedPreparationReceipt) -> Result<Self, Self::Error> {
        Self::new(value.source_epoch, value.request_sha256)
            .map_err(|_| "invalid preparation receipt")
    }
}

impl PreparationReceipt {
    pub fn new(
        source_epoch: u64,
        request_sha256: impl Into<String>,
    ) -> Result<Self, ModelPreparationProblem> {
        let request_sha256 = request_sha256.into();
        if source_epoch == 0
            || request_sha256.len() != 64
            || !request_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ModelPreparationProblem::SourceChanged);
        }
        Ok(Self {
            source_epoch,
            request_sha256,
        })
    }

    pub fn source_epoch(&self) -> u64 {
        self.source_epoch
    }

    pub fn request_sha256(&self) -> &str {
        &self.request_sha256
    }
}

impl Portable for PreparationReceipt {
    const SCHEMA: StaticSchema = StaticSchema::Struct(&[
        StaticField {
            name: "request_sha256",
            schema: StaticSchema::String,
        },
        StaticField {
            name: "source_epoch",
            schema: StaticSchema::U64,
        },
    ]);

    fn to_value(&self) -> Value {
        Value::map([
            ("request_sha256", self.request_sha256.to_value()),
            ("source_epoch", self.source_epoch.to_value()),
        ])
    }

    fn from_value(value: Value) -> Result<Self, Failure> {
        let Value::Map(mut fields) = value else {
            return Err(Failure::new(Code::SchemaRejected, "receipt must be a struct"));
        };
        let sha = fields
            .remove("request_sha256")
            .ok_or_else(|| Failure::new(Code::SchemaRejected, "missing request SHA"))
            .and_then(String::from_value)?;
        let epoch = fields
            .remove("source_epoch")
            .ok_or_else(|| Failure::new(Code::SchemaRejected, "missing source epoch"))
            .and_then(u64::from_value)?;
        if !fields.is_empty() {
            return Err(Failure::new(Code::SchemaRejected, "unknown receipt field"));
        }
        Self::new(epoch, sha)
            .map_err(|_| Failure::new(Code::SchemaRejected, "invalid preparation receipt"))
    }
}

#[phenix_sdk::contract(id = "phenix.advanced.model-preparation-change@1")]
pub trait ModelPreparationChange {
    fn run(
        &self,
        notification: phenix_basic_contracts::GraphStateChange,
    ) -> Result<PreparationReceipt, ModelPreparationError>;
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

#[cfg(test)]
mod receipt_tests {
    use super::*;

    fn sha() -> String {
        "a".repeat(64)
    }

    #[test]
    fn receipt_requires_nonzero_epoch_and_canonical_sha256() {
        assert!(PreparationReceipt::new(0, sha()).is_err());
        for invalid in ["", "ABC", "f".repeat(63).as_str(), "G".repeat(64).as_str()] {
            assert!(PreparationReceipt::new(1, invalid).is_err());
        }
        let receipt = PreparationReceipt::new(42, sha()).unwrap();
        assert_eq!(receipt.source_epoch(), 42);
        assert_eq!(receipt.request_sha256(), sha());
        let encoded = receipt.to_value();
        assert!(PreparationReceipt::SCHEMA.owned().unwrap().accepts(&encoded));
        assert_eq!(PreparationReceipt::from_value(encoded).unwrap(), receipt);
    }

    #[test]
    fn no_invalid_receipt_survives_either_wire_decoder() {
        let bad = Value::map([
            ("request_sha256", Value::String(sha())),
            ("source_epoch", Value::U64(0)),
        ]);
        assert!(PreparationReceipt::from_value(bad).is_err());
        let extra = Value::map([
            ("request_sha256", Value::String(sha())),
            ("source_epoch", Value::U64(1)),
            ("authority", Value::Bool(true)),
        ]);
        assert!(PreparationReceipt::from_value(extra).is_err());
        assert!(serde_json::from_value::<PreparationReceipt>(serde_json::json!({
            "source_epoch": 0,
            "request_sha256": sha()
        }))
        .is_err());
        assert!(serde_json::from_value::<PreparationReceipt>(serde_json::json!({
            "source_epoch": 1,
            "request_sha256": sha(),
            "authority": true
        }))
        .is_err());
    }
}
