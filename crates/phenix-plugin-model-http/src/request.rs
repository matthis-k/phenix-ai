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

/// An absent setting permits a deliberately credential-free local provider.
/// An explicitly supplied but unusable value must never become no-auth.
fn configured_credential(
    value: Result<String, env::VarError>,
) -> Result<Option<String>, BasicError> {
    match value {
        Err(env::VarError::NotPresent) => Ok(None),
        Ok(secret) if !secret.is_empty() && !secret.chars().any(char::is_control) => {
            Ok(Some(secret))
        }
        _ => Err(error(ErrorCode::InvalidInput, "invalid HTTP credential")),
    }
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
        let provider_max_tokens = configured_max_tokens(env::var("PHENIX_MODEL_HTTP_MAX_TOKENS"))?;
        let endpoint = endpoint_url(&endpoint)?;
        if model.is_empty() {
            return Err(error(
                ErrorCode::InvalidInput,
                "invalid HTTP model configuration",
            ));
        }
        let credential = configured_credential(env::var("PHENIX_MODEL_HTTP_API_KEY"))?;
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

#[cfg(test)]
mod credential_tests {
    use super::*;

    #[test]
    fn explicit_http_credential_errors_never_fall_back_to_no_auth() {
        assert_eq!(
            configured_credential(Err(env::VarError::NotPresent)).unwrap(),
            None
        );
        assert_eq!(
            configured_credential(Ok("fixture-key".into())).unwrap(),
            Some("fixture-key".into())
        );
        for bad in ["", "\n", "key\r", "key\u{0000}"] {
            assert_eq!(
                configured_credential(Ok(bad.into())).unwrap_err().code,
                ErrorCode::InvalidInput,
            );
        }
        assert_eq!(
            configured_credential(Err(env::VarError::NotUnicode(std::ffi::OsString::from(
                "not-unicode"
            ),)))
            .unwrap_err()
            .code,
            ErrorCode::InvalidInput,
        );
    }
}
