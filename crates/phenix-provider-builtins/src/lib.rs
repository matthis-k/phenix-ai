//! Common provider registrations without secrets, runtime policy or network I/O.
//!
//! A provider plugin may reuse these descriptions, override an installed
//! connection, or supply a different provider entirely. The runtime must
//! revalidate and authorize every selected account and model.

use phenix_provider_contracts::{
    AuthenticationKind, AuthenticationMethod, ConnectionDescriptor, DiscoveryKind, DiscoverySource,
    ProtocolBinding, ProviderDescriptor,
};
use phenix_provider_protocols::{same_credential_origin, validate_endpoint, EndpointMode};
use std::collections::BTreeSet;

pub const OPENAI_CHAT: &str = "openai-chat";
pub const OPENAI_RESPONSES: &str = "openai-responses";
pub const ANTHROPIC_MESSAGES: &str = "anthropic-messages";

#[cfg(test)]
fn method(id: &str, label: &str, kind: AuthenticationKind) -> AuthenticationMethod {
    AuthenticationMethod {
        id: id.into(),
        label: label.into(),
        kind,
    }
}

#[cfg(test)]
fn api_key() -> Vec<AuthenticationMethod> {
    vec![method("api-key", "API key", AuthenticationKind::ApiKey)]
}

#[cfg(test)]
fn source(endpoint: Option<&str>, id: &str) -> DiscoverySource {
    DiscoverySource {
        id: id.into(),
        kind: if endpoint.is_some() {
            DiscoveryKind::Dynamic
        } else {
            DiscoveryKind::Declared
        },
        endpoint: endpoint.map(str::to_string),
    }
}

#[cfg(test)]
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
        protocols: protocols
            .iter()
            .map(|id| ProtocolBinding {
                id: (*id).into(),
                endpoint: None,
            })
            .collect(),
        base_url: Some(base.into()),
        auth_methods: methods,
        model_sources: vec![source(discovery, "models")],
    }
}

#[cfg(test)]
fn provider(id: &str, label: &str, connections: Vec<ConnectionDescriptor>) -> ProviderDescriptor {
    ProviderDescriptor {
        id: id.into(),
        label: label.into(),
        connections,
    }
}

/// Registration is offline and does not imply account entitlement or model
/// availability. OAuth is only usable after provider-approved authorization.
pub fn registrations() -> Vec<ProviderDescriptor> {
    vec![
        ProviderDescriptor {
            id: "openai".into(),
            label: "OpenAI".into(),
            connections: vec![
                ConnectionDescriptor {
                    id: "api".into(),
                    label: "OpenAI API".into(),
                    protocols: vec![
                        ProtocolBinding {
                            id: OPENAI_RESPONSES.into(),
                            endpoint: None,
                        },
                        ProtocolBinding {
                            id: OPENAI_CHAT.into(),
                            endpoint: None,
                        },
                    ],
                    base_url: Some("https://api.openai.com/v1".into()),
                    auth_methods: vec![AuthenticationMethod {
                        id: "api-key".into(),
                        label: "API key".into(),
                        kind: AuthenticationKind::ApiKey,
                    }],
                    model_sources: vec![DiscoverySource {
                        id: "models".into(),
                        kind: DiscoveryKind::Dynamic,
                        endpoint: Some("https://api.openai.com/v1/models".into()),
                    }],
                },
                ConnectionDescriptor {
                    id: "chatgpt".into(),
                    label: "ChatGPT subscription".into(),
                    protocols: vec![ProtocolBinding {
                        id: OPENAI_RESPONSES.into(),
                        endpoint: None,
                    }],
                    base_url: Some("https://api.openai.com/v1".into()),
                    auth_methods: vec![AuthenticationMethod {
                        id: "chatgpt-sign-in".into(),
                        label: "Sign in with ChatGPT".into(),
                        kind: AuthenticationKind::OAuthAuthorizationCode,
                    }],
                    model_sources: vec![DiscoverySource {
                        id: "models".into(),
                        kind: DiscoveryKind::Dynamic,
                        endpoint: Some("https://api.openai.com/v1/models".into()),
                    }],
                },
            ],
        },
        ProviderDescriptor {
            id: "openrouter".into(),
            label: "OpenRouter".into(),
            connections: vec![ConnectionDescriptor {
                id: "api".into(),
                label: "OpenRouter API".into(),
                protocols: vec![
                    ProtocolBinding {
                        id: OPENAI_CHAT.into(),
                        endpoint: None,
                    },
                    ProtocolBinding {
                        id: OPENAI_RESPONSES.into(),
                        endpoint: None,
                    },
                ],
                base_url: Some("https://openrouter.ai/api/v1".into()),
                auth_methods: vec![AuthenticationMethod {
                    id: "api-key".into(),
                    label: "API key".into(),
                    kind: AuthenticationKind::ApiKey,
                }],
                model_sources: vec![DiscoverySource {
                    id: "models".into(),
                    kind: DiscoveryKind::Dynamic,
                    endpoint: Some("https://openrouter.ai/api/v1/models".into()),
                }],
            }],
        },
        ProviderDescriptor {
            id: "anthropic".into(),
            label: "Anthropic".into(),
            connections: vec![ConnectionDescriptor {
                id: "api".into(),
                label: "Anthropic API".into(),
                protocols: vec![ProtocolBinding {
                    id: ANTHROPIC_MESSAGES.into(),
                    endpoint: None,
                }],
                base_url: Some("https://api.anthropic.com/v1".into()),
                auth_methods: vec![AuthenticationMethod {
                    id: "api-key".into(),
                    label: "API key".into(),
                    kind: AuthenticationKind::ApiKey,
                }],
                model_sources: vec![DiscoverySource {
                    id: "models".into(),
                    kind: DiscoveryKind::Dynamic,
                    endpoint: Some("https://api.anthropic.com/v1/models".into()),
                }],
            }],
        },
        ProviderDescriptor {
            id: "opencode-zen".into(),
            label: "OpenCode Zen".into(),
            connections: vec![ConnectionDescriptor {
                id: "api".into(),
                label: "OpenCode Zen".into(),
                protocols: vec![
                    ProtocolBinding {
                        id: OPENAI_CHAT.into(),
                        endpoint: None,
                    },
                    ProtocolBinding {
                        id: OPENAI_RESPONSES.into(),
                        endpoint: None,
                    },
                    ProtocolBinding {
                        id: ANTHROPIC_MESSAGES.into(),
                        endpoint: None,
                    },
                ],
                base_url: Some("https://opencode.ai/zen/v1".into()),
                auth_methods: vec![AuthenticationMethod {
                    id: "api-key".into(),
                    label: "API key".into(),
                    kind: AuthenticationKind::ApiKey,
                }],
                model_sources: vec![DiscoverySource {
                    id: "models".into(),
                    kind: DiscoveryKind::Dynamic,
                    endpoint: Some("https://opencode.ai/zen/v1/models".into()),
                }],
            }],
        },
        ProviderDescriptor {
            id: "opencode-go".into(),
            label: "OpenCode Go".into(),
            connections: vec![ConnectionDescriptor {
                id: "api".into(),
                label: "OpenCode Go".into(),
                protocols: vec![
                    ProtocolBinding {
                        id: OPENAI_CHAT.into(),
                        endpoint: None,
                    },
                    ProtocolBinding {
                        id: OPENAI_RESPONSES.into(),
                        endpoint: None,
                    },
                    ProtocolBinding {
                        id: ANTHROPIC_MESSAGES.into(),
                        endpoint: None,
                    },
                ],
                base_url: Some("https://opencode.ai/zen/go/v1".into()),
                auth_methods: vec![AuthenticationMethod {
                    id: "api-key".into(),
                    label: "API key".into(),
                    kind: AuthenticationKind::ApiKey,
                }],
                model_sources: vec![DiscoverySource {
                    id: "models".into(),
                    kind: DiscoveryKind::Dynamic,
                    endpoint: Some("https://opencode.ai/zen/go/v1/models".into()),
                }],
            }],
        },
        ProviderDescriptor {
            id: "zai".into(),
            label: "Z.ai".into(),
            connections: vec![ConnectionDescriptor {
                id: "standard".into(),
                label: "Z.ai API".into(),
                protocols: vec![ProtocolBinding {
                    id: OPENAI_CHAT.into(),
                    endpoint: None,
                }],
                base_url: Some("https://api.z.ai/api/paas/v4".into()),
                auth_methods: vec![AuthenticationMethod {
                    id: "api-key".into(),
                    label: "API key".into(),
                    kind: AuthenticationKind::ApiKey,
                }],
                model_sources: vec![DiscoverySource {
                    id: "models".into(),
                    kind: DiscoveryKind::Declared,
                    endpoint: None,
                }],
            }],
        },
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
            if connection.id.is_empty()
                || !seen_connections.insert(connection.id.as_str())
                || connection.protocols.is_empty()
            {
                return Err("invalid provider connection");
            }
            let base = connection
                .base_url
                .as_deref()
                .ok_or("missing provider endpoint")?;
            let validated_base = validate_endpoint(base, EndpointMode::Remote)
                .map_err(|_| "unsafe provider endpoint")?;
            let mut protocol_ids = BTreeSet::new();
            for protocol in &connection.protocols {
                if protocol.id.is_empty() || !protocol_ids.insert(protocol.id.as_str()) {
                    return Err("duplicate or empty provider protocol");
                }
                if let Some(endpoint) = protocol.endpoint.as_deref() {
                    let checked = validate_endpoint(endpoint, EndpointMode::Remote)
                        .map_err(|_| "unsafe protocol endpoint")?;
                    if !same_credential_origin(&validated_base, &checked) {
                        return Err("cross-origin protocol endpoint");
                    }
                }
            }
            let mut method_ids = BTreeSet::new();
            for method in &connection.auth_methods {
                if !method_ids.insert(method.id.as_str()) || method.id.is_empty() {
                    return Err("duplicate authentication method");
                }
            }
            let mut source_ids = BTreeSet::new();
            for source in &connection.model_sources {
                if source.id.is_empty() || !source_ids.insert(source.id.as_str()) {
                    return Err("duplicate or empty model discovery source");
                }
                match (&source.kind, source.endpoint.as_deref()) {
                    (DiscoveryKind::Declared, None) => {}
                    (DiscoveryKind::Dynamic, Some(endpoint)) => {
                        let validated_endpoint = validate_endpoint(endpoint, EndpointMode::Remote)
                            .map_err(|_| "unsafe catalog endpoint")?;
                        if !same_credential_origin(&validated_base, &validated_endpoint) {
                            return Err("cross-origin model discovery");
                        }
                    }
                    _ => return Err("inconsistent model discovery source"),
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
        assert_eq!(
            openai.connections[1].protocols,
            vec![ProtocolBinding {
                id: OPENAI_RESPONSES.into(),
                endpoint: None,
            }]
        );
    }

    #[test]
    fn registration_accepts_no_auth_key_only_and_two_independent_methods() {
        // These are metadata-only fixtures, not claims that a provider
        // backend, OAuth issuer, or usable model has been installed.
        let no_auth = connection(
            "no-auth",
            "Credential-free connection",
            "https://models.example.test/v1",
            &[OPENAI_CHAT],
            vec![],
            None,
        );
        let key_only = connection(
            "key",
            "Named API-key connection",
            "https://models.example.test/v1",
            &[OPENAI_CHAT],
            api_key(),
            Some("https://models.example.test/v1/models"),
        );
        let both = connection(
            "both",
            "Two explicit methods",
            "https://models.example.test/v1",
            &[OPENAI_RESPONSES],
            vec![
                method("api-key", "API key", AuthenticationKind::ApiKey),
                method(
                    "oauth",
                    "Approved OAuth",
                    AuthenticationKind::OAuthAuthorizationCode,
                ),
            ],
            Some("https://models.example.test/v1/models"),
        );
        let fixture = provider(
            "fixture",
            "Fixture",
            vec![no_auth.clone(), key_only.clone(), both.clone()],
        );
        assert_eq!(validate_registrations(&[fixture]), Ok(()));
        assert_eq!(
            [
                no_auth.auth_methods.len(),
                key_only.auth_methods.len(),
                both.auth_methods.len()
            ],
            [0, 1, 2]
        );
        assert_eq!(both.auth_methods[0].kind, AuthenticationKind::ApiKey);
        assert_eq!(
            both.auth_methods[1].kind,
            AuthenticationKind::OAuthAuthorizationCode
        );
        // The choice is scoped to one connection. Duplicating method IDs
        // within that connection would make explicit authentication ambiguous.
        let mut duplicate = both;
        duplicate.auth_methods[1].id = "api-key".into();
        let bad = provider("fixture", "Fixture", vec![duplicate]);
        assert_eq!(
            validate_registrations(&[bad]),
            Err("duplicate authentication method")
        );
    }

    #[test]
    fn each_provider_uses_an_explicit_protocol_and_safe_endpoint() {
        let all = registrations();
        assert!(all.iter().all(|provider| provider
            .connections
            .iter()
            .all(|connection| !connection.protocols.is_empty() && connection.base_url.is_some())));
        assert_eq!(
            all.iter()
                .find(|provider| provider.id == "anthropic")
                .unwrap()
                .connections[0]
                .protocols,
            vec![ProtocolBinding {
                id: ANTHROPIC_MESSAGES.into(),
                endpoint: None,
            }]
        );
        validate_registrations(&all).unwrap();
    }

    #[test]
    fn reject_duplicate_provider_and_unsafe_registered_endpoint() {
        let mut providers = registrations();
        providers.push(providers[0].clone());
        assert_eq!(
            validate_registrations(&providers),
            Err("duplicate or empty provider identity")
        );
        let mut providers = registrations();
        providers[0].connections[0].base_url = Some("http://external.test/v1".into());
        assert_eq!(
            validate_registrations(&providers),
            Err("unsafe provider endpoint")
        );
    }

    #[test]
    fn reject_ambiguous_or_inconsistent_model_discovery_sources() {
        let mut providers = registrations();
        let duplicate = providers[0].connections[0].model_sources[0].clone();
        providers[0].connections[0].model_sources.push(duplicate);
        assert_eq!(
            validate_registrations(&providers),
            Err("duplicate or empty model discovery source")
        );

        let mut providers = registrations();
        providers[0].connections[0].model_sources[0].id.clear();
        assert_eq!(
            validate_registrations(&providers),
            Err("duplicate or empty model discovery source")
        );

        let mut providers = registrations();
        providers[0].connections[0].model_sources[0].endpoint = None;
        assert_eq!(
            validate_registrations(&providers),
            Err("inconsistent model discovery source")
        );

        let mut providers = registrations();
        providers[0].connections[0].model_sources[0].kind = DiscoveryKind::Declared;
        assert_eq!(
            validate_registrations(&providers),
            Err("inconsistent model discovery source")
        );

        let mut providers = registrations();
        providers[0].connections[0].model_sources[0].endpoint =
            Some("http://external.test/models".into());
        assert_eq!(
            validate_registrations(&providers),
            Err("unsafe catalog endpoint")
        );
    }

    #[test]
    fn registration_rejects_ambiguous_connection_and_protocol_identities() {
        let mut providers = registrations();
        providers[0].connections[0].id.clear();
        assert_eq!(
            validate_registrations(&providers),
            Err("invalid provider connection")
        );

        let mut providers = registrations();
        providers[0].connections[0].protocols.push(ProtocolBinding {
            id: OPENAI_CHAT.into(),
            endpoint: None,
        });
        assert_eq!(
            validate_registrations(&providers),
            Err("duplicate or empty provider protocol")
        );

        let mut providers = registrations();
        providers[0].connections[0].protocols[0].id.clear();
        assert_eq!(
            validate_registrations(&providers),
            Err("duplicate or empty provider protocol")
        );
    }

    #[test]
    fn registration_rejects_cross_origin_model_discovery() {
        let mut providers = registrations();
        providers[0].connections[0].model_sources[0].endpoint =
            Some("https://unrelated.test/v1/models".into());
        assert_eq!(
            validate_registrations(&providers),
            Err("cross-origin model discovery")
        );

        let mut providers = registrations();
        providers[0].connections[0].model_sources[0].endpoint =
            Some("https://api.openai.com:8443/v1/models".into());
        assert_eq!(
            validate_registrations(&providers),
            Err("cross-origin model discovery")
        );

        // Same origin with a different path is allowed for model discovery.
        let mut providers = registrations();
        providers[0].connections[0].model_sources[0].endpoint =
            Some("https://api.openai.com/available-models".into());
        validate_registrations(&providers).unwrap();
    }

    #[test]
    fn protocol_overrides_must_keep_the_registered_credential_origin() {
        let mut providers = registrations();
        providers[0].connections[0].protocols[0].endpoint =
            Some("https://api.openai.com/v1/responses".into());
        validate_registrations(&providers).unwrap();

        providers[0].connections[0].protocols[0].endpoint =
            Some("https://outside.test/responses".into());
        assert_eq!(
            validate_registrations(&providers),
            Err("cross-origin protocol endpoint")
        );

        providers[0].connections[0].protocols[0].endpoint =
            Some("http://outside.test/responses".into());
        assert_eq!(
            validate_registrations(&providers),
            Err("unsafe protocol endpoint")
        );
    }

    #[test]
    fn registration_never_serializes_credentials() {
        let providers = registrations();
        let json = format!("{providers:?}");
        assert!(!json.contains("secret="));
        assert!(providers
            .iter()
            .all(
                |provider| provider.connections.iter().all(|connection| connection
                    .auth_methods
                    .iter()
                    .all(|method| !method.id.is_empty()))
            ));
    }
}
