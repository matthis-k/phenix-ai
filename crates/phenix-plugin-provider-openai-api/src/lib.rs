//! Opt-in OpenAI API-key provider implementation.
//!
//! Registration does not imply authentication. Every profile status and model
//! query resolves the one launcher-authorized credential and verifies it over
//! the explicitly documented HTTPS /v1/models endpoint. The kernel does not
//! know the provider or manage its secrets. This plugin does not implement
//! subscription OAuth, plaintext storage, or arbitrary endpoint configuration.
use phenix_provider_auth_strategies::ApiKeyAttempts;
use phenix_provider_builtins::{discovered_model_page, registrations};
use phenix_provider_contracts::{
    AuthAttemptQuery, AuthAttemptState, AuthChallenge, AuthQuery, AuthStart, AuthenticatedProfile,
    AuthenticationStatus, CredentialIdentity, CredentialProfile, CredentialProfileStatus,
    ModelPage, ModelQuery, ProviderDescriptor, ProviderError, ProviderErrorCode,
};
use phenix_provider_discovery::{
    http::{fetch_bearer_catalog, CatalogFetchError},
    parse_standard_openai_model_ids,
};

const PROVIDER: &str = "openai";
const CONNECTION: &str = "api";
const MODEL_ENDPOINT: &str = "https://api.openai.com/v1/models";

#[cfg(feature = "c11-loopback-test")]
fn test_base() -> Result<String, ProviderError> {
    let source = std::env::var("PHENIX_C11_TEST_BASE").map_err(|_| {
        error(
            ProviderErrorCode::Unavailable,
            "test provider endpoint unset",
        )
    })?;
    let port = source
        .strip_prefix("http://127.0.0.1:")
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|text| text.parse::<u16>().ok())
        .filter(|port| *port != 0)
        .ok_or_else(|| error(ProviderErrorCode::Denied, "test endpoint must be loopback"))?;
    Ok(format!("http://127.0.0.1:{port}"))
}

fn models_url() -> Result<String, ProviderError> {
    #[cfg(feature = "c11-loopback-test")]
    {
        return Ok(format!("{}/models", test_base()?));
    }
    #[cfg(not(feature = "c11-loopback-test"))]
    {
        Ok(MODEL_ENDPOINT.into())
    }
}

fn error(code: ProviderErrorCode, message: &str) -> ProviderError {
    ProviderError {
        code,
        message: message.into(),
    }
}
fn invalid(message: &str) -> ProviderError {
    error(ProviderErrorCode::InvalidInput, message)
}

/// The launcher's exact root, not process-global configuration, selects an
/// account. A configured profile is only an additional restriction, never a
/// fallback that could redirect credential resolution to a sibling account.
fn selected_profile_from_resource(
    resource: Option<&str>,
    configured: Option<&str>,
) -> Result<CredentialIdentity, ProviderError> {
    let raw = resource.ok_or_else(|| {
        error(
            ProviderErrorCode::Denied,
            "OpenAI API credential root is not authorized",
        )
    })?;
    let identity = CredentialIdentity::from_resource(raw).map_err(|_| {
        error(
            ProviderErrorCode::Denied,
            "invalid provider credential root",
        )
    })?;
    require_identity(identity.provider_id(), identity.connection_id())?;
    if configured.is_some_and(|expected| expected != identity.profile_id()) {
        return Err(error(
            ProviderErrorCode::Denied,
            "configured provider profile does not match authorized credential root",
        ));
    }
    Ok(identity)
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
    #[cfg(feature = "c11-loopback-test")]
    {
        let base = test_base()?;
        let c = &mut provider.connections[0];
        c.base_url = Some(base.clone());
        for protocol in &mut c.protocols {
            if protocol.id == "openai-responses" {
                protocol.endpoint = Some(format!("{base}/responses"));
            }
        }
        for source in &mut c.model_sources {
            source.endpoint = Some(format!("{base}/models"));
        }
    }
    Ok(provider)
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

/// Reuse the generic, bounded Bearer catalog strategy. The provider's only
/// specializations are its published endpoint, account authorization and
/// Responses-model selection convention.
fn fetch_models(
    secret: &phenix_provider_contracts::CredentialSecret,
) -> Result<Vec<String>, ProviderError> {
    let bytes = secret.expose_for_authorized_provider();
    if bytes.len() > 4096 || !bytes.iter().all(u8::is_ascii_graphic) {
        return Err(invalid("invalid OpenAI API key encoding"));
    }
    let token = std::str::from_utf8(bytes).map_err(|_| invalid("invalid API key encoding"))?;
    let body = fetch_bearer_catalog(&models_url()?, token).map_err(|problem| match problem {
        CatalogFetchError::Unauthorized => error(
            ProviderErrorCode::AuthenticationRequired,
            "OpenAI API key rejected",
        ),
        CatalogFetchError::RateLimited | CatalogFetchError::Unavailable => error(
            ProviderErrorCode::Unavailable,
            "OpenAI API verification unavailable",
        ),
        CatalogFetchError::Rejected => {
            error(ProviderErrorCode::Denied, "OpenAI models request denied")
        }
        CatalogFetchError::TooLarge => error(
            ProviderErrorCode::Unavailable,
            "OpenAI model catalog too large",
        ),
        CatalogFetchError::InvalidBearer => invalid("invalid OpenAI API key encoding"),
        CatalogFetchError::UnsafeEndpoint => error(
            ProviderErrorCode::Denied,
            "invalid OpenAI model catalog endpoint",
        ),
    })?;
    parse_model_ids(&body)
}

fn parse_model_ids(body: &[u8]) -> Result<Vec<String>, ProviderError> {
    parse_standard_openai_model_ids(body).map_err(|_| {
        error(
            ProviderErrorCode::Unavailable,
            "invalid OpenAI model response",
        )
    })
}

/// Supported Responses families, rather than treating image, embedding,
/// moderation, speech and fine-tuning models as executable chat models.
/// Listing still does not guarantee account-specific inference quota.
fn supports_responses(model: &str) -> bool {
    ["gpt-4.1", "gpt-4o", "gpt-5", "gpt-6", "o1", "o3", "o4"]
        .iter()
        .any(|prefix| model == *prefix || model.starts_with(&format!("{prefix}-")))
}
#[phenix_sdk::plugin(
    id = "phenix.provider.openai-api",
    version = "1.0.0",
    profile = "state",
    provide(phenix_provider_contracts::ProviderCatalog, slot = "Catalog"),
    provide(
        phenix_provider_contracts::ProviderRegistrations,
        slot = "Registrations"
    ),
    provide(phenix_provider_contracts::ProviderAuth, slot = "Authentication"),
    require(phenix_provider_contracts::ProviderCredentials, slot = "Credentials")
)]
mod plugin {
    use super::*;
    use phenix_provider_contracts::{
        ProviderAuth, ProviderCatalog, ProviderCredentials, ProviderDetailsQuery, ProviderPage,
        ProviderQuery, ProviderRegistrations,
    };
    use phenix_sdk::{CallError, CallResult, Host};

    #[derive(Default)]
    pub struct State {
        attempts: ApiKeyAttempts,
    }

    fn scoped_profile(host: &Host<'_>) -> CallResult<CredentialIdentity, ProviderError> {
        // The deployment may restrict its provider instance to a profile.
        // Absent that restriction, the kernel-pinned credential resource is
        // sufficient and cannot be broadened by an ACP account selector.
        let configured = match std::env::var("PHENIX_OPENAI_API_PROFILE") {
            Ok(value) => Some(value),
            Err(std::env::VarError::NotPresent) => None,
            Err(_) => {
                return Err(CallError::Domain(error(
                    ProviderErrorCode::Denied,
                    "invalid provider profile configuration",
                )))
            }
        };
        selected_profile_from_resource(host.resource()?.as_deref(), configured.as_deref())
            .map_err(CallError::Domain)
    }

    fn check(host: &Host<'_>, scope: &str) -> CallResult<(), ProviderError> {
        // "user" is a selector, never an authority grant. The configured
        // composition pins an exact credential resource for this invocation.
        if scope != "user" {
            return Err(CallError::Domain(error(
                ProviderErrorCode::Denied,
                "provider management scope unauthorized",
            )));
        }
        scoped_profile(host)?;
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
        let selected = scoped_profile(host)?;
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

    /// A concrete provider contributes only its declaration to the manager.
    /// No credential or management authority is required to enumerate it.
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
        let selected = scoped_profile(host)?;
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
        discovered_model_page(
            selected.profile_id(),
            CONNECTION,
            "openai-responses",
            "responses",
            query.limit,
            ids.into_iter().map(|id| (id.clone(), id)),
            supports_responses,
        )
        .map_err(CallError::Domain)
    }

    #[provide(ProviderAuth::status)]
    fn status(
        _: &mut State,
        host: &Host<'_>,
        query: AuthQuery,
    ) -> CallResult<AuthenticationStatus, ProviderError> {
        check(host, &query.scope)?;
        require_identity(&query.provider_id, &query.connection_id).map_err(CallError::Domain)?;
        let profiles = vec![profile(host)?];
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
        let identity = scoped_profile(host)?;
        state
            .attempts
            .begin(
                identity,
                "openai-api",
                "Import API key into the launcher-authorized credential profile, then poll this attempt",
            )
            .map_err(CallError::Domain)
    }

    #[provide(ProviderAuth::poll)]
    fn poll(
        state: &mut State,
        host: &Host<'_>,
        query: AuthAttemptQuery,
    ) -> CallResult<AuthAttemptState, ProviderError> {
        check(host, &query.scope)?;
        require_identity(&query.provider_id, &query.connection_id).map_err(CallError::Domain)?;
        let identity = scoped_profile(host)?;
        if let Some(terminal) = state
            .attempts
            .check(&identity, &query.attempt_id)
            .map_err(CallError::Domain)?
        {
            return Ok(terminal);
        }
        let current = profile(host)?;
        match current.status {
            CredentialProfileStatus::Authenticated => state
                .attempts
                .complete(
                    &identity,
                    &query.attempt_id,
                    AuthenticatedProfile {
                        id: current.id,
                        label: current.label,
                        method_id: current.method_id,
                    },
                )
                .map_err(CallError::Domain),
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
        let identity = scoped_profile(host)?;
        state
            .attempts
            .cancel(&identity, &query.attempt_id)
            .map_err(CallError::Domain)
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
        // Account catalog may contain non-chat fine-tune IDs with colons.
        // They must not invalidate an otherwise verified profile.
        assert_eq!(
            parse_model_ids(br#"{"object":"list","data":[{"id":"ft:gpt-4.1:team:run"}]}"#).unwrap(),
            vec!["ft:gpt-4.1:team:run"],
        );
        assert_eq!(
            parse_model_ids(br#"{"data":[{"id":"gpt-5"}]}"#)
                .unwrap_err()
                .code,
            ProviderErrorCode::Unavailable,
        );
        assert_eq!(
            parse_model_ids(br#"{"object":"list","data":[{"id":"bad model"}]}"#)
                .unwrap_err()
                .code,
            ProviderErrorCode::Unavailable,
        );
        let wrong = br#"{"object":"list","data":[{"id":"gpt-5"},{"id":"gpt-5"}]}"#;
        assert_eq!(
            parse_model_ids(wrong).unwrap_err().code,
            ProviderErrorCode::Unavailable,
        );
    }

    #[test]
    fn exact_root_selects_named_account_without_implicit_environment_fallback() {
        let personal = CredentialIdentity::new(PROVIDER, CONNECTION, "personal").unwrap();
        let work = CredentialIdentity::new(PROVIDER, CONNECTION, "work").unwrap();
        let personal_resource = personal.resource().unwrap();
        let work_resource = work.resource().unwrap();
        assert_eq!(
            selected_profile_from_resource(Some(&work_resource), None).unwrap(),
            work
        );
        assert_eq!(
            selected_profile_from_resource(Some(&personal_resource), Some("personal")).unwrap(),
            personal
        );
        assert_eq!(
            selected_profile_from_resource(Some(&work_resource), Some("personal"))
                .unwrap_err()
                .code,
            ProviderErrorCode::Denied
        );
        assert_eq!(
            selected_profile_from_resource(None, Some("work"))
                .unwrap_err()
                .code,
            ProviderErrorCode::Denied
        );
        let foreign = CredentialIdentity::new("anthropic", CONNECTION, "work").unwrap();
        assert_eq!(
            selected_profile_from_resource(Some(&foreign.resource().unwrap()), None)
                .unwrap_err()
                .code,
            ProviderErrorCode::NotFound
        );
        assert_eq!(
            selected_profile_from_resource(Some("phenix.credentials.malformed"), None)
                .unwrap_err()
                .code,
            ProviderErrorCode::Denied
        );
    }

    #[test]
    fn auth_provider_requires_separate_credential_slot() {
        let meta = ContractMetadata::decode(plugin::METADATA.bytes()).unwrap();
        assert_eq!(meta.provides.len(), 3);
        assert_eq!(meta.requires.len(), 1);
        assert_eq!(meta.requires[0].slot, "Credentials");
    }

    #[cfg(not(feature = "c11-loopback-test"))]
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
