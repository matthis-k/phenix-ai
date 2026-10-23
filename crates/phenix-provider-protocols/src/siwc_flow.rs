//! One-shot Sign in with ChatGPT exchange and signed OIDC verification.
//!
//! This coordinator accepts only a state-correlated callback from the
//! provider's exact listener, sends an exchange form to the fixed token
//! endpoint, retrieves pinned issuer JWKS, and verifies scope + signed
//! issuer/audience/expiry/nonce/account before returning private tokens.
//!
//! A verified result is NOT yet durable authentication. The ProviderAuth
//! adapter must persist the tokens in a protected exact-account store and
//! make them usable by the selected Model before acknowledging the browser.
use crate::siwc::{RegistrationError, VerifiedCallback};
use crate::siwc_transport::{exchange_token, issuer_jwks, TransportError};
use crate::siwc_verified::{
    verify_chatgpt_identity, TokenEnvelope, VerificationError, VerifiedChatGptIdentity,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangeError {
    Callback(RegistrationError),
    Transport(TransportError),
    Verification(VerificationError),
}

/// Verified identity and still-private tokens. No Clone, Debug, Portable or
/// serialization; taking ownership is an explicit secure-storage handoff.
pub struct VerifiedTokenBundle {
    identity: VerifiedChatGptIdentity,
    tokens: TokenEnvelope,
}
impl VerifiedTokenBundle {
    pub fn identity(&self) -> &VerifiedChatGptIdentity {
        &self.identity
    }

    /// Consumed only by a provider with an approved credential store.
    /// Successful browser completion must follow this handoff, never precede it.
    pub fn into_parts(self) -> (VerifiedChatGptIdentity, TokenEnvelope) {
        (self.identity, self.tokens)
    }
}

/// Production exchange is pinned by siwc_transport; the editor and provider
/// descriptor cannot substitute a token or JWKS endpoint.
pub fn exchange_verified_callback(
    callback: VerifiedCallback,
    verifier: &str,
    original_nonce: &str,
    expected_subject: Option<&str>,
) -> Result<VerifiedTokenBundle, ExchangeError> {
    exchange_verified_with(
        callback,
        verifier,
        original_nonce,
        expected_subject,
        exchange_token,
        issuer_jwks,
    )
}

fn exchange_verified_with<E, J>(
    callback: VerifiedCallback,
    verifier: &str,
    original_nonce: &str,
    expected_subject: Option<&str>,
    exchange: E,
    jwks: J,
) -> Result<VerifiedTokenBundle, ExchangeError>
where
    E: FnOnce(&str) -> Result<TokenEnvelope, TransportError>,
    J: FnOnce() -> Result<Vec<u8>, TransportError>,
{
    let issued_client_id = callback.issued_client_id().to_owned();
    let form = callback
        .exchange_form(verifier)
        .map_err(ExchangeError::Callback)?;
    let tokens = exchange(&form).map_err(ExchangeError::Transport)?;
    // Never trust the ID token to provide a JWKS location. This must remain
    // the pinned issuer transport; token success without signed identity
    // and the granted plan scope is not authentication.
    let keys = jwks().map_err(ExchangeError::Transport)?;
    let identity = verify_chatgpt_identity(
        &tokens,
        &keys,
        &issued_client_id,
        original_nonce,
        expected_subject,
    )
    .map_err(ExchangeError::Verification)?;
    Ok(VerifiedTokenBundle { identity, tokens })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::siwc::{AuthorizationAttempt, Registration};

    const HOST: &str = "urn:uuid:123e4567-e89b-12d3-a456-426614174000";
    const CALLBACK: &str = "http://127.0.0.1:1455/auth/callback";
    const STATE: &str = "abcdefghijklmnopqrstuvwxyz0123456789";
    const NONCE: &str = "zyxwvutsrqponmlkjihgfedcba9876543210";
    const VERIFIER: &str = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGH";
    const FAKE_TOKEN: &[u8] = br#"{"token_type":"Bearer","access_token":"fake-access","refresh_token":"fake-refresh","id_token":"not-a-signed-jwt","expires_in":3600,"scope":"openid profile offline_access resource.invoke chatgpt.tokens.use.direct"}"#;

    fn verified_state_callback() -> VerifiedCallback {
        let challenge = "a".repeat(43);
        let attempt = AuthorizationAttempt {
            registration: Registration::First { app_name: "Phenix" },
            host_id: HOST,
            redirect_uri: CALLBACK,
            state: STATE,
            nonce: NONCE,
            pkce_challenge: &challenge,
        };
        attempt
            .accept_callback(&format!(
                "{CALLBACK}?state={STATE}&code=short-lived-code&client_id=oaiapp_issued"
            ))
            .unwrap()
    }

    #[test]
    fn token_response_and_scope_without_signed_oidc_never_complete_login() {
        let mut exchanged = false;
        let failed = exchange_verified_with(
            verified_state_callback(),
            VERIFIER,
            NONCE,
            None,
            |form| {
                exchanged = true;
                assert!(form.contains("grant_type=authorization_code"));
                assert!(form.contains("code=short-lived-code"));
                assert!(form.contains("client_id=oaiapp_issued"));
                TokenEnvelope::parse(FAKE_TOKEN).map_err(|_| TransportError::InvalidTokenResponse)
            },
            || Ok(br#"{"keys":[]}"#.to_vec()),
        );
        assert!(exchanged);
        assert!(matches!(
            failed,
            Err(ExchangeError::Verification(VerificationError::InvalidIdToken))
        ));
    }

    #[test]
    fn rejected_token_exchange_never_fetches_jwks_or_yields_identity() {
        let mut fetched_jwks = false;
        let failed = exchange_verified_with(
            verified_state_callback(),
            VERIFIER,
            NONCE,
            None,
            |_form| Err(TransportError::Rejected),
            || {
                fetched_jwks = true;
                Ok(Vec::new())
            },
        );
        assert!(matches!(
            failed,
            Err(ExchangeError::Transport(TransportError::Rejected))
        ));
        assert!(!fetched_jwks);
    }

    #[test]
    fn invalid_pkce_verifier_never_sends_a_token_request() {
        let mut network = false;
        let failed = exchange_verified_with(
            verified_state_callback(),
            "bad-verifier",
            NONCE,
            None,
            |_form| {
                network = true;
                Err(TransportError::Rejected)
            },
            || Ok(Vec::new()),
        );
        assert!(matches!(failed, Err(ExchangeError::Callback(RegistrationError::InvalidAttempt))));
        assert!(!network);
    }
}
