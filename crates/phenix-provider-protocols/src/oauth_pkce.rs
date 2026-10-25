//! Shared OAuth 2.0 Authorization Code + PKCE proof generation.
//!
//! OAuth issuers select their own endpoints, scopes, client registration and
//! token/identity verification policies. PKCE, state and nonce generation are
//! not provider-specific behavior.
use openidconnect::{CsrfToken, Nonce, PkceCodeChallenge};
use zeroize::Zeroize;

/// The caller retains the freshly generated verifier only for this attempt.
/// Deliberately not Debug/Clone: the verifier belongs only to one attempt,
/// never an auth URL, a model prompt or a session journal.
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_attempt_has_independent_state_nonce_and_pkce_verifier() {
        let first = GeneratedPkce::new();
        let second = GeneratedPkce::new();
        assert_ne!(first.state(), second.state());
        assert_ne!(first.nonce(), second.nonce());
        assert_ne!(first.verifier(), second.verifier());
        assert!(first.state().len() >= 24);
        assert!(first.nonce().len() >= 24);
        assert!((43..=128).contains(&first.verifier().len()));
        assert_ne!(first.challenge(), first.verifier());
    }
}
