//! Common provider registrations without secrets, runtime policy or network I/O.
//!
//! A provider plugin may reuse these descriptions, override an installed
//! connection, or supply a different provider entirely. The runtime must
//! revalidate and authorize every selected account and model.

use phenix_provider_contracts::{
    AuthenticationKind, AuthenticationMethod, ConnectionDescriptor,
    DiscoveryKind, DiscoverySource, ProviderDescriptor,
};
use phenix_provider_protocols::{validate_endpoint, EndpointMode};
use std::collections::BTreeSet;

pub const OPENAI_CHAT: &str = "openai-chat";
pub const OPENAI_RESPONSES: &str = "openai-responses";
pub const ANTHROPIC_MESSAGES: &str = "anthropic-messages";

fn method(id: &str, label: &str, kind: AuthenticationKind) -> AuthenticationMethod {
    AuthenticationMethod {
        id: id.into(),
        label: label.into(),
        kind,
    }
}

fn api_key() -> Vec<AuthenticationMethod> {
    vec![method("api-key", "API key", AuthenticationKind::ApiKey)]
}

fn source(endpoint: Option<&str>, id: &str) -> DiscoverySource {
    DiscoverySource {
        id: id.into(),
        kind: if endpoint.is_some() { DiscoveryKind::Dynamic } else { DiscoveryKind::Declared },
        endpoint: endpoint.map(str::to_string),
    }
}

fn connection(
    id: &str,
    label: &str,
    base: &str,
    protocols: &[&str],
    methods: Vec<AuthenticationMethod>,
    discovery: Option<&str>,
) -> ConnectionDescriptor {
    ConnectionDescriptor {
        id: id.into(),
        label: label.into(),
        protocols: protocols.iter().map(|id| (*id).into()).collect(),
        base_url: Some(base.into()),
        auth_methods: methods,
        model_sources: vec![source(discovery, "models")],
    }
}

fn provider(id: &str, label: &str, connections: Vec<ConnectionDescriptor>) -> ProviderDescriptor {
    ProviderDescriptor { id: id.into(), label: label.into(), connections }
}

/// Registration is offline and does not imply account entitlement or model
/// availability. OAuth is only usable after provider-approved authorization.
pub fn registrations() -> Vec<ProviderDescriptor> {
    vec![
        provider(
            "openai",
            "OpenAI",
            vec![
                connection(
                    "api",
                    "OpenAI API",
                    "https://api.openai.com/v1",
                    &[OPENAI_RESPONSES, OPENAI_CHAT],
                    api_key(),
                    Some("https://api.openai.com/v1/models"),
                ),
                connection(
                    "chatgpt",
                    "ChatGPT subscription",
                    "https://api.openai.com/v1",
                    &[OPENAI_RESPONSES],
                    vec![method(
                        "chatgpt-sign-in",
                        "Sign in with ChatGPT",
                        AuthenticationKind::OAuthAuthorizationCode,
                    )],
                    // This OAuth catalog has a separate schema from the API-key list.
                    Some("https://api.openai.com/v1/models"),
                ),
            ],
        ),
        provider(
            "openrouter",
            "OpenRouter",
            vec![connection(
                "api",
                "OpenRouter API",
                "https://openrouter.ai/api/v1",
                &[OPENAI_CHAT, OPENAI_RESPONSES],
                api_key(),
                Some("https://openrouter.ai/api/v1/models"),
            )],
        ),
        provider(
            "anthropic",
            "Anthropic",
            vec![connection(
                "api",
                "Anthropic API",
                "https://api.anthropic.com/v1",
                &[ANTHROPIC_MESSAGES],
                api_key(),
                Some("https://api.anthropic.com/v1/models"),
            )],
        ),
        provider(
            "opencode-zen",
            "OpenCode Zen",
            vec![connection(
                "api",
                "OpenCode Zen",
                "https://opencode.ai/zen/v1",
                &[OPENAI_CHAT, OPENAI_RESPONSES, ANTHROPIC_MESSAGES],
                api_key(),
                Some("https://opencode.ai/zen/v1/models"),
            )],
        ),
        provider(
            "opencode-go",
            "OpenCode Go",
            vec![connection(
                "api",
                "OpenCode Go",
                "https://opencode.ai/zen/go/v1",
                &[OPENAI_CHAT, OPENAI_RESPONSES, ANTHROPIC_MESSAGES],
                api_key(),
                Some("https://opencode.ai/zen/go/v1/models"),
            )],
        ),
        provider(
            "zai",
            "Z.ai",
            vec![connection(
                "standard",
                "Z.ai API",
                "https://api.z.ai/api/paas/v4",
                &[OPENAI_CHAT],
                api_key(),
                // Model names depend on account/plan. A provider plugin must
                // supply a declared list or verified live discovery.
                None,
            )],
        ),
    ]
}

/// Defensive startup check for static registrations. Dynamic provider plugins
/// must receive the same runtime validation before installation.
pub fn validate_registrations(providers: &[ProviderDescriptor]) -> Result<(), &'static str> {
    let mut seen_providers = BTreeSet::new();
    for provider in providers {
        if !seen_providers.insert(provider.id.as_str()) || provider.id.is_empty() {
            return Err("duplicate or empty provider identity");
        }
        let mut seen_connections = BTreeSet::new();
        for connection in &provider.connections {
            if !seen_connections.insert(connection.id.as_str()) || connection.protocols.is_empty() {
                return Err("invalid provider connection");
            }
            let base = connection.base_url.as_deref().ok_or("missing provider endpoint")?;
            validate_endpoint(base, EndpointMode::Remote).map_err(|_| "unsafe provider endpoint")?;
            let mut method_ids = BTreeSet::new();
            for method in &connection.auth_methods {
                if !method_ids.insert(method.id.as_str()) || method.id.is_empty() {
                    return Err("duplicate authentication method");
                }
            }
            for source in &connection.model_sources {
                if let Some(endpoint) = &source.endpoint {
                    validate_endpoint(endpoint, EndpointMode::Remote)
                        .map_err(|_| "unsafe catalog endpoint")?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_groups_and_distinct_api_and_chatgpt_accounts() {
        let catalog = registrations();
        validate_registrations(&catalog).unwrap();
        assert_eq!(catalog.len(), 6);
        let openai = &catalog[0];
        assert_eq!(openai.id, "openai");
        assert_eq!(openai.connections.len(), 2);
        assert_ne!(openai.connections[0].id, openai.connections[1].id);
        assert_eq!(
            openai.connections[0].auth_methods[0].kind,
            AuthenticationKind::ApiKey
        );
        assert_eq!(
            openai.connections[1].auth_methods[0].kind,
            AuthenticationKind::OAuthAuthorizationCode
        );
        assert_eq!(openai.connections[1].protocols, vec![OPENAI_RESPONSES]);
    }

    #[test]
    fn each_provider_uses_an_explicit_protocol_and_safe_endpoint() {
        let all = registrations();
        assert!(all.iter().all(|provider| provider.connections.iter().all(
            |connection| !connection.protocols.is_empty() && connection.base_url.is_some()
        )));
        assert_eq!(
            all.iter().find(|provider| provider.id == "anthropic").unwrap()
                .connections[0].protocols,
            vec![ANTHROPIC_MESSAGES]
        );
        validate_registrations(&all).unwrap();
    }

    #[test]
    fn reject_duplicate_provider_and_unsafe_registered_endpoint() {
        let mut providers = registrations();
        providers.push(providers[0].clone());
        assert_eq!(validate_registrations(&providers), Err("duplicate or empty provider identity"));
        let mut providers = registrations();
        providers[0].connections[0].base_url = Some("http://external.test/v1".into());
        assert_eq!(validate_registrations(&providers), Err("unsafe provider endpoint"));
    }

    #[test]
    fn registration_never_serializes_credentials() {
        let providers = registrations();
        let json = format!("{providers:?}");
        assert!(!json.contains("secret="));
        assert!(providers.iter().all(|provider| provider.connections.iter().all(
            |connection| connection.auth_methods.iter().all(|method| !method.id.is_empty())
        )));
    }
}
