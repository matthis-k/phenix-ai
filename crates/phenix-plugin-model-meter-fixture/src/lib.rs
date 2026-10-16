//! Verification-only selected Model and preflight meter.
//!
//! This mock model consumes the canonical Portable ModelRequest bytes as its
//! complete prompt. Its *defined* tokenizer maps one byte to one token.
//! Production providers must instead tokenize their actual model-specific
//! wire template, tokenizer and selected configuration. Never use this mock
//! byte count for HTTP or production model admission.
use phenix_advanced_contracts::{
    model_request_sha256, ModelTokenError, ModelTokenMeasurement, ModelTokenProblem,
};
use phenix_basic_contracts::{error, BasicError, ErrorCode, ModelReply, ModelRequest, Role, Usage};
use phenix_sdk::Portable;

pub const MODEL_ID: &str = "phenix.fixture.portable-byte-model@1";
const CONTEXT_WINDOW: u64 = 65_536;

fn failed() -> ModelTokenError {
    ModelTokenError {
        problem: ModelTokenProblem::ProviderFailed,
        message: "fixture request could not be serialized".into(),
    }
}

pub fn measure(request: &ModelRequest) -> Result<ModelTokenMeasurement, ModelTokenError> {
    let encoded = request.to_value().encode().map_err(|_| failed())?;
    let input_tokens = u64::try_from(encoded.len()).map_err(|_| failed())?;
    let mut fingerprint = model_request_sha256(request).map_err(|_| failed())?;
    // Fault injection only in this non-production fixture. Model::complete
    // still returns an answer, so rejection proves the adapter ran preflight.
    if request
        .history
        .last()
        .is_some_and(|message| message.text == "meter-stale")
    {
        fingerprint = "0".repeat(64);
    }
    // P2-3x verification-only faults: the selected adapter must meter the
    // original authoritative request as well as its compacted successor.
    // Only the compacted request carries the checkpoint context citation.
    let prompt = request
        .history
        .iter()
        .rev()
        .find(|message| message.role == Role::User)
        .map(|message| message.text.as_str());
    let compacted = request
        .context
        .iter()
        .any(|item| item.id.starts_with("compaction:"));
    if prompt == Some("inspect-original-stale") && !compacted {
        fingerprint = "0".repeat(64);
    }
    let tokenizer_revision =
        if prompt == Some("inspect-config-drift") && compacted {
            "portable-byte-fixture-v2"
        } else {
            "portable-byte-fixture-v1"
        };
    Ok(ModelTokenMeasurement {
        model: MODEL_ID.into(),
        tokenizer_revision: tokenizer_revision.into(),
        request_sha256: fingerprint,
        input_tokens,
        context_window_tokens: CONTEXT_WINDOW,
    })
}

pub fn complete(request: &ModelRequest) -> Result<ModelReply, BasicError> {
    let prompt = request
        .history
        .iter()
        .rev()
        .find(|message| message.role == Role::User)
        .ok_or_else(|| error(ErrorCode::InvalidInput, "missing user message"))?;
    let count = measure(request)
        .map_err(|_| error(ErrorCode::ModelFailed, "fixture measurement failed"))?;
    // Native acceptance probes the exact request that survived source
    // verification and selected-provider metering. This branch exists only
    // in the verification Model, never in production Model adapters.
    let text = if prompt.text == "inspect-compaction" {
        let cited = request
            .context
            .iter()
            .any(|part| part.id == "compaction:cp-live");
        format!(
            "fixture:history:{}:compaction:{}",
            request.history.len(),
            cited,
        )
    } else {
        format!("fixture:{}", prompt.text)
    };
    Ok(ModelReply {
        usage: Usage {
            input_tokens: count.input_tokens,
            output_tokens: u64::try_from(text.len()).unwrap_or(u64::MAX),
        },
        text,
        calls: vec![],
    })
}

#[phenix_sdk::plugin(
    id = "phenix.model.meter-fixture",
    version = "1.0.0",
    provide(phenix_basic_contracts::Model, slot = "Model"),
    provide(phenix_advanced_contracts::ModelTokenMeter, slot = "Meter")
)]
mod plugin {
    use phenix_advanced_contracts::{ModelTokenError, ModelTokenMeasurement, ModelTokenMeter};
    use phenix_basic_contracts::{BasicError, Model, ModelReply, ModelRequest};
    use phenix_sdk::{CallError, CallResult, Host};

    #[derive(Default)]
    pub struct State;

    #[provide(ModelTokenMeter::measure)]
    fn measure(
        _: &mut State,
        host: &Host<'_>,
        request: ModelRequest,
    ) -> CallResult<ModelTokenMeasurement, ModelTokenError> {
        phenix_basic_contracts::check_session(host, &request.session).map_err(|failure| {
            match failure {
                CallError::Kernel(kernel) => CallError::Kernel(kernel),
                CallError::Domain(_) => CallError::Domain(ModelTokenError {
                    problem: phenix_advanced_contracts::ModelTokenProblem::ScopeDenied,
                    message: "meter session scope denied".into(),
                }),
            }
        })?;
        super::measure(&request).map_err(CallError::Domain)
    }

    #[provide(Model::complete)]
    fn complete(
        _: &mut State,
        host: &Host<'_>,
        request: ModelRequest,
    ) -> CallResult<ModelReply, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        super::complete(&request).map_err(CallError::Domain)
    }
}
