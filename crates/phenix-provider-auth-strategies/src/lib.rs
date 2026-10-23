//! Reusable API-key authentication attempt lifecycle.
//!
//! Concrete providers own credential verification and the approved credential
//! store. This strategy tracks attempts without holding or serializing secrets.
use phenix_provider_contracts::{
    AuthAttemptState, AuthChallenge, AuthenticatedProfile, CredentialIdentity, ProviderError,
    ProviderErrorCode,
};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

const MAX_PENDING: usize = 16;
const ATTEMPT_TTL: Duration = Duration::from_secs(600);

#[derive(Debug)]
struct Attempt {
    owner: CredentialIdentity,
    started: Instant,
}

/// A provider-local lifecycle strategy. Neither IDs nor challenge payloads
/// grant account access; callers still authorize and verify the chosen profile.
#[derive(Debug, Default)]
pub struct ApiKeyAttempts {
    next: u64,
    pending: BTreeMap<String, Attempt>,
}

fn error(code: ProviderErrorCode, message: &str) -> ProviderError {
    ProviderError {
        code,
        message: message.into(),
    }
}

impl ApiKeyAttempts {
    /// Start a masked API-key input flow for the exact authorized account.
    /// The actual key is submitted through the separately authorized store.
    pub fn begin(
        &mut self,
        owner: CredentialIdentity,
        prefix: &str,
        prompt: &str,
    ) -> Result<AuthChallenge, ProviderError> {
        if prefix.is_empty()
            || prefix.len() > 64
            || !prefix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || prompt.is_empty()
            || prompt.len() > 1024
        {
            return Err(error(
                ProviderErrorCode::InvalidInput,
                "invalid API-key challenge",
            ));
        }
        // An abandoned attempt cannot permanently exhaust the provider's
        // bounded input budget. Expired IDs are never revived.
        self.pending
            .retain(|_, item| item.started.elapsed() <= ATTEMPT_TTL);
        if self.pending.len() >= MAX_PENDING {
            return Err(error(
                ProviderErrorCode::Unavailable,
                "too many open authentication attempts",
            ));
        }
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| error(ProviderErrorCode::Unavailable, "auth attempt overflow"))?;
        let attempt_id = format!("{prefix}-{}", self.next);
        self.pending.insert(
            attempt_id.clone(),
            Attempt {
                owner,
                started: Instant::now(),
            },
        );
        Ok(AuthChallenge::ApiKeyInput {
            attempt_id,
            prompt: prompt.into(),
        })
    }

    /// Check expiry and account binding before a provider verifies credentials.
    /// `None` means the attempt is pending; missing and foreign IDs share a
    /// NotFound outcome to avoid disclosing account existence.
    pub fn check(
        &mut self,
        owner: &CredentialIdentity,
        id: &str,
    ) -> Result<Option<AuthAttemptState>, ProviderError> {
        let item = self
            .pending
            .get(id)
            .filter(|item| item.owner == *owner)
            .ok_or_else(|| error(ProviderErrorCode::NotFound, "auth attempt not found"))?;
        if item.started.elapsed() > ATTEMPT_TTL {
            self.pending.remove(id);
            return Ok(Some(AuthAttemptState::Expired));
        }
        Ok(None)
    }

    /// Complete only after the concrete provider has verified the account.
    pub fn complete(
        &mut self,
        owner: &CredentialIdentity,
        id: &str,
        profile: AuthenticatedProfile,
    ) -> Result<AuthAttemptState, ProviderError> {
        if let Some(expired) = self.check(owner, id)? {
            return Ok(expired);
        }
        self.pending.remove(id);
        Ok(AuthAttemptState::Completed { profile })
    }

    pub fn cancel(
        &mut self,
        owner: &CredentialIdentity,
        id: &str,
    ) -> Result<AuthAttemptState, ProviderError> {
        if let Some(expired) = self.check(owner, id)? {
            return Ok(expired);
        }
        self.pending.remove(id);
        Ok(AuthAttemptState::Cancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(profile: &str) -> CredentialIdentity {
        CredentialIdentity::new("openai", "api", profile).unwrap()
    }

    fn begin(attempts: &mut ApiKeyAttempts, owner: CredentialIdentity) -> String {
        let AuthChallenge::ApiKeyInput { attempt_id, .. } =
            attempts.begin(owner, "provider", "Enter API key").unwrap()
        else {
            panic!("expected API-key input");
        };
        attempt_id
    }

    #[test]
    fn credentials_and_accounts_do_not_leak_through_attempt_ids() {
        let mut attempts = ApiKeyAttempts::default();
        let id = begin(&mut attempts, owner("work"));
        assert_eq!(id, "provider-1");
        assert_eq!(
            attempts.check(&owner("personal"), &id).unwrap_err().code,
            ProviderErrorCode::NotFound
        );
        assert_eq!(attempts.check(&owner("work"), &id).unwrap(), None);
        let completed = attempts.complete(
            &owner("work"),
            &id,
            AuthenticatedProfile {
                id: "work".into(),
                label: "Work".into(),
                method_id: "api-key".into(),
            },
        );
        assert!(matches!(completed, Ok(AuthAttemptState::Completed { .. })));
        assert_eq!(
            attempts.check(&owner("work"), &id).unwrap_err().code,
            ProviderErrorCode::NotFound
        );
    }

    #[test]
    fn cancellation_expiration_and_capacity_have_exclusive_outcomes() {
        let mut attempts = ApiKeyAttempts::default();
        let first = begin(&mut attempts, owner("work"));
        let cancelled = attempts.cancel(&owner("work"), &first).unwrap();
        assert_eq!(cancelled, AuthAttemptState::Cancelled);
        assert_eq!(
            attempts.cancel(&owner("work"), &first).unwrap_err().code,
            ProviderErrorCode::NotFound
        );
        let expired = begin(&mut attempts, owner("work"));
        attempts.pending.get_mut(&expired).unwrap().started =
            Instant::now() - ATTEMPT_TTL - Duration::from_secs(1);
        assert_eq!(
            attempts.check(&owner("work"), &expired).unwrap(),
            Some(AuthAttemptState::Expired)
        );
        for _ in 0..MAX_PENDING {
            begin(&mut attempts, owner("work"));
        }
        assert_eq!(
            attempts
                .begin(owner("work"), "provider", "Enter API key")
                .unwrap_err()
                .code,
            ProviderErrorCode::Unavailable
        );
    }
}
