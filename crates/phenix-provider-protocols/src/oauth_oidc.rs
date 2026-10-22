//! Reusable OAuth/OIDC identity verification strategy.
//!
//! A provider supplies issuer, issued client ID, nonce, and a JWKS fetched
//! from its trusted, configured HTTPS origin. No provider-specific endpoint
//! knowledge, secret storage, model codec or kernel policy lives here.
//! Callback correlation and a token response are never identity evidence.

use openidconnect::core::{
    CoreIdToken, CoreIdTokenVerifier, CoreJsonWebKeySet, CoreJwsSigningAlgorithm,
};
use openidconnect::{ClientId, IssuerUrl, Nonce};
use serde_json::Value;

const MAX_JWKS_BODY: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OidcError {
    InvalidConfiguration,
    InvalidJwks,
    InvalidIdToken,
    IncorrectAccount,
}

#[derive(Debug, PartialEq, Eq)]
pub struct VerifiedSubject(String);

impl VerifiedSubject {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Reusable public-client OIDC signature, issuer, audience, expiry and nonce
/// gate. The caller must fetch JWKS from a trusted provider-declared endpoint
/// using TLS and no redirects: **no URL supplied by the JWT is consulted**.
/// This strategy does not decide access-token scope or account storage policy.
pub fn verify_signed_oidc_identity(
    jwt: &str,
    jwks_body: &[u8],
    issuer: &str,
    issued_client_id: &str,
    original_nonce: &str,
    expected_subject: Option<&str>,
) -> Result<VerifiedSubject, OidcError> {
    if jwt.is_empty()
        || jwt.len() > 16 * 1024
        || issued_client_id.is_empty()
        || issued_client_id.len() > 256
        || original_nonce.len() < 24
        || original_nonce.len() > 256
        || !issuer.starts_with("https://")
        || issuer.chars().any(char::is_control)
    {
        return Err(OidcError::InvalidConfiguration);
    }
    if jwks_body.is_empty() || jwks_body.len() > MAX_JWKS_BODY {
        return Err(OidcError::InvalidJwks);
    }
    let jwks: CoreJsonWebKeySet =
        serde_json::from_slice(jwks_body).map_err(|_| OidcError::InvalidJwks)?;
    let id_token: CoreIdToken = serde_json::from_value(Value::String(jwt.to_owned()))
        .map_err(|_| OidcError::InvalidIdToken)?;
    let verifier = CoreIdTokenVerifier::new_public_client(
        ClientId::new(issued_client_id.to_owned()),
        IssuerUrl::new(issuer.to_owned()).map_err(|_| OidcError::InvalidConfiguration)?,
        jwks,
    )
    .set_allowed_algs([CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256]);
    let claims = id_token
        .claims(&verifier, &Nonce::new(original_nonce.to_owned()))
        .map_err(|_| OidcError::InvalidIdToken)?;
    let subject = claims.subject().as_str();
    if subject.is_empty() || subject.len() > 256 {
        return Err(OidcError::InvalidIdToken);
    }
    if expected_subject.is_some_and(|current| current != subject) {
        return Err(OidcError::IncorrectAccount);
    }
    Ok(VerifiedSubject(subject.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_tokens_never_produce_verified_subjects() {
        for issuer in ["https://auth.example.test", "https://auth.openai.com"] {
            assert_eq!(
                verify_signed_oidc_identity(
                    "unsigned-or-forged-token",
                    br#"{"keys":[]}"#,
                    issuer,
                    "issued-client",
                    "randomnoncerandomnoncerandomnonce",
                    None,
                ),
                Err(OidcError::InvalidIdToken)
            );
        }
    }

    #[test]
    fn invalid_provider_verification_parameters_fail_before_jwt() {
        assert_eq!(
            verify_signed_oidc_identity(
                "jwt", br#"{"keys":[]}"#, "http://host.test",
                "issued-client", "randomnoncerandomnoncerandomnonce", None,
            ),
            Err(OidcError::InvalidConfiguration)
        );
        assert_eq!(
            verify_signed_oidc_identity(
                "jwt", b"not jwks", "https://issuer.example.test",
                "issued-client", "randomnoncerandomnoncerandomnonce", None,
            ),
            Err(OidcError::InvalidJwks)
        );
    }
}
