//! Provider-neutral account-bound model discovery execution.
//!
//! This is invoked only after the host checks that a profile-specific
//! credential resolution has been admitted by kernel authority. Model catalog
//! metadata never grants the authority to resolve a credential or invoke a
//! wire protocol. Unsupported formats fail closed.
use phenix_provider_builtins::discovered_model_page;
use phenix_provider_contracts::{
    ConnectionDescriptor, CredentialSecret, DiscoveryKind, ModelPage, ProviderError,
    ProviderErrorCode,
};
use phenix_provider_discovery::{
    http::{fetch_bearer_catalog, CatalogFetchError},
    parse_standard_openai_model_ids,
};

fn error(code: ProviderErrorCode, message: &str) -> ProviderError {
    ProviderError {
        code,
        message: message.into(),
    }
}

/// The exact declared model catalog adapter, not the provider ID, selects
/// supported parsing and model eligibility. A registered adapter name alone
/// never authorizes credential resolution.
pub fn can_discover(connection: &ConnectionDescriptor) -> bool {
    connection
        .model_sources
        .iter()
        .any(|source| {
            source.id == "models"
                && source.kind == DiscoveryKind::Dynamic
                && source.endpoint.is_some()
                && source.adapter.id == "openai.models"
        })
        && connection
            .protocols
            .iter()
            .any(|protocol| protocol.id == "openai-responses" && protocol.endpoint.is_some())
}

fn response_eligible(model: &str) -> bool {
    // A model being in /models is not proof that it supports Responses.
    // Keep the conservative known-family rule of the existing API plugin,
    // until an explicit per-model capability strategy replaces it.
    ["gpt-4.1", "gpt-4o", "gpt-5", "gpt-6", "o1", "o3", "o4"]
        .iter()
        .any(|prefix| model == *prefix || model.starts_with(&format!("{prefix}-")))
}

pub fn discover(
    connection: &ConnectionDescriptor,
    profile: &str,
    secret: &CredentialSecret,
    limit: u64,
) -> Result<ModelPage, ProviderError> {
    if !can_discover(connection) {
        return Err(error(
            ProviderErrorCode::Unavailable,
            "selected connection does not have a supported discovery strategy",
        ));
    }
    let Some(source) = connection.model_sources.iter().find(|item| {
        item.id == "models"
            && item.kind == DiscoveryKind::Dynamic
            && item.adapter.id == "openai.models"
    }) else {
        return Err(error(ProviderErrorCode::Unavailable, "model adapter unavailable"));
    };
    let endpoint = source
        .endpoint
        .as_deref()
        .ok_or_else(|| error(ProviderErrorCode::Unavailable, "model endpoint unavailable"))?;
    let bytes = secret.expose_for_authorized_provider();
    let bearer = std::str::from_utf8(bytes)
        .map_err(|_| error(ProviderErrorCode::InvalidInput, "invalid API key encoding"))?;
    let catalog = fetch_bearer_catalog(endpoint, bearer).map_err(|reason| match reason {
        CatalogFetchError::Unauthorized => error(
            ProviderErrorCode::AuthenticationRequired,
            "selected API key was rejected",
        ),
        CatalogFetchError::Rejected | CatalogFetchError::UnsafeEndpoint => {
            error(ProviderErrorCode::Denied, "model catalog access denied")
        }
        CatalogFetchError::InvalidBearer => {
            error(ProviderErrorCode::InvalidInput, "invalid API key encoding")
        }
        CatalogFetchError::RateLimited
        | CatalogFetchError::Unavailable
        | CatalogFetchError::TooLarge => {
            error(ProviderErrorCode::Unavailable, "model catalog unavailable")
        }
    })?;
    let ids = parse_standard_openai_model_ids(&catalog)
        .map_err(|_| error(ProviderErrorCode::Unavailable, "invalid model catalog response"))?;
    discovered_model_page(
        profile,
        &connection.id,
        "openai-responses",
        "responses",
        limit,
        ids.into_iter().map(|id| (id.clone(), id)),
        response_eligible,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_provider_builtins::registrations;

    #[test]
    fn catalog_requires_exact_standard_adapter_and_protocol() {
        let all = registrations();
        let mut api = all[0].connections[0].clone();
        assert!(can_discover(&api));
        api.protocols.clear();
        assert!(!can_discover(&api));
        api = all[0].connections[0].clone();
        api.model_sources[0].adapter.id = "uninstalled.models".into();
        assert!(!can_discover(&api));
        api = all[0].connections[1].clone();
        assert!(!can_discover(&api));
    }

    #[test]
    fn bearer_secret_is_not_leaked_for_unavailable_adapters() {
        let all = registrations();
        let secret = CredentialSecret::new(b"confidential-secret".to_vec()).unwrap();
        let invalid = &all[0].connections[1];
        let err = discover(invalid, "account", &secret, 20).unwrap_err();
        assert_eq!(err.code, ProviderErrorCode::Unavailable);
        assert!(!err.message.contains("confidential"));
    }

    #[test]
    fn responses_eligibility_is_conservative() {
        for allowed in ["gpt-4.1", "gpt-4o-mini", "gpt-5", "gpt-6", "o3-mini"] {
            assert!(response_eligible(allowed));
        }
        for denied in ["whisper-1", "text-embedding-3-large", "gpt-50", "o4not"] {
            assert!(!response_eligible(denied));
        }
    }
}
