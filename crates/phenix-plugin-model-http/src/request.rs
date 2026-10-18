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

impl Config {
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
            credential,
            ca_file: env::var("PHENIX_MODEL_HTTP_CA_FILE").ok(),
        })
    }
}
