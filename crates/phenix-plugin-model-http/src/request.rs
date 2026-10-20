use crate::failed;
use phenix_basic_contracts::{error, BasicError, ErrorCode};
use phenix_provider_protocols::Protocol;
use std::env;

#[cfg(test)]
pub(crate) use phenix_provider_protocols::request::encode;

pub(crate) struct Config {
    pub endpoint: String,
    pub model: String,
    pub protocol: Protocol,
    pub provider_max_tokens: Option<u64>,
    pub credential: Option<String>,
    pub ca_file: Option<String>,
}

pub(crate) fn endpoint_url(input: &str) -> Result<String, BasicError> {
    phenix_provider_protocols::validate_endpoint(
        input,
        phenix_provider_protocols::EndpointMode::LocalLoopback,
    )
    .map(|endpoint| endpoint.url)
    .map_err(|_| {
        error(
            ErrorCode::InvalidInput,
            "invalid or insecure HTTP model endpoint",
        )
    })
}

// Legacy HTTP preset only. Model-specific required output budgets belong to
// provider configuration, never to the kernel or generic caller Limits.
pub(crate) fn configured_max_tokens(
    value: Result<String, env::VarError>,
) -> Result<Option<u64>, BasicError> {
    match value {
        Err(env::VarError::NotPresent) => Ok(None),
        Ok(raw) => match raw.parse::<u64>() {
            Ok(tokens) if tokens > 0 => Ok(Some(tokens)),
            _ => Err(error(
                ErrorCode::InvalidInput,
                "invalid HTTP provider max tokens",
            )),
        },
        Err(_) => Err(error(
            ErrorCode::InvalidInput,
            "invalid HTTP provider max tokens",
        )),
    }
}

// The checked path is consumed once the controller resolves the selected
// credential root; the legacy HTTP environment preset remains a separate
// explicit composition mode until the management/Model handoff is installed.
impl Config {
    #[allow(dead_code)]
    pub(crate) fn from_checked_binding(
        binding: &phenix_provider_contracts::HttpModelBinding,
        credential: Option<&phenix_provider_contracts::CredentialSecret>,
        provider_max_tokens: Option<u64>,
    ) -> Result<Self, BasicError> {
        if binding.credential_identity().is_some() != credential.is_some() {
            return Err(error(
                ErrorCode::InvalidInput,
                "selected connection credential context is missing or unexpected",
            ));
        }
        let protocol = Protocol::parse(binding.protocol_id())?;
        let endpoint = endpoint_url(binding.endpoint())?;
        let secret = credential
            .map(|key| {
                std::str::from_utf8(key.expose_for_authorized_provider())
                    .map(str::to_owned)
                    .map_err(|_| error(ErrorCode::InvalidInput, "invalid HTTP credential"))
            })
            .transpose()?;
        if secret
            .as_deref()
            .is_some_and(|value| value.is_empty() || value.chars().any(char::is_control))
        {
            return Err(error(ErrorCode::InvalidInput, "invalid HTTP credential"));
        }
        Ok(Self {
            endpoint,
            model: binding.model_id().into(),
            protocol,
            provider_max_tokens,
            credential: secret,
            ca_file: None,
        })
    }

    pub fn load() -> Result<Self, BasicError> {
        let endpoint = env::var("PHENIX_MODEL_HTTP_ENDPOINT")
            .map_err(|_| failed("HTTP model endpoint is not configured"))?;
        let model = env::var("PHENIX_MODEL_HTTP_MODEL")
            .map_err(|_| failed("HTTP model identity is not configured"))?;
        let protocol = match env::var("PHENIX_MODEL_HTTP_PROTOCOL") {
            Ok(value) => Protocol::parse(&value)?,
            Err(env::VarError::NotPresent) => Protocol::OpenaiChat,
            Err(_) => return Err(error(ErrorCode::InvalidInput, "invalid HTTP protocol")),
        };
        let provider_max_tokens = configured_max_tokens(env::var("PHENIX_MODEL_HTTP_MAX_TOKENS"))?;
        let endpoint = endpoint_url(&endpoint)?;
        if model.is_empty() {
            return Err(error(
                ErrorCode::InvalidInput,
                "invalid HTTP model configuration",
            ));
        }
        let credential = env::var("PHENIX_MODEL_HTTP_API_KEY").ok();
        if credential
            .as_ref()
            .is_some_and(|s| s.chars().any(char::is_control))
        {
            return Err(error(ErrorCode::InvalidInput, "invalid HTTP credential"));
        }
        Ok(Self {
            endpoint,
            model,
            protocol,
            provider_max_tokens,
            credential,
            ca_file: env::var("PHENIX_MODEL_HTTP_CA_FILE").ok(),
        })
    }
}

impl std::fmt::Debug for Config {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Config")
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field("protocol", &self.protocol)
            .field("provider_max_tokens", &self.provider_max_tokens)
            .field("credential", &self.credential.as_ref().map(|_| "[REDACTED]"))
            .field("ca_file", &self.ca_file)
            .finish()
    }
}
