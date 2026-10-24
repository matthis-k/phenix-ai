//! SIWC cryptographic identity gate, separate from registration and UI.
//!
//! Validating the callback state or receiving a token response is not login.
//! Only a signature/issuer/audience/expiry/nonce checked ID token *and* an
//! independently granted plan scope produce a VerifiedChatGptIdentity.
//! This module contains no HTTP client and cannot download attacker-supplied
//! JWKS URLs. The provider plugin must fetch the fixed issuer's public JWKS,
//! perform strict TLS/no-redirect token exchange and authorize secure storage.

use crate::oauth_oidc::{
    verify_refreshed_signed_oidc_identity, verify_signed_oidc_identity, OidcError, VerifiedSubject,
};
use crate::siwc::require_token_granted_plan_scope;
use openidconnect::{CsrfToken, Nonce, PkceCodeChallenge};
use serde_json::Value;
use zeroize::Zeroize;

pub const ISSUER: &str = "https://auth.openai.com";
pub const JWKS_URL: &str = "https://auth.openai.com/.well-known/jwks.json";
pub const MAX_TOKEN_BODY: usize = 64 * 1024;
pub const MAX_JWKS_BODY: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationError {
    InvalidTokenResponse,
    InvalidJwks,
    InvalidIdToken,
    MissingPlanPermission,
    IncorrectAccount,
}

/// The caller retains the freshly generated verifier only for this attempt.
/// Deliberately not Debug/Clone; in particular, never log a future reauth URL
/// containing an id_token_hint, nor persist this verifier in a session journal.
pub struct GeneratedPkce {
    state: String,
    nonce: String,
    challenge: String,
    verifier: String,
}

impl GeneratedPkce {
    pub fn new() -> Self {
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        // Two independent 128-bit values avoid any dependence on a short
        // random token representation and meet the registration input limits.
        let state = format!(
            "{}{}",
            CsrfToken::new_random().secret(),
            CsrfToken::new_random().secret()
        );
        let nonce = format!(
            "{}{}",
            Nonce::new_random().secret(),
            Nonce::new_random().secret()
        );
        Self {
            state,
            nonce,
            challenge: challenge.as_str().to_owned(),
            verifier: verifier.secret().to_owned(),
        }
    }

    pub fn state(&self) -> &str {
        &self.state
    }
    pub fn nonce(&self) -> &str {
        &self.nonce
    }
    pub fn challenge(&self) -> &str {
        &self.challenge
    }
    pub fn verifier(&self) -> &str {
        &self.verifier
    }
}

impl Default for GeneratedPkce {
    fn default() -> Self {
        Self::new()
    }
}
impl Drop for GeneratedPkce {
    fn drop(&mut self) {
        self.verifier.zeroize();
        self.nonce.zeroize();
        self.state.zeroize();
    }
}

/// Raw token material has no Debug/Clone/serialization. A refresh token is
/// storage-domain only and MUST NOT be handed to HTTP model inference.
pub struct TokenEnvelope {
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: String,
    pub expires_in: u64,
    pub granted_scope: String,
}

impl TokenEnvelope {
    pub fn parse(body: &[u8]) -> Result<Self, VerificationError> {
        if body.is_empty() || body.len() > MAX_TOKEN_BODY {
            return Err(VerificationError::InvalidTokenResponse);
        }
        let json: Value =
            serde_json::from_slice(body).map_err(|_| VerificationError::InvalidTokenResponse)?;
        let field = |name| {
            json.get(name)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty() && s.len() <= 16 * 1024)
                .map(str::to_owned)
                .ok_or(VerificationError::InvalidTokenResponse)
        };
        if !json
            .get("token_type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind.eq_ignore_ascii_case("bearer"))
        {
            return Err(VerificationError::InvalidTokenResponse);
        }
        let expires_in = json
            .get("expires_in")
            .and_then(Value::as_u64)
            .filter(|seconds| *seconds > 0 && *seconds <= 60 * 60 * 24 * 90)
            .ok_or(VerificationError::InvalidTokenResponse)?;
        Ok(Self {
            access_token: field("access_token")?,
            refresh_token: field("refresh_token")?,
            id_token: field("id_token")?,
            expires_in,
            granted_scope: field("scope")?,
        })
    }
}
impl Drop for TokenEnvelope {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
        self.id_token.zeroize();
    }
}

/// A verified OIDC subject witness, not caller-provided profile metadata.
/// Only the signature/issuer/audience/nonce verifier can construct this
/// identity; the browser's verified completion consumes this witness after
/// the concrete provider has securely persisted the authorized account.
#[derive(Debug, PartialEq, Eq)]
pub struct VerifiedChatGptIdentity {
    subject: VerifiedSubject,
    issued_client_id: String,
}

impl VerifiedChatGptIdentity {
    pub fn subject(&self) -> &str {
        self.subject.as_str()
    }

    pub fn issued_client_id(&self) -> &str {
        &self.issued_client_id
    }

    /// Passed to OAuthLoopback::CallbackRequest::confirm_verified only after
    /// the verified account and current credentials have been stored.
    pub fn verified_subject(&self) -> &VerifiedSubject {
        &self.subject
    }
}

/// This is the only entrance to the authenticated-account state. A caller
/// cannot supply a subject, email, or an unverified claims JSON directly.
/// JWKS must be fetched from JWKS_URL over pinned HTTPS with no redirects.
/// The OIDC library performs RS256 verification and enforces issuer,
/// audience, expiry and the original nonce.
pub fn verify_chatgpt_identity(
    envelope: &TokenEnvelope,
    jwks_body: &[u8],
    issued_client_id: &str,
    original_nonce: &str,
    expected_subject: Option<&str>,
) -> Result<VerifiedChatGptIdentity, VerificationError> {
    verify_chatgpt_identity_with_mode(
        envelope,
        jwks_body,
        issued_client_id,
        original_nonce,
        expected_subject,
        false,
    )
}

/// Validate a persisted login or a rotating refresh generation. Initial
/// browser exchanges MUST contain the nonce; refresh ID tokens MAY omit it
/// under OIDC Core 12.2. In either case, signature/issuer/audience/expiry,
/// original nonce when present, saved subject and the plan grant must hold.
pub fn verify_chatgpt_persisted_identity(
    envelope: &TokenEnvelope,
    jwks_body: &[u8],
    issued_client_id: &str,
    original_nonce: &str,
    expected_subject: &str,
) -> Result<VerifiedChatGptIdentity, VerificationError> {
    if expected_subject.is_empty() {
        return Err(VerificationError::IncorrectAccount);
    }
    verify_chatgpt_identity_with_mode(
        envelope,
        jwks_body,
        issued_client_id,
        original_nonce,
        Some(expected_subject),
        true,
    )
}

fn verify_chatgpt_identity_with_mode(
    envelope: &TokenEnvelope,
    jwks_body: &[u8],
    issued_client_id: &str,
    original_nonce: &str,
    expected_subject: Option<&str>,
    persisted: bool,
) -> Result<VerifiedChatGptIdentity, VerificationError> {
    if issued_client_id.is_empty()
        || issued_client_id == "dynamic_agent_client"
        || issued_client_id.len() > 256
        || original_nonce.len() < 24
        || original_nonce.len() > 256
    {
        return Err(VerificationError::InvalidIdToken);
    }
    require_token_granted_plan_scope(&envelope.granted_scope)
        .map_err(|_| VerificationError::MissingPlanPermission)?;
    if jwks_body.is_empty() || jwks_body.len() > MAX_JWKS_BODY {
        return Err(VerificationError::InvalidJwks);
    }
    let subject = if persisted {
        verify_refreshed_signed_oidc_identity(
            &envelope.id_token,
            jwks_body,
            ISSUER,
            issued_client_id,
            original_nonce,
            expected_subject.ok_or(VerificationError::IncorrectAccount)?,
        )
    } else {
        verify_signed_oidc_identity(
            &envelope.id_token,
            jwks_body,
            ISSUER,
            issued_client_id,
            original_nonce,
            expected_subject,
        )
    }
    .map_err(|problem| match problem {
        OidcError::InvalidJwks => VerificationError::InvalidJwks,
        OidcError::IncorrectAccount => VerificationError::IncorrectAccount,
        OidcError::InvalidConfiguration | OidcError::InvalidIdToken => {
            VerificationError::InvalidIdToken
        }
    })?;
    Ok(VerifiedChatGptIdentity {
        subject,
        issued_client_id: issued_client_id.to_owned(),
    })
}

pub fn refresh_form(
    issued_client_id: &str,
    refresh_token: &str,
) -> Result<String, VerificationError> {
    if issued_client_id.is_empty()
        || issued_client_id == "dynamic_agent_client"
        || refresh_token.is_empty()
        || refresh_token.len() > 16 * 1024
    {
        return Err(VerificationError::InvalidTokenResponse);
    }
    let mut form = url::form_urlencoded::Serializer::new(String::new());
    form.append_pair("grant_type", "refresh_token")
        .append_pair("client_id", issued_client_id)
        .append_pair("refresh_token", refresh_token)
        .append_pair("resource", crate::siwc::RESOURCE);
    Ok(form.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_attempt_has_independent_state_nonce_and_s256_challenge() {
        let a = GeneratedPkce::new();
        let b = GeneratedPkce::new();
        assert_ne!(a.state(), b.state());
        assert_ne!(a.nonce(), b.nonce());
        assert_ne!(a.state(), a.nonce());
        assert_eq!(a.challenge().len(), 43);
        assert!((43..=128).contains(&a.verifier().len()));
        assert!(!format!("{:?}", VerificationError::InvalidIdToken).contains(a.verifier()));
    }

    fn valid_token_response() -> &'static [u8] {
        br#"{"token_type":"Bearer","access_token":"fake-access","refresh_token":"fake-refresh","id_token":"not-a-signed-jwt","expires_in":3600,"scope":"openid profile offline_access resource.invoke chatgpt.tokens.use.direct"}"#
    }

    #[test]
    fn token_response_and_permissions_do_not_imply_oidc_login() {
        let token = TokenEnvelope::parse(valid_token_response()).unwrap();
        assert_eq!(
            verify_chatgpt_identity(
                &token,
                br#"{"keys":[]}"#,
                "oaiapp_issued",
                "validnoncerandomstatefromoauthprovider",
                None,
            ),
            Err(VerificationError::InvalidIdToken)
        );
        assert_eq!(
            verify_chatgpt_identity(
                &token,
                br#"{"keys":[]}"#,
                "dynamic_agent_client",
                "validnoncerandomstatefromoauthprovider",
                None,
            ),
            Err(VerificationError::InvalidIdToken)
        );
    }

    #[test]
    fn reject_missing_scope_expiry_and_wrong_token_type() {
        for body in [
            br#"{"token_type":"Basic","access_token":"a","refresh_token":"r","id_token":"i","expires_in":3600,"scope":"openid"}"#.as_slice(),
            br#"{"token_type":"Bearer","access_token":"a","refresh_token":"r","id_token":"i","expires_in":0,"scope":"openid"}"#,
            br#"{"token_type":"Bearer","access_token":"a","id_token":"i","expires_in":3600,"scope":"openid"}"#,
            br#"{"access_token":"a","refresh_token":"r","id_token":"i","expires_in":3600,"scope":"openid"}"#,
        ] {
            assert!(TokenEnvelope::parse(body).is_err());
        }
        let without_scope = TokenEnvelope::parse(
            br#"{"token_type":"Bearer","access_token":"a","refresh_token":"r","id_token":"i","expires_in":3600,"scope":"openid"}"#,
        ).unwrap();
        assert_eq!(
            verify_chatgpt_identity(
                &without_scope,
                br#"{"keys":[]}"#,
                "oaiapp_issued",
                "validnoncerandomstatefromoauthprovider",
                None,
            ),
            Err(VerificationError::MissingPlanPermission)
        );
    }

    #[test]
    fn refresh_uses_issued_client_without_expanding_scopes() {
        let form = refresh_form("oaiapp_issued", "private-rotation-token").unwrap();
        let fields: std::collections::BTreeMap<_, _> = url::form_urlencoded::parse(form.as_bytes())
            .into_owned()
            .collect();
        assert_eq!(fields["client_id"], "oaiapp_issued");
        assert_eq!(fields["grant_type"], "refresh_token");
        assert_eq!(fields["refresh_token"], "private-rotation-token");
        assert_eq!(fields["resource"], crate::siwc::RESOURCE);
        assert!(!fields.contains_key("scope"));
        assert!(refresh_form("dynamic_agent_client", "secret").is_err());
    }

    #[test]
    fn verified_identity_witness_matches_browser_completion_contract() {
        // This bridge compiles without constructing or forging a verified
        // subject. Providers may call it only after durable authorized
        // credential persistence and correct-account verification.
        fn finish_after_persistence(
            callback: crate::oauth_loopback::CallbackRequest,
            identity: &VerifiedChatGptIdentity,
        ) -> Result<(), crate::oauth_loopback::CallbackError> {
            callback.confirm_verified(identity.verified_subject())
        }
        let _typed_bridge: fn(
            crate::oauth_loopback::CallbackRequest,
            &VerifiedChatGptIdentity,
        ) -> Result<(), crate::oauth_loopback::CallbackError> = finish_after_persistence;
    }

    #[test]
    fn required_scope_constant_is_same_as_registration() {
        assert_eq!(crate::siwc::REQUIRED_SCOPE, "chatgpt.tokens.use.direct");
    }
}
