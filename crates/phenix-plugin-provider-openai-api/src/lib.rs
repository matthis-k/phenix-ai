//! Opt-in OpenAI API-key provider implementation.
//!
//! Registration does not imply authentication. Every profile status and model
//! query resolves the one launcher-authorized credential and verifies it over
//! the explicitly documented HTTPS /v1/models endpoint. The kernel does not
//! know the provider or manage its secrets. This plugin does not implement
//! subscription OAuth, plaintext storage, or arbitrary endpoint configuration.
use curl::easy::{Easy2, Handler, List, WriteError};
use phenix_provider_builtins::registrations;
use phenix_provider_contracts::{
    AuthAttemptQuery, AuthAttemptState, AuthChallenge, AuthQuery, AuthStart, AuthenticatedProfile,
    AuthenticationStatus, ConnectionDescriptor, CredentialIdentity, CredentialProfile,
    CredentialProfileStatus, ModelOrigin, ModelPage, ModelQuery, ProviderDescriptor, ProviderError,
    ProviderErrorCode, ProviderModel,
};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

const PROVIDER: &str = "openai";
const CONNECTION: &str = "api";
const MODEL_ENDPOINT: &str = "https://api.openai.com/v1/models";
const MAX_MODELS: usize = 256;
const MAX_BODY: usize = 256 * 1024;

fn error(code: ProviderErrorCode, message: &str) -> ProviderError {
    ProviderError {
        code,
        message: message.into(),
    }
}
fn invalid(message: &str) -> ProviderError {
    error(ProviderErrorCode::InvalidInput, message)
}

fn selected_profile() -> Result<CredentialIdentity, ProviderError> {
    let profile = std::env::var("PHENIX_OPENAI_API_PROFILE").map_err(|_| {
        error(
            ProviderErrorCode::Unavailable,
            "OpenAI API profile is not configured",
        )
    })?;
    CredentialIdentity::new(PROVIDER, CONNECTION, profile)
}

fn descriptor() -> Result<ProviderDescriptor, ProviderError> {
    let mut provider = registrations()
        .into_iter()
        .find(|p| p.id == PROVIDER)
        .ok_or_else(|| {
            error(
                ProviderErrorCode::Unavailable,
                "OpenAI registration missing",
            )
        })?;
    provider
        .connections
        .retain(|connection| connection.id == CONNECTION);
    if provider.connections.len() != 1 {
        return Err(error(
            ProviderErrorCode::Unavailable,
            "OpenAI API registration invalid",
        ));
    }
    Ok(provider)
}

fn connection() -> Result<ConnectionDescriptor, ProviderError> {
    descriptor()?.connections.into_iter().next().ok_or_else(|| {
        error(
            ProviderErrorCode::Unavailable,
            "OpenAI API connection unavailable",
        )
    })
}

fn require_identity(provider: &str, connection: &str) -> Result<(), ProviderError> {
    if provider != PROVIDER || connection != CONNECTION {
        Err(error(
            ProviderErrorCode::NotFound,
            "provider connection not installed",
        ))
    } else {
        Ok(())
    }
}

struct Response {
    bytes: Vec<u8>,
    too_large: bool,
}
impl Handler for Response {
    fn write(&mut self, incoming: &[u8]) -> Result<usize, WriteError> {
        if self
            .bytes
            .len()
            .checked_add(incoming.len())
            .is_none_or(|size| size > MAX_BODY)
        {
            self.too_large = true;
            return Ok(0);
        }
        self.bytes.extend_from_slice(incoming);
        Ok(incoming.len())
    }
}

/// This is a verification probe, not a transport of model prompts. It never
/// forwards redirects or provider error bodies. Credential bytes do not enter
/// request URLs, journals, returned structures, or diagnostics.
fn fetch_models(
    secret: &phenix_provider_contracts::CredentialSecret,
) -> Result<Vec<String>, ProviderError> {
    let bytes = secret.expose_for_authorized_provider();
    if bytes.len() > 4096 || !bytes.iter().all(u8::is_ascii_graphic) {
        return Err(invalid("invalid OpenAI API key encoding"));
    }
    let token = std::str::from_utf8(bytes).map_err(|_| invalid("invalid API key encoding"))?;
    let mut easy = Easy2::new(Response {
        bytes: Vec::new(),
        too_large: false,
    });
    let configure = (|| -> Result<(), curl::Error> {
        easy.url(MODEL_ENDPOINT)?;
        easy.get(true)?;
        easy.follow_location(false)?;
        easy.connect_timeout(Duration::from_secs(4))?;
        easy.timeout(Duration::from_secs(10))?;
        easy.ssl_verify_peer(true)?;
        easy.ssl_verify_host(true)?;
        easy.signal(false)?;
        let mut headers = List::new();
        headers.append(&format!("Authorization: Bearer {token}"))?;
        headers.append("Accept: application/json")?;
        easy.http_headers(headers)?;
        Ok(())
    })();
    configure.map_err(|_| {
        error(
            ProviderErrorCode::Unavailable,
            "API verification unavailable",
        )
    })?;
    let outcome = easy.perform();
    let status = easy.response_code().unwrap_or(0);
    if status == 401 || status == 403 {
        return Err(error(
            ProviderErrorCode::AuthenticationRequired,
            "OpenAI API key rejected",
        ));
    }
    if status == 429 || status >= 500 {
        return Err(error(
            ProviderErrorCode::Unavailable,
            "OpenAI API verification unavailable",
        ));
    }
    if status != 200 {
        return Err(error(
            ProviderErrorCode::Denied,
            "OpenAI models request denied",
        ));
    }
    if easy.get_ref().too_large {
        return Err(error(
            ProviderErrorCode::Unavailable,
            "OpenAI model catalog too large",
        ));
    }
    outcome.map_err(|_| error(ProviderErrorCode::Unavailable, "OpenAI API request failed"))?;
    parse_model_ids(&easy.get_ref().bytes)
}

fn parse_model_ids(body: &[u8]) -> Result<Vec<String>, ProviderError> {
    if body.len() > MAX_BODY {
        return Err(error(
            ProviderErrorCode::Unavailable,
            "OpenAI model catalog too large",
        ));
    }
    let response: serde_json::Value = serde_json::from_slice(body).map_err(|_| {
        error(
            ProviderErrorCode::Unavailable,
            "invalid OpenAI model response",
        )
    })?;
    if response.get("object").and_then(serde_json::Value::as_str) != Some("list") {
        return Err(error(
            ProviderErrorCode::Unavailable,
            "unexpected OpenAI catalog shape",
        ));
    }
    let entries = response
        .get("data")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| error(ProviderErrorCode::Unavailable, "missing OpenAI models"))?;
    if entries.len() > MAX_MODELS {
        return Err(error(
            ProviderErrorCode::Unavailable,
            "too many OpenAI models",
        ));
    }
    let mut ids = BTreeSet::new();
    for entry in entries {
        let id = entry
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| error(ProviderErrorCode::Unavailable, "invalid OpenAI model ID"))?;
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(error(
                ProviderErrorCode::Unavailable,
                "invalid OpenAI model identity",
            ));
        }
        if !ids.insert(id.to_owned()) {
            return Err(error(
                ProviderErrorCode::Unavailable,
                "duplicate OpenAI model ID",
            ));
        }
    }
    Ok(ids.into_iter().collect())
}

/// Supported Responses families, rather than treating image, embedding,
/// moderation, speech and fine-tuning models as executable chat models.
/// Listing still does not guarantee account-specific inference quota.
fn supports_responses(model: &str) -> bool {
    ["gpt-4.1", "gpt-4o", "gpt-5", "gpt-6", "o1", "o3", "o4"]
        .iter()
        .any(|prefix| model == *prefix || model.starts_with(&format!("{prefix}-")))
}
fn revision(ids: &[String]) -> u64 {
    let hash = ids
        .iter()
        .flat_map(|name| name.bytes().chain([0u8]))
        .fold(14695981039346656037u64, |value, byte| {
            (value ^ u64::from(byte)).wrapping_mul(1099511628211)
        });
    hash.max(1)
}

#[phenix_sdk::plugin(
    id = "phenix.provider.openai-api",
    version = "1.0.0",
    profile = "state",
    provide(phenix_provider_contracts::ProviderCatalog, slot = "Catalog"),
    provide(phenix_provider_contracts::ProviderAuth, slot = "Authentication"),
    require(phenix_provider_contracts::ProviderCredentials, slot = "Credentials")
)]
mod plugin {
    use super::*;
    use phenix_provider_contracts::{
        ProviderAuth, ProviderCatalog, ProviderCredentials, ProviderDetailsQuery, ProviderPage,
        ProviderQuery,
    };
    use phenix_sdk::{CallError, CallResult, Host};

    #[derive(Default)]
    pub struct State {
        serial: u64,
        attempts: BTreeMap<String, Instant>,
    }

    fn check(host: &Host<'_>, scope: &str) -> CallResult<(), ProviderError> {
        if scope != "user" || host.resource()?.as_deref() != Some("phenix.providers.user") {
            return Err(CallError::Domain(error(
                ProviderErrorCode::Denied,
                "provider management scope unauthorized",
            )));
        }
        Ok(())
    }

    fn models(
        host: &Host<'_>,
        identity: &CredentialIdentity,
    ) -> CallResult<Vec<String>, ProviderError> {
        // The only authority is the exact launcher-granted credential resource.
        // A provider selector alone cannot authorize credential resolution.
        let scope = identity.resolve_scope().map_err(CallError::Domain)?;
        let credential =
            host.invoke_scoped::<ProviderCredentials::resolve>("Credentials", &scope, identity)?;
        fetch_models(&credential).map_err(CallError::Domain)
    }

    fn profile(host: &Host<'_>) -> CallResult<CredentialProfile, ProviderError> {
        let selected = selected_profile().map_err(CallError::Domain)?;
        let status = match models(host, &selected) {
            Ok(_) => CredentialProfileStatus::Authenticated,
            Err(_) => CredentialProfileStatus::Unavailable {
                reason: "provider verification failed or profile not authorized".into(),
            },
        };
        Ok(CredentialProfile {
            id: selected.profile_id().into(),
            label: selected.profile_id().into(),
            method_id: "api-key".into(),
            status,
        })
    }

    #[provide(ProviderCatalog::list)]
    fn list(
        _: &mut State,
        host: &Host<'_>,
        query: ProviderQuery,
    ) -> CallResult<ProviderPage, ProviderError> {
        check(host, &query.scope)?;
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
        check(host, &query.scope)?;
        if query.provider_id != PROVIDER {
            return Err(CallError::Domain(error(
                ProviderErrorCode::NotFound,
                "provider is not installed",
            )));
        }
        descriptor().map_err(CallError::Domain)
    }

    #[provide(ProviderCatalog::models)]
    fn model_page(
        _: &mut State,
        host: &Host<'_>,
        query: ModelQuery,
    ) -> CallResult<ModelPage, ProviderError> {
        check(host, &query.scope)?;
        require_identity(&query.provider_id, &query.connection_id).map_err(CallError::Domain)?;
        let selected = selected_profile().map_err(CallError::Domain)?;
        if query.profile_id.as_deref() != Some(selected.profile_id())
            || query.cursor.is_some()
            || query.limit == 0
            || query.limit > 256
        {
            return Err(CallError::Domain(invalid(
                "invalid authorized model catalog query",
            )));
        }
        let ids = models(host, &selected)?;
        let revision = revision(&ids);
        let models = ids
            .into_iter()
            .filter(|id| supports_responses(id))
            .map(|id| ProviderModel {
                label: id.clone(),
                id,
                connection_id: CONNECTION.into(),
                protocol_id: "openai-responses".into(),
                endpoint_id: "responses".into(),
                capabilities: Vec::new(),
                origin: ModelOrigin::Discovered,
                selectable: true,
            })
            .collect::<Vec<_>>();
        if models.len() > query.limit as usize {
            return Err(CallError::Domain(error(
                ProviderErrorCode::Unavailable,
                "authorized model catalog exceeds requested page budget",
            )));
        }
        Ok(ModelPage {
            profile_id: Some(selected.profile_id().into()),
            models,
            next_cursor: None,
            revision,
        })
    }

    #[provide(ProviderAuth::status)]
    fn status(
        _: &mut State,
        host: &Host<'_>,
        query: AuthQuery,
    ) -> CallResult<AuthenticationStatus, ProviderError> {
        check(host, &query.scope)?;
        require_identity(&query.provider_id, &query.connection_id).map_err(CallError::Domain)?;
        let profiles = match selected_profile() {
            Ok(_) => vec![profile(host)?],
            Err(_) => vec![],
        };
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
        check(host, &query.scope)?;
        require_identity(&query.provider_id, &query.connection_id).map_err(CallError::Domain)?;
        if query.method_id != "api-key" {
            return Err(CallError::Domain(invalid(
                "unavailable authentication method",
            )));
        }
        selected_profile().map_err(CallError::Domain)?;
        if state.attempts.len() >= 16 {
            return Err(CallError::Domain(error(
                ProviderErrorCode::Unavailable,
                "too many open authentication attempts",
            )));
        }
        state.serial = state.serial.checked_add(1).ok_or_else(|| {
            CallError::Domain(error(
                ProviderErrorCode::Unavailable,
                "auth attempt overflow",
            ))
        })?;
        let id = format!("openai-api-{}", state.serial);
        state.attempts.insert(id.clone(), Instant::now());
        Ok(AuthChallenge::ApiKeyInput {
            attempt_id: id,
            prompt: "Import API key into the launcher-authorized credential profile, then poll this attempt".into(),
        })
    }

    #[provide(ProviderAuth::poll)]
    fn poll(
        state: &mut State,
        host: &Host<'_>,
        query: AuthAttemptQuery,
    ) -> CallResult<AuthAttemptState, ProviderError> {
        check(host, &query.scope)?;
        require_identity(&query.provider_id, &query.connection_id).map_err(CallError::Domain)?;
        let started = *state.attempts.get(&query.attempt_id).ok_or_else(|| {
            CallError::Domain(error(ProviderErrorCode::NotFound, "auth attempt not found"))
        })?;
        if started.elapsed() > Duration::from_secs(600) {
            state.attempts.remove(&query.attempt_id);
            return Ok(AuthAttemptState::Expired);
        }
        let current = profile(host)?;
        match current.status {
            CredentialProfileStatus::Authenticated => {
                state.attempts.remove(&query.attempt_id);
                Ok(AuthAttemptState::Completed {
                    profile: AuthenticatedProfile {
                        id: current.id,
                        label: current.label,
                        method_id: current.method_id,
                    },
                })
            }
            CredentialProfileStatus::Unavailable { .. } => Ok(AuthAttemptState::Pending),
        }
    }

    #[provide(ProviderAuth::cancel)]
    fn cancel(
        state: &mut State,
        host: &Host<'_>,
        query: AuthAttemptQuery,
    ) -> CallResult<AuthAttemptState, ProviderError> {
        check(host, &query.scope)?;
        require_identity(&query.provider_id, &query.connection_id).map_err(CallError::Domain)?;
        if state.attempts.remove(&query.attempt_id).is_none() {
            return Err(CallError::Domain(error(
                ProviderErrorCode::NotFound,
                "auth attempt not found",
            )));
        }
        Ok(AuthAttemptState::Cancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_sdk::ContractMetadata;

    #[test]
    fn catalog_requires_real_ids_and_no_fake_entitlement() {
        let data = br#"{"object":"list","data":[{"id":"gpt-5"},{"id":"text-embedding-3-large"},{"id":"o3"}]}"#;
        assert_eq!(
            parse_model_ids(data).unwrap(),
            vec!["gpt-5", "o3", "text-embedding-3-large"],
        );
        assert!(supports_responses("gpt-5"));
        assert!(supports_responses("o3"));
        assert!(!supports_responses("text-embedding-3-large"));
        assert!(!supports_responses("dall-e-3"));
        let wrong = br#"{"object":"list","data":[{"id":"gpt-5"},{"id":"gpt-5"}]}"#;
        assert_eq!(
            parse_model_ids(wrong).unwrap_err().code,
            ProviderErrorCode::Unavailable,
        );
    }

    #[test]
    fn auth_provider_requires_separate_credential_slot() {
        let meta = ContractMetadata::decode(plugin::METADATA.bytes()).unwrap();
        assert_eq!(meta.provides.len(), 2);
        assert_eq!(meta.requires.len(), 1);
        assert_eq!(meta.requires[0].slot, "Credentials");
    }

    #[test]
    fn unsupported_identities_do_not_fall_through_to_another_provider() {
        assert_eq!(
            require_identity("anthropic", "api").unwrap_err().code,
            ProviderErrorCode::NotFound
        );
        assert_eq!(
            require_identity("openai", "chatgpt").unwrap_err().code,
            ProviderErrorCode::NotFound
        );
        let p = descriptor().unwrap();
        assert_eq!(p.connections.len(), 1);
        assert_eq!(p.connections[0].id, "api");
    }
}
