//! Private, non-journaled native handoff into the selected HTTP Model plugin.
//! A request is not authorization: the controller must revalidate provider
//! status/catalog, resolve one exact credential root and hold an explicit
//! session-scoped native install grant.
use super::{
    AuthenticationKind, CredentialIdentity, CredentialSecret, HttpModelBinding, ProviderError,
    ProviderErrorCode,
};
use phenix_sdk::{contract, Portable};

/// Exactly one selected HTTP authentication mode; not an ACP or config value.
#[derive(Debug, Portable)]
pub enum HttpModelAuthorization {
    CredentialFree,
    ApiKey {
        identity: CredentialIdentity,
        secret: CredentialSecret,
    },
}

/// One-turn native install. The Rust constructor is checked; the receiving
/// plugin must validate decoded/untrusted Portable input and host resource.
/// No serde, Clone or plain secret-bearing configuration is provided.
#[derive(Debug, Portable)]
pub struct HttpModelInstall {
    session: String,
    endpoint: String,
    model_id: String,
    protocol_id: String,
    provider_max_tokens: Option<u64>,
    authorization: HttpModelAuthorization,
}

impl HttpModelInstall {
    pub fn from_checked(
        binding: &HttpModelBinding,
        session: String,
        credential: Option<(CredentialIdentity, CredentialSecret)>,
        provider_max_tokens: Option<u64>,
    ) -> Result<Self, ProviderError> {
        let problem = |code, message: &str| ProviderError {
            code,
            message: message.into(),
        };
        if session.trim().is_empty()
            || binding.model_id().trim().is_empty()
            || provider_max_tokens == Some(0)
        {
            return Err(problem(ProviderErrorCode::InvalidInput, "invalid selected model install"));
        }
        let authorization = match (
            binding.credential_identity(),
            binding.authentication_kind(),
            credential,
        ) {
            (None, None, None) => HttpModelAuthorization::CredentialFree,
            (Some(expected), Some(AuthenticationKind::ApiKey), Some((actual, secret)))
                if *expected == actual =>
            {
                HttpModelAuthorization::ApiKey {
                    identity: actual,
                    secret,
                }
            }
            _ => return Err(problem(ProviderErrorCode::Denied, "selected model credentials denied")),
        };
        Ok(Self {
            session,
            endpoint: binding.endpoint().into(),
            model_id: binding.model_id().into(),
            protocol_id: binding.protocol_id().into(),
            provider_max_tokens,
            authorization,
        })
    }

    /// Consumed once by the native HTTP Model provider. Decoding a Portable
    /// value does not skip host scope, endpoint, protocol or secret validation.
    pub fn into_parts(
        self,
    ) -> (String, String, String, String, Option<u64>, HttpModelAuthorization) {
        (
            self.session,
            self.endpoint,
            self.model_id,
            self.protocol_id,
            self.provider_max_tokens,
            self.authorization,
        )
    }

    pub fn session(&self) -> &str {
        &self.session
    }
}

/// Controller-only native install, separate from journaled Model::complete.
/// A launcher's explicit session-specific root grant is still required.
#[contract(id = "phenix.provider.http-model-session@1")]
pub trait HttpModelSession {
    fn install(&self, request: HttpModelInstall) -> Result<(), ProviderError>;
}
