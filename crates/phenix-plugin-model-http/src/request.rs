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
    pub credential: Option<phenix_provider_contracts::CredentialSecret>,
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
pub(crate) fn configured_credential(
    value: Result<String, env::VarError>,
) -> Result<Option<phenix_provider_contracts::CredentialSecret>, BasicError> {
    match value {
        Err(env::VarError::NotPresent) => Ok(None),
        Ok(secret) => validated_credential(secret.into_bytes()).map(Some),
        Err(_) => Err(error(ErrorCode::InvalidInput, "invalid HTTP credential")),
    }
}

/// Keep both explicit checked and legacy preset credentials in the canonical
/// clearing secret type. Reject malformed text without retaining its bytes.
fn validated_credential(
    mut bytes: Vec<u8>,
) -> Result<phenix_provider_contracts::CredentialSecret, BasicError> {
    let valid = std::str::from_utf8(&bytes)
        .ok()
        .is_some_and(|text| !text.is_empty() && !text.chars().any(char::is_control));
    if !valid {
        bytes.fill(0);
        return Err(error(ErrorCode::InvalidInput, "invalid HTTP credential"));
    }
    phenix_provider_contracts::CredentialSecret::new(bytes)
        .map_err(|_| error(ErrorCode::InvalidInput, "invalid HTTP credential"))
}

// The checked path is consumed once the controller resolves the selected
// credential root; the legacy HTTP environment preset remains a separate
// explicit composition mode until the management/Model handoff is installed.
impl Config {
    /// Validate one decoded private native install before it becomes the next
    /// model turn's configuration. A forged Portable payload cannot create
    /// an unsafe endpoint, blank model or malformed header value.
    pub(crate) fn from_install(
        install: phenix_provider_contracts::HttpModelInstall,
    ) -> Result<Self, BasicError> {
        use phenix_provider_contracts::HttpModelAuthorization;
        let (_, endpoint, model, protocol, provider_max_tokens, authentication) =
            install.into_parts();
        if model.trim().is_empty() || provider_max_tokens == Some(0) {
            return Err(error(
                ErrorCode::InvalidInput,
                "invalid selected HTTP model",
            ));
        }
        let endpoint = endpoint_url(&endpoint)?;
        let protocol = Protocol::parse(&protocol)?;
        let credential = match authentication {
            HttpModelAuthorization::CredentialFree => None,
            HttpModelAuthorization::ApiKey {
                identity: _,
                secret,
            }
            | HttpModelAuthorization::BearerToken {
                identity: _,
                secret,
            } => {
                let valid = std::str::from_utf8(secret.expose_for_authorized_provider())
                    .ok()
                    .is_some_and(|text| !text.is_empty() && !text.chars().any(char::is_control));
                if !valid {
                    return Err(error(ErrorCode::InvalidInput, "invalid HTTP credential"));
                }
                Some(secret)
            }
        };
        Ok(Self {
            endpoint,
            model,
            protocol,
            provider_max_tokens,
            credential,
            ca_file: None,
        })
    }

    #[allow(dead_code)]
    pub(crate) fn from_checked_binding(
        binding: &phenix_provider_contracts::HttpModelBinding,
        credential: Option<(
            &phenix_provider_contracts::CredentialIdentity,
            &phenix_provider_contracts::CredentialSecret,
        )>,
        provider_max_tokens: Option<u64>,
    ) -> Result<Self, BasicError> {
        // Provider-admitted metadata is not automatically valid transport
        // configuration. Reject invalid scalar values before secret handling.
        if binding.model_id().trim().is_empty() {
            return Err(error(
                ErrorCode::InvalidInput,
                "invalid selected HTTP model",
            ));
        }
        if provider_max_tokens == Some(0) {
            return Err(error(
                ErrorCode::InvalidInput,
                "invalid HTTP provider max tokens",
            ));
        }
        // The selected provider must already have verified this account
        // and admitted the specific authentication method. Native transport
        // accepts API keys or current OAuth bearer tokens; plugin-specific
        // unknown modes remain unsupported.
        match (binding.credential_identity(), binding.authentication_kind()) {
            (None, None)
            | (Some(_), Some(phenix_provider_contracts::AuthenticationKind::ApiKey))
            | (Some(_), Some(phenix_provider_contracts::AuthenticationKind::OAuthAuthorizationCode))
            | (Some(_), Some(phenix_provider_contracts::AuthenticationKind::OAuthDeviceCode)) => {}
            _ => {
                return Err(error(
                    ErrorCode::InvalidInput,
                    "selected HTTP authentication method is unsupported",
                ));
            }
        }
        match (binding.credential_identity(), credential.as_ref()) {
            (None, None) => {}
            (Some(expected), Some((actual, _))) if expected == *actual => {}
            _ => {
                return Err(error(
                    ErrorCode::InvalidInput,
                    "credential identity does not match selected provider connection and profile",
                ));
            }
        }
        let protocol = Protocol::parse(binding.protocol_id())?;
        let endpoint = endpoint_url(binding.endpoint())?;
        let secret = credential
            .map(|(_, key)| validated_credential(key.expose_for_authorized_provider().to_vec()))
            .transpose()?;
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

impl std::fmt::Debug for Config {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Config")
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field("protocol", &self.protocol)
            .field("provider_max_tokens", &self.provider_max_tokens)
            .field(
                "credential",
                &self.credential.as_ref().map(|_| "[REDACTED]"),
            )
            .field("ca_file", &self.ca_file)
            .finish()
    }
}
