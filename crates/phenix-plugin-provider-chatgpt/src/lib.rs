//! Opt-in provider-owned ChatGPT OAuth state machine.
//!
//! The kernel only enforces capabilities. A verified ChatGPT login is created
//! after an exact loopback callback, issued-client token exchange, signed OIDC
//! verification, granted plan scope and securely persisted named-account data.
//! No tokens, PKCE values, callback codes or id_token_hint enter ACP messages.
use phenix_provider_builtins::registrations;
use phenix_provider_contracts::{
    AuthAttemptQuery, AuthAttemptState, AuthChallenge, AuthQuery, AuthStart,
    AuthenticatedProfile, AuthenticationStatus, CredentialIdentity, CredentialImport,
    CredentialProfile, CredentialProfileStatus, CredentialSecret, ModelQuery, ModelPage,
    ProviderDescriptor, ProviderError, ProviderErrorCode,
};
use phenix_provider_protocols::{
    oauth_loopback::{OAuthLoopbackListener, CallbackError},
    siwc::{AuthorizationAttempt, Registration},
    siwc_transport::{exchange_token, issuer_jwks},
    siwc_verified::{verify_chatgpt_identity, GeneratedPkce, VerifiedChatGptIdentity},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

const PROVIDER: &str = "openai";
const CONNECTION: &str = "chatgpt";
const METHOD: &str = "chatgpt-sign-in";
const MAX_ATTEMPTS: usize = 8;
const MAX_ATTEMPT_SECONDS: u64 = 600;

fn problem(code: ProviderErrorCode, message: &str) -> ProviderError {
    ProviderError { code, message: message.into() }
}
fn denied(message: &str) -> ProviderError {
    problem(ProviderErrorCode::Denied, message)
}
fn unavailable(message: &str) -> ProviderError {
    problem(ProviderErrorCode::Unavailable, message)
}
fn now_epoch() -> Result<u64, ProviderError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)
        .map_err(|_| unavailable("system clock unavailable"))?.as_secs())
}

/// Kept exclusively in ProviderCredentials under its exact authorized root.
/// Serialization happens only inside the provider-to-credential-store call.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredOAuth {
    host_id: String,
    account: Option<OAuthAccount>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OAuthAccount {
    subject: String,
    issued_client_id: String,
    access_token: String,
    refresh_token: String,
    id_token: String,
    granted_scope: String,
    expires_at: u64,
}

fn exact_identity(resource: Option<&str>) -> Result<CredentialIdentity, ProviderError> {
    let identity = CredentialIdentity::from_resource(
        resource.ok_or_else(|| denied("ChatGPT account is not authorized"))?
    ).map_err(|_| denied("invalid ChatGPT account authorization"))?;
    if identity.provider_id() != PROVIDER || identity.connection_id() != CONNECTION {
        return Err(denied("provider account does not match authorized resource"));
    }
    Ok(identity)
}

fn descriptor() -> Result<ProviderDescriptor, ProviderError> {
    let mut provider = registrations().into_iter()
        .find(|item| item.id == PROVIDER)
        .ok_or_else(|| unavailable("OpenAI provider registration missing"))?;
    provider.connections.retain(|connection| connection.id == CONNECTION);
    if provider.connections.len() != 1
        || provider.connections[0].auth_methods.len() != 1
        || provider.connections[0].auth_methods[0].id != METHOD
    {
        return Err(unavailable("ChatGPT OAuth connection is not installed"));
    }
    Ok(provider)
}

fn expired(account: &OAuthAccount, now: u64) -> bool {
    // Require a small safety window rather than begin inference on an expiring
    // token. Refresh/rotation will be implemented as a separate lifecycle step.
    account.expires_at <= now.saturating_add(30)
}

struct Pending {
    listener: OAuthLoopbackListener,
    pkce: GeneratedPkce,
    host_id: String,
    issued_client_id: Option<String>,
    expected_subject: Option<String>,
}

#[phenix_sdk::plugin(
    id = "phenix.provider.chatgpt",
    version = "1.0.0",
    profile = "state",
    provide(phenix_provider_contracts::ProviderCatalog, slot = "Catalog"),
    provide(phenix_provider_contracts::ProviderAuth, slot = "Authentication"),
    require(phenix_provider_contracts::ProviderCredentials, slot = "Credentials")
)]
mod plugin {
    use super::*;
    use phenix_provider_contracts::{
        ProviderAuth, ProviderCatalog, ProviderCredentials, ProviderDetailsQuery,
        ProviderPage, ProviderQuery,
    };
    use phenix_sdk::{Authority, CallError, CallResult, Host, Operation, ScopedRequest};

    #[derive(Default)]
    pub struct State {
        attempts: BTreeMap<String, Pending>,
    }

    fn checked(host: &Host<'_>, scope: &str) -> CallResult<CredentialIdentity, ProviderError> {
        if scope != "user" {
            return Err(CallError::Domain(denied("provider management scope not authorized")));
        }
        exact_identity(host.resource()?.as_deref()).map_err(CallError::Domain)
    }

    fn load(host: &Host<'_>, identity: &CredentialIdentity)
        -> CallResult<Option<StoredOAuth>, ProviderError>
    {
        let scope = identity.resolve_scope().map_err(CallError::Domain)?;
        let secret = match host.invoke_scoped::<ProviderCredentials::resolve>(
            "Credentials", &scope, identity
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
            return Err(CallError::Domain(denied("invalid protected ChatGPT host ID")));
        }
        Ok(Some(record))
    }

    fn save(host: &Host<'_>, identity: &CredentialIdentity, record: &StoredOAuth)
        -> CallResult<(), ProviderError>
    {
        let resource = identity.resource().map_err(CallError::Domain)?;
        let authority = Authority {
            operations: [<ProviderCredentials::import as Operation>::KEY.into()].into(),
            resources: [resource.clone()].into(),
        };
        let scope = ScopedRequest { authority, resource: Some(resource) };
        let bytes = serde_json::to_vec(record)
            .map_err(|_| CallError::Domain(unavailable("credential encoding failed")))?;
        let secret = CredentialSecret::new(bytes).map_err(CallError::Domain)?;
        host.invoke_scoped::<ProviderCredentials::import>(
            "Credentials", &scope, CredentialImport {
                identity: identity.clone(), secret,
            }
        )
    }

    fn valid_host_id(value: &str) -> bool {
        value.strip_prefix("urn:uuid:")
            .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
    }

    fn stored_or_new(host: &Host<'_>, identity: &CredentialIdentity)
        -> CallResult<StoredOAuth, ProviderError>
    {
        if let Some(record) = load(host, identity)? { return Ok(record); }
        let record = StoredOAuth {
            host_id: format!("urn:uuid:{}", uuid::Uuid::new_v4()),
            account: None,
        };
        // A durable host identity must exist BEFORE generating an OAuth
        // challenge so failed/cancelled attempts can reuse the same host.
        save(host, identity, &record)?;
        Ok(record)
    }

    fn current_profile(host: &Host<'_>, identity: &CredentialIdentity)
        -> CallResult<Option<CredentialProfile>, ProviderError>
    {
        let Some(record) = load(host, identity)? else { return Ok(None); };
        let Some(account) = record.account else { return Ok(None); };
        let ready = !expired(&account, now_epoch().map_err(CallError::Domain)?)
            && issuer_jwks()
                .ok()
                .and_then(|jwks| verify_chatgpt_identity(
                    &phenix_provider_protocols::siwc_verified::TokenEnvelope {
                        access_token: account.access_token.clone(),
                        refresh_token: account.refresh_token.clone(),
                        id_token: account.id_token.clone(),
                        expires_in: account.expires_at.saturating_sub(
                            now_epoch().unwrap_or(u64::MAX)
                        ),
                        granted_scope: account.granted_scope.clone(),
                    },
                    &jwks, &account.issued_client_id,
                    // The original nonce belongs only to the completed
                    // attempt. Persisting a separate signed-ID-token proof
                    // requires the nonce in the protected account record.
                    "", Some(&account.subject),
                ).ok())
                .is_some();
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

    #[provide(ProviderCatalog::list)]
    fn list(_: &mut State, host: &Host<'_>, query: ProviderQuery)
        -> CallResult<ProviderPage, ProviderError>
    {
        checked(host, &query.scope)?;
        Ok(ProviderPage {
            providers: vec![descriptor().map_err(CallError::Domain)?], revision: 1
        })
    }

    #[provide(ProviderCatalog::describe)]
    fn describe(_: &mut State, host: &Host<'_>, query: ProviderDetailsQuery)
        -> CallResult<ProviderDescriptor, ProviderError>
    {
        checked(host, &query.scope)?;
        if query.provider_id != PROVIDER {
            return Err(CallError::Domain(denied("provider not installed")));
        }
        descriptor().map_err(CallError::Domain)
    }

    #[provide(ProviderCatalog::models)]
    fn models(_: &mut State, host: &Host<'_>, query: ModelQuery)
        -> CallResult<ModelPage, ProviderError>
    {
        let identity = checked(host, &query.scope)?;
        if query.provider_id != PROVIDER || query.connection_id != CONNECTION
            || query.profile_id.as_deref() != Some(identity.profile_id())
        {
            return Err(CallError::Domain(denied("model catalog account not authorized")));
        }
        Err(CallError::Domain(unavailable(
            "ChatGPT model discovery requires authenticated selected HTTP transport",
        )))
    }

    #[provide(ProviderAuth::status)]
    fn status(_: &mut State, host: &Host<'_>, query: AuthQuery)
        -> CallResult<AuthenticationStatus, ProviderError>
    {
        let identity = checked(host, &query.scope)?;
        if query.provider_id != PROVIDER || query.connection_id != CONNECTION {
            return Err(CallError::Domain(denied("provider connection not authorized")));
        }
        let profiles = current_profile(host, &identity)?.into_iter().collect();
        Ok(AuthenticationStatus {
            provider_id: PROVIDER.into(), connection_id: CONNECTION.into(), profiles
        })
    }

    #[provide(ProviderAuth::begin)]
    fn begin(state: &mut State, host: &Host<'_>, query: AuthStart)
        -> CallResult<AuthChallenge, ProviderError>
    {
        let identity = checked(host, &query.scope)?;
        if query.provider_id != PROVIDER || query.connection_id != CONNECTION
            || query.method_id != METHOD
        {
            return Err(CallError::Domain(denied("OAuth method not authorized")));
        }
        if state.attempts.len() >= MAX_ATTEMPTS {
            return Err(CallError::Domain(unavailable("too many OAuth attempts")));
        }
        let record = stored_or_new(host, &identity)?;
        let listener = OAuthLoopbackListener::bind(Duration::from_secs(MAX_ATTEMPT_SECONDS))
            .map_err(|_| CallError::Domain(unavailable("could not bind OAuth callback")))?;
        let pkce = GeneratedPkce::new();
        let redirect_uri = listener.redirect_uri();
        let registration = match &record.account {
            Some(account) => Registration::Returning {
                issued_client_id: &account.issued_client_id,
                id_token_hint: Some(&account.id_token),
                login_hint: None,
            },
            None => Registration::First { app_name: "Phenix AI" },
        };
        let attempt = AuthorizationAttempt {
            registration, host_id: &record.host_id, redirect_uri: &redirect_uri,
            state: pkce.state(), nonce: pkce.nonce(), pkce_challenge: pkce.challenge(),
        };
        let url = attempt.authorization_url()
            .map_err(|_| CallError::Domain(denied("invalid OAuth registration")))?;
        let id = uuid::Uuid::new_v4().to_string();
        state.attempts.insert(id.clone(), Pending {
            listener, pkce, host_id: record.host_id,
            issued_client_id: record.account.as_ref().map(|a| a.issued_client_id.clone()),
            expected_subject: record.account.as_ref().map(|a| a.subject.clone()),
        });
        Ok(AuthChallenge::OpenBrowser { attempt_id: id, url })
    }

    #[provide(ProviderAuth::poll)]
    fn poll(state: &mut State, host: &Host<'_>, query: AuthAttemptQuery)
        -> CallResult<AuthAttemptState, ProviderError>
    {
        let identity = checked(host, &query.scope)?;
        if query.provider_id != PROVIDER || query.connection_id != CONNECTION {
            return Err(CallError::Domain(denied("OAuth attempt not authorized")));
        }
        let Some(mut pending) = state.attempts.remove(&query.attempt_id) else {
            return Err(CallError::Domain(problem(
                ProviderErrorCode::NotFound, "OAuth attempt not found",
            )));
        };
        let callback = match pending.listener.poll() {
            Ok(None) => {
                state.attempts.insert(query.attempt_id, pending);
                return Ok(AuthAttemptState::Pending);
            },
            Ok(Some(callback)) => callback,
            Err(CallbackError::Expired) => return Ok(AuthAttemptState::Expired),
            Err(_) => return Ok(AuthAttemptState::Failed {
                reason: denied("invalid or already consumed callback"),
            }),
        };
        let redirect = pending.listener.redirect_uri();
        let registration = match pending.issued_client_id.as_deref() {
            Some(issued_client_id) => Registration::Returning {
                issued_client_id, id_token_hint: None, login_hint: None,
            },
            None => Registration::First { app_name: "Phenix AI" },
        };
        let attempt = AuthorizationAttempt {
            registration, host_id: &pending.host_id, redirect_uri: &redirect,
            state: pending.pkce.state(), nonce: pending.pkce.nonce(),
            pkce_challenge: pending.pkce.challenge(),
        };
        let result = (|| -> CallResult<(VerifiedChatGptIdentity, StoredOAuth), ProviderError> {
            let correlated = attempt.accept_callback(callback.uri())
                .map_err(|_| CallError::Domain(denied("OAuth callback state was invalid")))?;
            let client_id = correlated.issued_client_id().to_owned();
            let mut form = correlated.exchange_form(pending.pkce.verifier())
                .map_err(|_| CallError::Domain(denied("invalid OAuth exchange")))?;
            let tokens = exchange_token(&form)
                .map_err(|_| CallError::Domain(denied("OAuth code exchange rejected")))?;
            let mut form_bytes = std::mem::take(&mut form).into_bytes();
            form_bytes.fill(0);
            let jwks = issuer_jwks()
                .map_err(|_| CallError::Domain(unavailable("issuer identity keys unavailable")))?;
            let verified = verify_chatgpt_identity(
                &tokens, &jwks, &client_id, pending.pkce.nonce(),
                pending.expected_subject.as_deref(),
            ).map_err(|_| CallError::Domain(denied("OAuth identity or plan grant not verified")))?;
            let expires_at = now_epoch().map_err(CallError::Domain)?
                .checked_add(tokens.expires_in)
                .ok_or_else(|| CallError::Domain(denied("invalid OAuth expiration")))?;
            let account = OAuthAccount {
                subject: verified.subject.clone(),
                issued_client_id: verified.issued_client_id.clone(),
                access_token: tokens.access_token.clone(),
                refresh_token: tokens.refresh_token.clone(),
                id_token: tokens.id_token.clone(),
                granted_scope: tokens.granted_scope.clone(),
                expires_at,
            };
            let record = StoredOAuth { host_id: pending.host_id.clone(), account: Some(account) };
            save(host, &identity, &record)?;
            Ok((verified, record))
        })();
        match result {
            Ok((verified, _)) => {
                let _ = callback.confirm_chatgpt(&verified);
                Ok(AuthAttemptState::Completed {
                    profile: AuthenticatedProfile {
                        id: identity.profile_id().into(),
                        label: identity.profile_id().into(),
                        method_id: METHOD.into(),
                    }
                })
            },
            Err(_) => {
                let _ = callback.reject();
                Ok(AuthAttemptState::Failed {
                    reason: denied("OAuth verification or credential persistence failed"),
                })
            }
        }
    }

    #[provide(ProviderAuth::cancel)]
    fn cancel(state: &mut State, host: &Host<'_>, query: AuthAttemptQuery)
        -> CallResult<AuthAttemptState, ProviderError>
    {
        checked(host, &query.scope)?;
        if query.provider_id != PROVIDER || query.connection_id != CONNECTION {
            return Err(CallError::Domain(denied("OAuth attempt not authorized")));
        }
        if state.attempts.remove(&query.attempt_id).is_none() {
            return Err(CallError::Domain(problem(
                ProviderErrorCode::NotFound, "OAuth attempt not found",
            )));
        }
        Ok(AuthAttemptState::Cancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_root_and_registration_are_independent_of_browser_account() {
        let expected = CredentialIdentity::new("openai", "chatgpt", "personal").unwrap();
        let other = CredentialIdentity::new("openai", "api", "personal").unwrap();
        assert_eq!(exact_identity(Some(&expected.resource().unwrap())).unwrap(), expected);
        assert!(exact_identity(Some(&other.resource().unwrap())).is_err());
        assert!(exact_identity(None).is_err());
        assert!(expired(&OAuthAccount {
            subject: "user".into(), issued_client_id: "issued".into(),
            access_token: "access".into(), refresh_token: "refresh".into(),
            id_token: "signed".into(), granted_scope: "openid".into(), expires_at: 1,
        }, 100));
    }

    #[test]
    fn provider_descriptor_does_not_offer_api_billing_keys_as_subscription_auth() {
        let catalog = descriptor().unwrap();
        assert_eq!(catalog.connections.len(), 1);
        assert_eq!(catalog.connections[0].id, CONNECTION);
        assert_eq!(catalog.connections[0].auth_methods[0].id, METHOD);
    }
}
