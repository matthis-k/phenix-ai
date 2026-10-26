//! Opt-in provider-owned ChatGPT OAuth state machine.
//!
//! The kernel only enforces capabilities. A verified ChatGPT login is created
//! after an exact loopback callback, issued-client token exchange, signed OIDC
//! verification, granted plan scope and securely persisted named-account data.
//! No tokens, PKCE values, callback codes or id_token_hint enter ACP messages.
use phenix_provider_auth_strategies::BrowserAttempt;
use phenix_provider_builtins::{discovered_model_page, registrations};
use phenix_provider_contracts::{
    AuthAttemptQuery, AuthAttemptState, AuthChallenge, AuthQuery, AuthStart, AuthenticatedProfile,
    AuthenticationStatus, CredentialEnvironmentBinding, CredentialIdentity, CredentialImport,
    CredentialProfile, CredentialProfileStatus, CredentialRotation, CredentialSecret, ModelPage,
    ModelQuery, ProviderDescriptor, ProviderError, ProviderErrorCode,
};
use phenix_provider_protocols::{
    oauth_loopback::CallbackError,
    siwc::{AuthorizationAttempt, Registration, RegistrationError},
    siwc_models::parse_chatgpt_models,
    siwc_transport::{
        chatgpt_models, exchange_token, issuer_jwks, revoke_renewable_session, TransportError,
    },
    siwc_verified::{
        refresh_form, verify_chatgpt_identity, verify_chatgpt_persisted_identity,
        VerificationError, VerifiedChatGptIdentity,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zeroize::{Zeroize, Zeroizing};

const PROVIDER: &str = "openai";
const CONNECTION: &str = "chatgpt";
const METHOD: &str = "chatgpt-sign-in";
const MAX_ATTEMPTS: usize = 8;
const MAX_ATTEMPT_SECONDS: u64 = 600;

fn problem(code: ProviderErrorCode, message: &str) -> ProviderError {
    ProviderError {
        code,
        message: message.into(),
    }
}
fn denied(message: &str) -> ProviderError {
    problem(ProviderErrorCode::Denied, message)
}
fn unavailable(message: &str) -> ProviderError {
    problem(ProviderErrorCode::Unavailable, message)
}
fn oauth_callback_error(error: RegistrationError) -> ProviderError {
    match error {
        RegistrationError::AuthorizationDenied => {
            denied("ChatGPT OAuth authorization was declined")
        }
        RegistrationError::InvalidState => denied("OAuth callback state did not match"),
        RegistrationError::ClientMismatch => denied("OAuth issued client ID did not match"),
        _ => denied("OAuth callback parameters were invalid"),
    }
}

// Only fixed, provider-authored diagnostics cross the ACP boundary. Never
// propagate token endpoints' response bodies, callback URLs or vault errors.
fn token_exchange_error(error: TransportError) -> ProviderError {
    match error {
        TransportError::InvalidForm => denied("OAuth token exchange request was invalid"),
        TransportError::Unavailable => {
            unavailable("OAuth token transport returned no usable response")
        }
        TransportError::Rejected => denied("OAuth token endpoint rejected the authorization code"),
        TransportError::TooLarge => denied("OAuth token response exceeded the size limit"),
        TransportError::InvalidTokenResponse => denied("OAuth token response was invalid"),
        TransportError::Network => unavailable(
            "OAuth token request could not connect; check DNS, proxy, or VPN and retry sign-in",
        ),
        TransportError::Timeout => unavailable("OAuth token request timed out; retry sign-in"),
        TransportError::Tls => unavailable(
            "OAuth token TLS handshake or certificate verification failed; check system trust",
        ),
        TransportError::HttpStatus(429) => unavailable(
            "OAuth token endpoint rate limited the request (HTTP 429); retry sign-in later",
        ),
        TransportError::HttpStatus(300..=399) => unavailable(
            "OAuth token endpoint redirected unexpectedly; check proxy or captive portal",
        ),
        TransportError::HttpStatus(500..=599) => unavailable(
            "OAuth token endpoint returned a server error (HTTP 5xx); retry sign-in later",
        ),
        TransportError::HttpStatus(status) => unavailable(&format!(
            "OAuth token endpoint returned unexpected HTTP status {status}",
        )),
    }
}

fn identity_verification_error(error: VerificationError) -> ProviderError {
    match error {
        VerificationError::InvalidTokenResponse => denied("OAuth token response was invalid"),
        VerificationError::InvalidJwks => unavailable("OAuth issuer signing keys were invalid"),
        VerificationError::InvalidIdToken => denied("OAuth identity token was invalid"),
        VerificationError::MissingPlanPermission => {
            denied("ChatGPT plan permission was not granted")
        }
        VerificationError::IncorrectAccount => denied("OAuth account identity did not match"),
    }
}

fn now_epoch() -> Result<u64, ProviderError> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| unavailable("system clock unavailable"))?
        .as_secs())
}

/// Kept exclusively in ProviderCredentials under its exact authorized root.
/// Serialization happens only inside the provider-to-credential-store call.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredOAuth {
    host_id: String,
    // Preserve the issued registration after logout, without preserving
    // bearer/refresh credentials. Omitting None keeps old JSON generations
    // byte-for-byte CAS-compatible with the existing protected vault.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    registration_client_id: Option<String>,
    account: Option<OAuthAccount>,
}

impl StoredOAuth {
    /// A correlated dynamic-client registration must be durable before the
    /// authorization-code exchange. Retrying an expired/denied token grant
    /// reuses the issued client ID instead of creating another registration.
    /// Never manufacture an authenticated account from registration alone.
    fn registered_client(&self, client_id: &str) -> Self {
        Self {
            host_id: self.host_id.clone(),
            registration_client_id: Some(client_id.to_owned()),
            account: None,
        }
    }

    fn returning_client_id(&self) -> Option<&str> {
        self.account
            .as_ref()
            .map(|account| account.issued_client_id.as_str())
            .or(self.registration_client_id.as_deref())
    }

    /// Durable, token-free generation that can only replace the exact prior
    /// protected record by CAS. Reuse the same transition in sign-out and
    /// deterministic restart tests, so the test exercises production logic.
    fn signed_out(&self) -> Option<Self> {
        let client_id = self.returning_client_id()?;
        Some(Self {
            host_id: self.host_id.clone(),
            registration_client_id: Some(client_id.to_owned()),
            account: None,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OAuthAccount {
    subject: String,
    issued_client_id: String,
    id_token_nonce: String,
    access_token: String,
    refresh_token: String,
    id_token: String,
    granted_scope: String,
    expires_at: u64,
}

impl Drop for OAuthAccount {
    fn drop(&mut self) {
        // The protected credential store owns persistence. Use a
        // compiler-resistant erasure primitive for in-memory token strings,
        // including any temporary decoded JSON projections.
        self.access_token.zeroize();
        self.refresh_token.zeroize();
        self.id_token.zeroize();
    }
}

fn exact_identity(resource: Option<&str>) -> Result<CredentialIdentity, ProviderError> {
    let identity = CredentialIdentity::from_resource(
        resource.ok_or_else(|| denied("ChatGPT account is not authorized"))?,
    )
    .map_err(|_| denied("invalid ChatGPT account authorization"))?;
    if identity.provider_id() != PROVIDER || identity.connection_id() != CONNECTION {
        return Err(denied(
            "provider account does not match authorized resource",
        ));
    }
    Ok(identity)
}

fn descriptor() -> Result<ProviderDescriptor, ProviderError> {
    let mut provider = registrations()
        .into_iter()
        .find(|item| item.id == PROVIDER)
        .ok_or_else(|| unavailable("OpenAI provider registration missing"))?;
    provider
        .connections
        .retain(|connection| connection.id == CONNECTION);
    if provider.connections.len() != 1
        || provider.connections[0].auth_methods.len() != 1
        || provider.connections[0].auth_methods[0].id != METHOD
    {
        return Err(unavailable("ChatGPT OAuth connection is not installed"));
    }
    // The built-in registration intentionally only advertises metadata.
    // An authenticated ChatGPT-plan provider must contribute the explicit,
    // public inference origin before it can install an executable model.
    // SIWC documents /v1/responses, not private ChatGPT backend APIs.
    let connection = &mut provider.connections[0];
    connection.base_url = Some("https://api.openai.com/v1".into());
    for protocol in &mut connection.protocols {
        if protocol.id == "openai-responses" {
            protocol.endpoint = Some("https://api.openai.com/v1/responses".into());
        }
    }
    if connection.protocols.len() != 1
        || connection.protocols[0].id != "openai-responses"
        || connection.protocols[0].endpoint.as_deref()
            != Some("https://api.openai.com/v1/responses")
    {
        return Err(unavailable(
            "ChatGPT plan inference endpoint is not installed",
        ));
    }
    Ok(provider)
}

fn expired(account: &OAuthAccount, now: u64) -> bool {
    // Require a small safety window rather than begin inference on an expiring
    // token. Refresh/rotation will be implemented as a separate lifecycle step.
    account.expires_at <= now.saturating_add(30)
}

fn require_attempt_owner(
    expected: &CredentialIdentity,
    authorized: &CredentialIdentity,
) -> Result<(), ProviderError> {
    if expected != authorized {
        return Err(denied("OAuth attempt belongs to a different account"));
    }
    Ok(())
}

struct Pending {
    expires_at: Instant,
    identity: CredentialIdentity,
    browser: BrowserAttempt,
    /// Credential generation captured at browser begin. The callback
    /// cannot replace any newer login, refresh, or revocation of this account.
    prior: StoredOAuth,
}

#[phenix_sdk::plugin(
    id = "phenix.provider.chatgpt",
    version = "1.0.0",
    profile = "state",
    provide(phenix_provider_contracts::ProviderCatalog, slot = "Catalog"),
    provide(
        phenix_provider_contracts::ProviderRegistrations,
        slot = "Registrations"
    ),
    provide(phenix_provider_contracts::ProviderAuth, slot = "Authentication"),
    provide(phenix_provider_contracts::ProviderCredentials, slot = "Bearer"),
    require(phenix_provider_contracts::ProviderCredentials, slot = "Credentials")
)]
mod plugin {
    use super::*;
    use phenix_provider_contracts::{
        ProviderAuth, ProviderCatalog, ProviderCredentials, ProviderDetailsQuery, ProviderPage,
        ProviderQuery, ProviderRegistrations,
    };
    use phenix_sdk::{Authority, CallError, CallResult, Host, Operation, ScopedRequest};

    #[derive(Default)]
    pub struct State {
        attempts: BTreeMap<String, Pending>,
    }

    impl State {
        // Browser attempts can outlive disconnected clients. Reclaim expired
        // loopback listeners before enforcing the bounded attempt budget.
        // Expiry is monotonic and never makes an attempt authenticated.
        fn reap_expired_attempts(&mut self) {
            let now = Instant::now();
            self.attempts.retain(|_, pending| pending.expires_at > now);
        }
    }

    fn checked(host: &Host<'_>, scope: &str) -> CallResult<CredentialIdentity, ProviderError> {
        if scope != "user" {
            return Err(CallError::Domain(denied(
                "provider management scope not authorized",
            )));
        }
        exact_identity(host.resource()?.as_deref()).map_err(CallError::Domain)
    }

    fn load(
        host: &Host<'_>,
        identity: &CredentialIdentity,
    ) -> CallResult<Option<StoredOAuth>, ProviderError> {
        let scope = identity.resolve_scope().map_err(CallError::Domain)?;
        let secret = match host.invoke_scoped::<ProviderCredentials::resolve>(
            "Credentials",
            &scope,
            identity,
        ) {
            Ok(secret) => secret,
            Err(CallError::Domain(failure)) if failure.code == ProviderErrorCode::NotFound => {
                return Ok(None);
            }
            Err(problem) => return Err(problem),
        };
        let bytes = secret.expose_for_authorized_provider();
        let record: StoredOAuth = serde_json::from_slice(bytes).map_err(|_| {
            CallError::Domain(denied("invalid protected ChatGPT credential record"))
        })?;
        if !valid_host_id(&record.host_id) {
            return Err(CallError::Domain(denied(
                "invalid protected ChatGPT host ID",
            )));
        }
        Ok(Some(record))
    }

    fn save(
        host: &Host<'_>,
        identity: &CredentialIdentity,
        record: &StoredOAuth,
    ) -> CallResult<(), ProviderError> {
        let resource = identity.resource().map_err(CallError::Domain)?;
        let authority = Authority {
            operations: [<ProviderCredentials::import as Operation>::KEY.into()].into(),
            resources: [resource.clone()].into(),
        };
        let scope = ScopedRequest {
            authority,
            resource: Some(resource),
        };
        let bytes = serde_json::to_vec(record)
            .map_err(|_| CallError::Domain(unavailable("credential encoding failed")))?;
        let secret = CredentialSecret::new(bytes).map_err(CallError::Domain)?;
        host.invoke_scoped::<ProviderCredentials::import>(
            "Credentials",
            &scope,
            &CredentialImport {
                identity: identity.clone(),
                secret,
            },
        )
    }

    /// Native-only exact-account durable refresh replacement. The backend
    /// compares the *whole* original protected record under its exclusive
    /// lock. It can never replace a newer refresh generation, even if another
    /// process completed a refresh concurrently. Caller and backend each
    /// independently enforce exact resource scope.
    fn rotate(
        host: &Host<'_>,
        identity: &CredentialIdentity,
        expected: &StoredOAuth,
        replacement: &StoredOAuth,
    ) -> CallResult<bool, ProviderError> {
        let resource = identity.resource().map_err(CallError::Domain)?;
        let scope = ScopedRequest {
            authority: Authority {
                operations: [<ProviderCredentials::compare_exchange as Operation>::KEY.into()]
                    .into(),
                resources: [resource.clone()].into(),
            },
            resource: Some(resource),
        };
        let expected = CredentialSecret::new(
            serde_json::to_vec(expected)
                .map_err(|_| CallError::Domain(unavailable("invalid previous OAuth record")))?,
        )
        .map_err(CallError::Domain)?;
        let replacement = CredentialSecret::new(
            serde_json::to_vec(replacement)
                .map_err(|_| CallError::Domain(unavailable("invalid refreshed OAuth record")))?,
        )
        .map_err(CallError::Domain)?;
        host.invoke_scoped::<ProviderCredentials::compare_exchange>(
            "Credentials",
            &scope,
            &CredentialRotation {
                identity: identity.clone(),
                expected,
                replacement,
            },
        )
    }

    fn valid_host_id(value: &str) -> bool {
        value
            .strip_prefix("urn:uuid:")
            .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
    }

    fn stored_or_new(
        host: &Host<'_>,
        identity: &CredentialIdentity,
    ) -> CallResult<StoredOAuth, ProviderError> {
        if let Some(record) = load(host, identity)? {
            return Ok(record);
        }
        let record = StoredOAuth {
            host_id: format!("urn:uuid:{}", uuid::Uuid::new_v4()),
            registration_client_id: None,
            account: None,
        };
        // A durable host identity must exist BEFORE generating an OAuth
        // challenge so failed/cancelled attempts can reuse the same host.
        save(host, identity, &record)?;
        Ok(record)
    }

    /// Single provider-owned boundary shared by catalog discovery and
    /// the private selected-model bearer resolver. The encrypted/storage
    /// record is not itself a valid model credential; validate account,
    /// signature, scope, nonce and expiry on every use.
    fn authorized_account(
        host: &Host<'_>,
        identity: &CredentialIdentity,
    ) -> CallResult<OAuthAccount, ProviderError> {
        let mut record = load(host, identity)?.ok_or_else(|| {
            CallError::Domain(problem(
                ProviderErrorCode::AuthenticationRequired,
                "ChatGPT account is not authenticated",
            ))
        })?;
        let now = now_epoch().map_err(CallError::Domain)?;
        if record.account.is_none() {
            return Err(CallError::Domain(problem(
                ProviderErrorCode::AuthenticationRequired,
                "ChatGPT login not completed",
            )));
        }
        if record
            .account
            .as_ref()
            .is_some_and(|account| expired(account, now))
        {
            let previous = record.account.as_ref().unwrap();
            // Token forms contain a rotating credential. A zeroizing guard
            // ensures both network error and successful exchange clear it.
            let form = Zeroizing::new(
                refresh_form(&previous.issued_client_id, &previous.refresh_token)
                    .map_err(|_| CallError::Domain(denied("invalid saved refresh token")))?,
            );
            let tokens = exchange_token(&form).map_err(|_| {
                CallError::Domain(unavailable(
                    "ChatGPT refresh failed; reauthorize if revoked",
                ))
            })?;
            let keys = issuer_jwks()
                .map_err(|_| CallError::Domain(unavailable("issuer identity keys unavailable")))?;
            let verified = verify_chatgpt_persisted_identity(
                &tokens,
                &keys,
                &previous.issued_client_id,
                &previous.id_token_nonce,
                &previous.subject,
            )
            .map_err(|_| {
                CallError::Domain(denied("refreshed account identity or plan grant invalid"))
            })?;
            let expires_at = now
                .checked_add(tokens.expires_in)
                .ok_or_else(|| CallError::Domain(denied("invalid refreshed expiry")))?;
            let next = StoredOAuth {
                host_id: record.host_id.clone(),
                registration_client_id: record.registration_client_id.clone(),
                account: Some(OAuthAccount {
                    subject: verified.subject().to_owned(),
                    issued_client_id: verified.issued_client_id().to_owned(),
                    id_token_nonce: previous.id_token_nonce.clone(),
                    access_token: tokens.access_token.clone(),
                    refresh_token: tokens.refresh_token.clone(),
                    id_token: tokens.id_token.clone(),
                    granted_scope: tokens.granted_scope.clone(),
                    expires_at,
                }),
            };
            if !rotate(host, identity, &record, &next)? {
                return Err(CallError::Domain(problem(
                    ProviderErrorCode::Conflict,
                    "ChatGPT refresh was superseded; retry the selected account",
                )));
            }
            record = next;
        }
        let account = record.account.ok_or_else(|| {
            CallError::Domain(problem(
                ProviderErrorCode::AuthenticationRequired,
                "ChatGPT login not completed",
            ))
        })?;
        let keys =
            issuer_jwks().map_err(|_| CallError::Domain(unavailable("issuer keys unavailable")))?;
        let bundle = phenix_provider_protocols::siwc_verified::TokenEnvelope {
            access_token: account.access_token.clone(),
            refresh_token: account.refresh_token.clone(),
            id_token: account.id_token.clone(),
            expires_in: account.expires_at.saturating_sub(now),
            granted_scope: account.granted_scope.clone(),
        };
        verify_chatgpt_persisted_identity(
            &bundle,
            &keys,
            &account.issued_client_id,
            &account.id_token_nonce,
            &account.subject,
        )
        .map_err(|_| {
            CallError::Domain(denied("ChatGPT identity or plan permission not verified"))
        })?;
        Ok(account)
    }

    fn current_profile(
        host: &Host<'_>,
        identity: &CredentialIdentity,
    ) -> CallResult<Option<CredentialProfile>, ProviderError> {
        let Some(record) = load(host, identity)? else {
            return Ok(None);
        };
        if record.account.is_none() {
            return Ok(None);
        }
        // Reuse the same provider-authoritative renewal and signed-account
        // verification path as model discovery and selected bearer use.
        let ready = authorized_account(host, identity).is_ok();
        Ok(Some(CredentialProfile {
            id: identity.profile_id().into(),
            label: identity.profile_id().into(),
            method_id: METHOD.into(),
            status: if ready {
                CredentialProfileStatus::Authenticated
            } else {
                CredentialProfileStatus::Unavailable {
                    reason: "ChatGPT token is expired or identity cannot be revalidated".into(),
                }
            },
        }))
    }

    #[cfg(test)]
    #[test]
    fn abandoned_oauth_attempts_do_not_exhaust_login_capacity() {
        let identity = CredentialIdentity::new(PROVIDER, CONNECTION, "personal").unwrap();
        let pending = |expires_at: Instant| Pending {
            expires_at,
            identity: identity.clone(),
            browser: BrowserAttempt::new(Duration::from_secs(MAX_ATTEMPT_SECONDS)).unwrap(),
            prior: StoredOAuth {
                host_id: "urn:uuid:123e4567-e89b-12d3-a456-426614174000".into(),
                registration_client_id: None,
                account: None,
            },
        };
        let mut state = State::default();
        let now = Instant::now();
        state
            .attempts
            .insert("expired".into(), pending(now - Duration::from_secs(1)));
        state
            .attempts
            .insert("live".into(), pending(now + Duration::from_secs(60)));
        state.reap_expired_attempts();
        assert_eq!(state.attempts.len(), 1);
        assert!(state.attempts.contains_key("live"));
        assert!(!state.attempts.contains_key("expired"));
    }

    /// Declarative registration is account-independent. Completing sign-in,
    /// selecting a verified credential and dispatching a model remain separate
    /// authorized operations; this metadata alone grants none of them.
    #[provide(ProviderRegistrations::list)]
    fn registrations_list(_: &mut State) -> CallResult<ProviderPage, ProviderError> {
        Ok(ProviderPage {
            providers: vec![descriptor().map_err(CallError::Domain)?],
            revision: 1,
        })
    }

    #[provide(ProviderCatalog::list)]
    fn list(
        _: &mut State,
        host: &Host<'_>,
        query: ProviderQuery,
    ) -> CallResult<ProviderPage, ProviderError> {
        checked(host, &query.scope)?;
        Ok(ProviderPage {
            providers: vec![descriptor().map_err(CallError::Domain)?],
            revision: 1,
        })
    }

    #[provide(ProviderCatalog::describe)]
    fn describe(
        _: &mut State,
        host: &Host<'_>,
        query: ProviderDetailsQuery,
    ) -> CallResult<ProviderDescriptor, ProviderError> {
        checked(host, &query.scope)?;
        if query.provider_id != PROVIDER {
            return Err(CallError::Domain(denied("provider not installed")));
        }
        descriptor().map_err(CallError::Domain)
    }

    #[provide(ProviderCatalog::models)]
    fn models(
        _: &mut State,
        host: &Host<'_>,
        query: ModelQuery,
    ) -> CallResult<ModelPage, ProviderError> {
        let identity = checked(host, &query.scope)?;
        if query.provider_id != PROVIDER
            || query.connection_id != CONNECTION
            || query.profile_id.as_deref() != Some(identity.profile_id())
        {
            return Err(CallError::Domain(denied(
                "model catalog account not authorized",
            )));
        }
        if query.cursor.is_some() || query.limit == 0 || query.limit > 256 {
            return Err(CallError::Domain(denied(
                "invalid ChatGPT model page request",
            )));
        }
        let account = authorized_account(host, &identity)?;
        // Use the same verified reqwest/Rustls transport as OAuth token
        // exchange. The old generic discovery adapter still uses libcurl,
        // which failed for this user's real sign-in environment. Account
        // selection, bounded JSON, TLS, redirects and token ownership remain
        // enforced by the shared SIWC helper.
        let response = chatgpt_models(&account.access_token)
            .map_err(|_| CallError::Domain(unavailable("ChatGPT model catalog unavailable")))?;
        let discovered = parse_chatgpt_models(&response)
            .map_err(|_| CallError::Domain(unavailable("invalid ChatGPT model catalog")))?;
        discovered_model_page(
            identity.profile_id(),
            CONNECTION,
            "openai-responses",
            "responses",
            query.limit,
            discovered
                .into_iter()
                .map(|item| (item.slug, item.display_name)),
            |_| true,
        )
        .map_err(CallError::Domain)
    }

    // This provider also implements the complete credentials contract because
    // the M1 plugin descriptor requires a concrete handler for every supplied
    // operation. Only resolve is launcher-granted: interactive secret import,
    // variable binding and remote revocation cannot be invoked through this
    // OAuth-only bearer adapter. ProviderAuth owns its token persistence.
    #[provide(ProviderCredentials::import)]
    fn reject_direct_import(
        _: &mut State,
        _: &Host<'_>,
        _: CredentialImport,
    ) -> CallResult<(), ProviderError> {
        Err(CallError::Domain(denied(
            "ChatGPT OAuth does not import API keys",
        )))
    }

    #[provide(ProviderCredentials::bind_environment)]
    fn reject_environment_binding(
        _: &mut State,
        _: &Host<'_>,
        _: CredentialEnvironmentBinding,
    ) -> CallResult<(), ProviderError> {
        Err(CallError::Domain(denied(
            "ChatGPT OAuth has no API-key environment source",
        )))
    }

    #[provide(ProviderCredentials::revoke)]
    fn revoke_account(
        _: &mut State,
        host: &Host<'_>,
        request: CredentialIdentity,
    ) -> CallResult<(), ProviderError> {
        let authorized = exact_identity(host.resource()?.as_deref()).map_err(CallError::Domain)?;
        require_attempt_owner(&request, &authorized).map_err(CallError::Domain)?;
        let Some(previous) = load(host, &authorized)? else {
            return Ok(());
        };
        let Some(account) = previous.account.as_ref() else {
            return Ok(());
        };
        let issued_client_id = account.issued_client_id.clone();
        let token = Zeroizing::new(account.refresh_token.clone());
        // First invalidate all *future* bearer resolutions. If the remote
        // request times out we must never leave a locally usable refresh
        // token, and a racing callback/refresh must fail the whole-record CAS.
        let cleared = previous
            .signed_out()
            .ok_or_else(|| CallError::Domain(denied("ChatGPT account registration missing")))?;
        if !rotate(host, &authorized, &previous, &cleared)? {
            return Err(CallError::Domain(problem(
                ProviderErrorCode::Conflict,
                "ChatGPT account changed during sign-out; retry",
            )));
        }
        // A successful empty 200 confirms the remote session was revoked.
        // On transport failure, report local-only removal as unconfirmed.
        revoke_renewable_session(&token, &issued_client_id).map_err(|_| {
            CallError::Domain(problem(
                ProviderErrorCode::RemoteRevocationUnconfirmed,
                "ChatGPT credentials cleared locally; remote revocation unconfirmed",
            ))
        })
    }

    #[provide(ProviderCredentials::compare_exchange)]
    fn reject_external_rotation(
        _: &mut State,
        _: &Host<'_>,
        _: CredentialRotation,
    ) -> CallResult<bool, ProviderError> {
        Err(CallError::Domain(denied(
            "ChatGPT OAuth refresh state is only renewable through the selected account",
        )))
    }

    /// Controller-only selected-model resolution. This is intentionally the
    /// existing ProviderCredentials contract so the P1 Model installation
    /// remains generic. It is never mapped to a frontend credential RPC.
    /// Unlike the backing vault, this implementation returns only a currently
    /// validated access token, not the persisted OAuth JSON or refresh token.
    #[provide(ProviderCredentials::resolve)]
    fn resolve_bearer(
        _: &mut State,
        host: &Host<'_>,
        request: CredentialIdentity,
    ) -> CallResult<CredentialSecret, ProviderError> {
        let authorized = exact_identity(host.resource()?.as_deref()).map_err(CallError::Domain)?;
        require_attempt_owner(&request, &authorized).map_err(CallError::Domain)?;
        let mut account = authorized_account(host, &authorized)?;
        let access = std::mem::take(&mut account.access_token);
        CredentialSecret::new(access.into_bytes()).map_err(CallError::Domain)
    }

    #[provide(ProviderAuth::status)]
    fn status(
        _: &mut State,
        host: &Host<'_>,
        query: AuthQuery,
    ) -> CallResult<AuthenticationStatus, ProviderError> {
        let identity = checked(host, &query.scope)?;
        if query.provider_id != PROVIDER || query.connection_id != CONNECTION {
            return Err(CallError::Domain(denied(
                "provider connection not authorized",
            )));
        }
        let profiles = current_profile(host, &identity)?.into_iter().collect();
        Ok(AuthenticationStatus {
            provider_id: PROVIDER.into(),
            connection_id: CONNECTION.into(),
            profiles,
        })
    }

    #[provide(ProviderAuth::begin)]
    fn begin(
        state: &mut State,
        host: &Host<'_>,
        query: AuthStart,
    ) -> CallResult<AuthChallenge, ProviderError> {
        let identity = checked(host, &query.scope)?;
        if query.provider_id != PROVIDER
            || query.connection_id != CONNECTION
            || query.method_id != METHOD
        {
            return Err(CallError::Domain(denied("OAuth method not authorized")));
        }
        state.reap_expired_attempts();
        if state.attempts.len() >= MAX_ATTEMPTS {
            return Err(CallError::Domain(unavailable("too many OAuth attempts")));
        }
        let record = stored_or_new(host, &identity)?;
        let browser = BrowserAttempt::new(Duration::from_secs(MAX_ATTEMPT_SECONDS))
            .map_err(|_| CallError::Domain(unavailable("could not bind OAuth callback")))?;
        let redirect_uri = browser.redirect_uri();
        let registration = match record.returning_client_id() {
            Some(issued_client_id) => Registration::Returning {
                issued_client_id,
                id_token_hint: None, // never expose the saved ID token in client-visible URLs
                login_hint: None,
            },
            None => Registration::First {
                app_name: "Phenix AI",
            },
        };
        let attempt = AuthorizationAttempt {
            registration,
            host_id: &record.host_id,
            redirect_uri: &redirect_uri,
            state: browser.state(),
            nonce: browser.nonce(),
            pkce_challenge: browser.challenge(),
        };
        let url = attempt
            .authorization_url()
            .map_err(|_| CallError::Domain(denied("invalid OAuth registration")))?;
        let id = uuid::Uuid::new_v4().to_string();
        state.attempts.insert(
            id.clone(),
            Pending {
                expires_at: Instant::now() + Duration::from_secs(MAX_ATTEMPT_SECONDS),
                identity,
                browser,
                prior: record,
            },
        );
        Ok(AuthChallenge::OpenBrowser {
            attempt_id: id,
            url,
        })
    }

    #[provide(ProviderAuth::poll)]
    fn poll(
        state: &mut State,
        host: &Host<'_>,
        query: AuthAttemptQuery,
    ) -> CallResult<AuthAttemptState, ProviderError> {
        let identity = checked(host, &query.scope)?;
        if query.provider_id != PROVIDER || query.connection_id != CONNECTION {
            return Err(CallError::Domain(denied("OAuth attempt not authorized")));
        }
        let Some(found) = state.attempts.get(&query.attempt_id) else {
            return Err(CallError::Domain(problem(
                ProviderErrorCode::NotFound,
                "OAuth attempt not found",
            )));
        };
        // A known attempt ID does not authorize another account to poll it,
        // consume its one-shot callback, or acquire the resulting credential.
        require_attempt_owner(&found.identity, &identity).map_err(CallError::Domain)?;
        let Some(mut pending) = state.attempts.remove(&query.attempt_id) else {
            return Err(CallError::Domain(unavailable("OAuth attempt disappeared")));
        };
        let callback = match pending.browser.poll() {
            Ok(None) => {
                state.attempts.insert(query.attempt_id, pending);
                return Ok(AuthAttemptState::Pending);
            }
            Ok(Some(callback)) => callback,
            Err(CallbackError::Expired) => return Ok(AuthAttemptState::Expired),
            Err(_) => {
                return Ok(AuthAttemptState::Failed {
                    reason: denied("invalid or already consumed callback"),
                })
            }
        };
        let redirect = pending.browser.redirect_uri();
        let registration = match pending.prior.returning_client_id() {
            Some(issued_client_id) => Registration::Returning {
                issued_client_id,
                id_token_hint: None,
                login_hint: None,
            },
            None => Registration::First {
                app_name: "Phenix AI",
            },
        };
        let attempt = AuthorizationAttempt {
            registration,
            host_id: &pending.prior.host_id,
            redirect_uri: &redirect,
            state: pending.browser.state(),
            nonce: pending.browser.nonce(),
            pkce_challenge: pending.browser.challenge(),
        };
        let result = (|| -> CallResult<(VerifiedChatGptIdentity, StoredOAuth), ProviderError> {
            let correlated = attempt
                .accept_callback(callback.uri())
                .map_err(|error| CallError::Domain(oauth_callback_error(error)))?;
            let client_id = correlated.issued_client_id().to_owned();
            // SIWC dynamic registration returns a new issued client ID before
            // token exchange. Save that token-free registration under the
            // exact account via CAS, even when exchange or OIDC later fails.
            // Returning registrations are already durable and must not be
            // replaced speculatively.
            let registration_record = if pending.prior.returning_client_id().is_none() {
                let registered = pending.prior.registered_client(&client_id);
                if !rotate(host, &identity, &pending.prior, &registered).map_err(|_| {
                    CallError::Domain(unavailable("OAuth registration persistence failed"))
                })? {
                    return Err(CallError::Domain(problem(
                        ProviderErrorCode::Conflict,
                        "ChatGPT registration was superseded; sign in again for this account",
                    )));
                }
                Some(registered)
            } else {
                None
            };
            // The typed SIWC exchange_form already returns Zeroizing<String>:
            // dropping it clears its allocation on both successful and failed
            // transport paths, without illegally moving from a Zeroizing guard.
            let form = correlated
                .exchange_form(pending.browser.verifier())
                .map_err(|_| CallError::Domain(denied("invalid OAuth exchange")))?;
            let tokens = exchange_token(&form)
                .map_err(|error| CallError::Domain(token_exchange_error(error)))?;
            let jwks = issuer_jwks()
                .map_err(|_| CallError::Domain(unavailable("issuer identity keys unavailable")))?;
            let verified = verify_chatgpt_identity(
                &tokens,
                &jwks,
                &client_id,
                pending.browser.nonce(),
                pending
                    .prior
                    .account
                    .as_ref()
                    .map(|account| account.subject.as_str()),
            )
            .map_err(|error| CallError::Domain(identity_verification_error(error)))?;
            let expires_at = now_epoch()
                .map_err(CallError::Domain)?
                .checked_add(tokens.expires_in)
                .ok_or_else(|| CallError::Domain(denied("invalid OAuth expiration")))?;
            let account = OAuthAccount {
                subject: verified.subject().to_owned(),
                issued_client_id: verified.issued_client_id().to_owned(),
                id_token_nonce: pending.browser.nonce().to_owned(),
                access_token: tokens.access_token.clone(),
                refresh_token: tokens.refresh_token.clone(),
                id_token: tokens.id_token.clone(),
                granted_scope: tokens.granted_scope.clone(),
                expires_at,
            };
            let record = StoredOAuth {
                host_id: pending.prior.host_id.clone(),
                registration_client_id: Some(client_id),
                account: Some(account),
            };
            // Backend failures can contain private paths or implementation details.
            // Report the stage without forwarding an untrusted vault error.
            let expected = registration_record.as_ref().unwrap_or(&pending.prior);
            if !rotate(host, &identity, expected, &record).map_err(|_| {
                CallError::Domain(unavailable("OAuth credential persistence failed"))
            })? {
                return Err(CallError::Domain(problem(
                    ProviderErrorCode::Conflict,
                    "ChatGPT login was superseded; sign in again for this account",
                )));
            }
            Ok((verified, record))
        })();
        match result {
            Ok((verified, _)) => {
                let _ = callback.confirm_verified(verified.verified_subject());
                Ok(AuthAttemptState::Completed {
                    profile: AuthenticatedProfile {
                        id: identity.profile_id().into(),
                        label: identity.profile_id().into(),
                        method_id: METHOD.into(),
                    },
                })
            }
            Err(CallError::Domain(reason)) => {
                let _ = callback.reject();
                Ok(AuthAttemptState::Failed { reason })
            }
            Err(_) => {
                let _ = callback.reject();
                Ok(AuthAttemptState::Failed {
                    reason: unavailable("OAuth completion failed"),
                })
            }
        }
    }

    #[provide(ProviderAuth::cancel)]
    fn cancel(
        state: &mut State,
        host: &Host<'_>,
        query: AuthAttemptQuery,
    ) -> CallResult<AuthAttemptState, ProviderError> {
        let identity = checked(host, &query.scope)?;
        if query.provider_id != PROVIDER || query.connection_id != CONNECTION {
            return Err(CallError::Domain(denied("OAuth attempt not authorized")));
        }
        let Some(found) = state.attempts.get(&query.attempt_id) else {
            return Err(CallError::Domain(problem(
                ProviderErrorCode::NotFound,
                "OAuth attempt not found",
            )));
        };
        // A second named account cannot cancel another account's pending
        // authorization, even if the attempt identifier becomes visible.
        require_attempt_owner(&found.identity, &identity).map_err(CallError::Domain)?;
        state.attempts.remove(&query.attempt_id);
        Ok(AuthAttemptState::Cancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_provider_contributes_plain_declaration_without_account_grant() {
        use phenix_sdk::ContractMetadata;
        let meta = ContractMetadata::decode(plugin::METADATA.bytes()).unwrap();
        assert_eq!(meta.provides.len(), 4);
        assert_eq!(meta.requires.len(), 1);
        assert_eq!(meta.requires[0].slot, "Credentials");
        let page = descriptor().unwrap();
        assert_eq!(page.id, "openai");
        assert_eq!(page.connections.len(), 1);
        assert_eq!(page.connections[0].id, "chatgpt");
        assert_eq!(page.connections[0].auth_methods.len(), 1);
        assert_eq!(page.connections[0].auth_methods[0].id, METHOD);
    }

    #[test]
    fn legacy_vault_record_remains_byte_exact_for_atomic_rotation() {
        let legacy =
            r#"{"host_id":"urn:uuid:123e4567-e89b-12d3-a456-426614174000","account":null}"#;
        let record: StoredOAuth = serde_json::from_str(legacy).unwrap();
        assert!(record.registration_client_id.is_none());
        assert_eq!(serde_json::to_string(&record).unwrap(), legacy);
    }

    #[test]
    fn first_registration_survives_failed_token_exchange_without_login() {
        let previous = StoredOAuth {
            host_id: "urn:uuid:123e4567-e89b-12d3-a456-426614174000".into(),
            registration_client_id: None,
            account: None,
        };
        let registered = previous.registered_client("oaiapp_issued");
        assert_eq!(registered.host_id, previous.host_id);
        assert_eq!(registered.returning_client_id(), Some("oaiapp_issued"));
        assert!(registered.account.is_none());
        let recovered: StoredOAuth =
            serde_json::from_slice(&serde_json::to_vec(&registered).unwrap()).unwrap();
        assert_eq!(recovered.returning_client_id(), Some("oaiapp_issued"));
        assert!(recovered.account.is_none());
    }

    #[test]
    fn logout_tombstone_retains_host_and_client_but_no_bearer_tokens() {
        let host_id = "urn:uuid:123e4567-e89b-12d3-a456-426614174000";
        let account = OAuthAccount {
            subject: "user-fixture".into(),
            issued_client_id: "issued-client".into(),
            id_token_nonce: "nonce-fixture".into(),
            access_token: "secret-access-fixture".into(),
            refresh_token: "secret-refresh-fixture".into(),
            id_token: "secret-id-fixture".into(),
            granted_scope: "chatgpt.tokens.use.direct".into(),
            expires_at: 100,
        };
        let record = StoredOAuth {
            host_id: host_id.into(),
            registration_client_id: None,
            account: Some(account),
        };
        assert_eq!(record.returning_client_id(), Some("issued-client"));
        let cleared = record.signed_out().expect("existing account");
        assert!(record.signed_out().is_some());
        assert_eq!(cleared.host_id, host_id);
        assert_eq!(cleared.returning_client_id(), Some("issued-client"));
        let serialized = serde_json::to_string(&cleared).unwrap();
        for secret in [
            "secret-access-fixture",
            "secret-refresh-fixture",
            "secret-id-fixture",
        ] {
            assert!(!serialized.contains(secret));
        }
        let restarted: StoredOAuth = serde_json::from_str(&serialized).unwrap();
        assert!(restarted.account.is_none());
        assert_eq!(restarted.returning_client_id(), Some("issued-client"));
        assert_eq!(restarted.host_id, host_id);
    }

    #[test]
    fn pinned_root_and_registration_are_independent_of_browser_account() {
        let expected = CredentialIdentity::new("openai", "chatgpt", "personal").unwrap();
        let other = CredentialIdentity::new("openai", "api", "personal").unwrap();
        assert_eq!(
            exact_identity(Some(&expected.resource().unwrap())).unwrap(),
            expected
        );
        assert!(exact_identity(Some(&other.resource().unwrap())).is_err());
        assert!(exact_identity(None).is_err());
        assert!(expired(
            &OAuthAccount {
                subject: "user".into(),
                issued_client_id: "issued".into(),
                id_token_nonce: "test-nonce".into(),
                access_token: "access".into(),
                refresh_token: "refresh".into(),
                id_token: "signed".into(),
                granted_scope: "openid".into(),
                expires_at: 1,
            },
            100
        ));
    }

    #[test]
    fn oauth_attempt_is_bound_to_the_exact_authorized_account() {
        let owner = CredentialIdentity::new("openai", "chatgpt", "personal").unwrap();
        let other = CredentialIdentity::new("openai", "chatgpt", "work").unwrap();
        assert!(require_attempt_owner(&owner, &owner).is_ok());
        assert_eq!(
            require_attempt_owner(&owner, &other).unwrap_err().code,
            ProviderErrorCode::Denied,
        );
    }

    #[test]
    fn oauth_completion_failures_report_the_stage_without_sensitive_data() {
        let declined = oauth_callback_error(RegistrationError::AuthorizationDenied);
        assert_eq!(declined.message, "ChatGPT OAuth authorization was declined");

        let exchange = token_exchange_error(TransportError::Rejected);
        assert_eq!(exchange.code, ProviderErrorCode::Denied);
        assert_eq!(
            exchange.message,
            "OAuth token endpoint rejected the authorization code"
        );

        let token = token_exchange_error(TransportError::InvalidTokenResponse);
        assert_eq!(token.message, "OAuth token response was invalid");

        for (failure, expected) in [
            (TransportError::Network, "could not connect"),
            (TransportError::Timeout, "timed out"),
            (TransportError::Tls, "TLS handshake"),
            (TransportError::HttpStatus(429), "HTTP 429"),
            (TransportError::HttpStatus(302), "redirected unexpectedly"),
            (TransportError::HttpStatus(503), "HTTP 5xx"),
            (TransportError::HttpStatus(404), "HTTP status 404"),
        ] {
            let error = token_exchange_error(failure);
            assert_eq!(error.code, ProviderErrorCode::Unavailable);
            assert!(error.message.contains(expected), "{error:?}");
            assert!(!error.message.contains("private"));
        }

        let scope = identity_verification_error(VerificationError::MissingPlanPermission);
        assert_eq!(scope.code, ProviderErrorCode::Denied);
        assert_eq!(scope.message, "ChatGPT plan permission was not granted");

        let account = identity_verification_error(VerificationError::IncorrectAccount);
        assert_eq!(account.message, "OAuth account identity did not match");
    }

    #[test]
    fn provider_descriptor_does_not_offer_api_billing_keys_as_subscription_auth() {
        let catalog = descriptor().unwrap();
        assert_eq!(catalog.connections.len(), 1);
        assert_eq!(catalog.connections[0].id, CONNECTION);
        assert_eq!(catalog.connections[0].auth_methods[0].id, METHOD);
        assert_eq!(
            catalog.connections[0].base_url.as_deref(),
            Some("https://api.openai.com/v1")
        );
        assert_eq!(catalog.connections[0].protocols.len(), 1);
        assert_eq!(
            catalog.connections[0].protocols[0].endpoint.as_deref(),
            Some("https://api.openai.com/v1/responses")
        );
    }
}
