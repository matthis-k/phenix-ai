//! Provider-neutral OAuth authorization-code browser attempt mechanism.
//!
//! Bind a local callback listener before presenting an authorization URL,
//! generate fresh PKCE/state/nonce for that attempt, and expose a one-shot
//! callback for the provider's registered issuer adapter to verify. This
//! mechanism never treats a received browser redirect as authentication.
use phenix_provider_protocols::{
    oauth_loopback::{CallbackError, CallbackRequest, OAuthLoopbackListener},
    oauth_pkce::GeneratedPkce,
};
use std::time::Duration;

/// Private provider-owned browser attempt; contains live socket and PKCE
/// verifier and intentionally cannot be cloned, debugged or serialized.
pub struct BrowserAttempt {
    listener: OAuthLoopbackListener,
    pkce: GeneratedPkce,
}

impl BrowserAttempt {
    pub fn new(ttl: Duration) -> Result<Self, CallbackError> {
        // Bind before returning any browser URL, so callbacks cannot race an
        // uninitialized listener. Invalid TTL and bind failures fail closed.
        let listener = OAuthLoopbackListener::bind(ttl)?;
        Ok(Self {
            listener,
            pkce: GeneratedPkce::new(),
        })
    }

    pub fn redirect_uri(&self) -> String {
        self.listener.redirect_uri()
    }

    pub fn state(&self) -> &str {
        self.pkce.state()
    }

    pub fn nonce(&self) -> &str {
        self.pkce.nonce()
    }

    pub fn challenge(&self) -> &str {
        self.pkce.challenge()
    }

    /// Secret PKCE verifier: pass only to an issuer-authored token exchange.
    /// Never return it through frontend events or provider catalog metadata.
    pub fn verifier(&self) -> &str {
        self.pkce.verifier()
    }

    pub fn poll(&mut self) -> Result<Option<CallbackRequest>, CallbackError> {
        self.listener.poll()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_browser_attempts_prebind_one_shot_loopback() {
        let mut first = BrowserAttempt::new(Duration::from_secs(30)).unwrap();
        let second = BrowserAttempt::new(Duration::from_secs(30)).unwrap();
        assert!(first.redirect_uri().starts_with("http://127.0.0.1:"));
        assert!(first.redirect_uri().ends_with("/auth/callback"));
        assert_ne!(first.redirect_uri(), second.redirect_uri());
        assert_ne!(first.state(), second.state());
        assert_ne!(first.nonce(), second.nonce());
        assert_ne!(first.verifier(), second.verifier());
        assert!(first.poll().unwrap().is_none());
    }

    #[test]
    fn invalid_ttl_does_not_initiate_browser_authorization() {
        assert!(matches!(
            BrowserAttempt::new(Duration::ZERO),
            Err(CallbackError::Bind)
        ));
        assert!(matches!(
            BrowserAttempt::new(Duration::from_secs(601)),
            Err(CallbackError::Bind)
        ));
    }
}
