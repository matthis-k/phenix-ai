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
    parse_standard_openai_model_ids, standard_openai_responses_model,
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
    connection.model_sources.iter().any(|source| {
        source.id == "models"
            && source.kind == DiscoveryKind::Dynamic
            && source.endpoint.is_some()
            && source.adapter.id == "openai.models"
    }) && connection
        .protocols
        .iter()
        .any(|protocol| protocol.id == "openai-responses" && protocol.endpoint.is_some())
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
        return Err(error(
            ProviderErrorCode::Unavailable,
            "model adapter unavailable",
        ));
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
    let ids = parse_standard_openai_model_ids(&catalog).map_err(|_| {
        error(
            ProviderErrorCode::Unavailable,
            "invalid model catalog response",
        )
    })?;
    discovered_model_page(
        profile,
        &connection.id,
        "openai-responses",
        "responses",
        limit,
        ids.into_iter().map(|id| (id.clone(), id)),
        standard_openai_responses_model,
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
    fn admitted_model_catalog_uses_bounded_transport_and_never_promotes_embeddings() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            thread,
            time::Duration,
        };
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://127.0.0.1:{}/models",
            server.local_addr().unwrap().port()
        );
        let service = thread::spawn(move || {
            let (mut peer, _) = server.accept().unwrap();
            peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut raw = [0u8; 8192];
            let size = peer.read(&mut raw).unwrap();
            let received = String::from_utf8_lossy(&raw[..size]).to_string();
            let payload = r#"{"object":"list","data":[{"id":"text-embedding-3-large"},{"id":"gpt-5"},{"id":"gpt-4o-mini"}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                payload.len(),
                payload
            );
            peer.write_all(response.as_bytes()).unwrap();
            received
        });
        let mut connection = registrations()[0].connections[0].clone();
        connection.model_sources[0].endpoint = Some(url.clone());
        connection.base_url = Some(url.trim_end_matches("/models").to_owned());
        connection.protocols[0].endpoint =
            Some(url.trim_end_matches("/models").to_owned() + "/responses");
        connection.protocols[1].endpoint =
            Some(url.trim_end_matches("/models").to_owned() + "/chat/completions");
        let secret = CredentialSecret::new(b"private-test-token".to_vec()).unwrap();
        let page = discover(&connection, "personal", &secret, 20).unwrap();
        assert_eq!(page.profile_id.as_deref(), Some("personal"));
        assert_eq!(
            page.models
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            vec!["gpt-4o-mini", "gpt-5"]
        );
        assert!(page
            .models
            .iter()
            .all(|m| m.protocol_id == "openai-responses"));
        let request = service.join().unwrap();
        assert!(request.starts_with("GET /models HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer private-test-token"));
    }

    #[test]
    fn responses_eligibility_is_conservative() {
        for allowed in ["gpt-4.1", "gpt-4o-mini", "gpt-5", "gpt-6", "o3-mini"] {
            assert!(standard_openai_responses_model(allowed));
        }
        for denied in ["whisper-1", "text-embedding-3-large", "gpt-50", "o4not"] {
            assert!(!standard_openai_responses_model(denied));
        }
    }
}
