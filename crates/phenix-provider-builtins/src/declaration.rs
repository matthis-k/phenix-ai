//! Provider authoring is immutable data; adapter execution belongs to plugins.
//!
//! This module deliberately has no proc macros, registration ABI or provider
//! state. A single shared registration host turns these definitions into
//! M1 `ProviderRegistrations` values. Descriptions do not grant credentials,
//! OAuth sessions, model entitlement, or protocol execution authority.

use crate::{
    standard_endpoint, validate_registrations, AuthenticationAdapters, ANTHROPIC_MESSAGES,
    OPENAI_CHAT, OPENAI_RESPONSES,
};
use phenix_provider_contracts::{
    AdapterBinding, ApiKeyOptions, ApiKeyPresentation, AuthenticationMethod, ConnectionDescriptor,
    DiscoveryKind, DiscoverySource, ProtocolBinding, ProviderDescriptor,
};

#[derive(Debug, Clone, Copy)]
pub struct Provider<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub connections: &'a [Connection<'a>],
}

#[derive(Debug, Clone, Copy)]
pub struct Connection<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub endpoint: &'a str,
    pub protocols: &'a [Protocol<'a>],
    pub auth: Auth<'a>,
    pub discovery: Discovery<'a>,
}

/// Only listed protocols are advertised. Standard variants reuse installed
/// route conventions; a custom binding does not infer an executable adapter.
#[derive(Debug, Clone, Copy)]
pub enum Protocol<'a> {
    OpenAiResponses,
    OpenAiChat,
    AnthropicMessages,
    Custom {
        id: &'a str,
        endpoint: Option<&'a str>,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum KeyPresentation<'a> {
    Bearer,
    Header { name: &'a str, prefix: &'a str },
}

#[derive(Debug, Clone, Copy)]
pub enum AuthPreset {
    ChatGptOAuth,
}

#[derive(Debug, Clone, Copy)]
pub enum Auth<'a> {
    ApiKey {
        env_hint: Option<&'a str>,
        presentation: KeyPresentation<'a>,
    },
    Preset(AuthPreset),
    /// An adapter must be installed by composition; a name alone grants nothing.
    Custom(&'a str),
    None,
}

#[derive(Debug, Clone, Copy)]
pub enum Discovery<'a> {
    /// Uses the explicitly selected standard `/models` convention.
    StandardModels(&'a str),
    /// Metadata for a catalog implemented by another selected plugin.
    Declared(&'a str),
    /// A separately implemented discovery strategy with an exact URL.
    Custom {
        adapter: &'a str,
        endpoint: Option<&'a str>,
    },
    None,
}

impl Protocol<'_> {
    fn binding(self, base: &str) -> Result<ProtocolBinding, &'static str> {
        let (id, endpoint) = match self {
            Self::OpenAiResponses => (
                OPENAI_RESPONSES,
                Some(standard_endpoint(base, "responses")?),
            ),
            Self::OpenAiChat => (
                OPENAI_CHAT,
                Some(standard_endpoint(base, "chat/completions")?),
            ),
            Self::AnthropicMessages => (
                ANTHROPIC_MESSAGES,
                Some(standard_endpoint(base, "messages")?),
            ),
            Self::Custom { id, endpoint } => (id, endpoint.map(str::to_owned)),
        };
        Ok(ProtocolBinding {
            id: id.into(),
            endpoint,
        })
    }
}

fn methods(
    auth: Auth<'_>,
    registry: &impl AuthenticationAdapters,
) -> Result<Vec<AuthenticationMethod>, &'static str> {
    match auth {
        Auth::ApiKey {
            env_hint,
            presentation,
        } => {
            let presentation = match presentation {
                KeyPresentation::Bearer => ApiKeyPresentation::Bearer,
                KeyPresentation::Header { name, prefix } => ApiKeyPresentation::Header {
                    name: name.into(),
                    prefix: prefix.into(),
                },
            };
            let options = ApiKeyOptions {
                environment_variable: env_hint.map(str::to_owned),
                interactive: true,
                presentation,
            };
            options
                .validate()
                .map_err(|_| "invalid API key declaration")?;
            let mut available = registry.methods("api-key")?;
            let key = available
                .iter_mut()
                .find(|method| method.id == "api-key")
                .ok_or("API key strategy is not installed")?;
            key.adapter = AdapterBinding::from_typed("api-key", &options)
                .map_err(|_| "invalid API key strategy")?;
            Ok(available)
        }
        Auth::Preset(AuthPreset::ChatGptOAuth) => registry.methods("chatgpt-oauth"),
        Auth::Custom(id) => registry.methods(id),
        Auth::None => Ok(Vec::new()),
    }
}

fn sources(discovery: Discovery<'_>, base: &str) -> Result<Vec<DiscoverySource>, &'static str> {
    let (adapter, kind, endpoint) = match discovery {
        Discovery::StandardModels(adapter) => (
            adapter,
            DiscoveryKind::Dynamic,
            Some(standard_endpoint(base, "models")?),
        ),
        Discovery::Declared(adapter) => (adapter, DiscoveryKind::Declared, None),
        Discovery::Custom { adapter, endpoint } => {
            (adapter, DiscoveryKind::Dynamic, endpoint.map(str::to_owned))
        }
        Discovery::None => return Ok(Vec::new()),
    };
    Ok(vec![DiscoverySource {
        id: "models".into(),
        kind,
        endpoint,
        adapter: AdapterBinding::from_typed(adapter, &())
            .map_err(|_| "invalid discovery adapter")?,
    }])
}

/// Compile data from any provider crate into validated registration metadata.
/// An adapter selector is never automatically activated here.
pub fn compile(
    providers: &[Provider<'_>],
    registry: &impl AuthenticationAdapters,
) -> Result<Vec<ProviderDescriptor>, &'static str> {
    let mut result = Vec::with_capacity(providers.len());
    for provider in providers {
        let mut connections = Vec::with_capacity(provider.connections.len());
        for spec in provider.connections {
            connections.push(ConnectionDescriptor {
                id: spec.id.into(),
                label: spec.name.into(),
                base_url: Some(spec.endpoint.into()),
                protocols: spec
                    .protocols
                    .iter()
                    .map(|protocol| protocol.binding(spec.endpoint))
                    .collect::<Result<_, _>>()?,
                auth_methods: methods(spec.auth, registry)?,
                model_sources: sources(spec.discovery, spec.endpoint)?,
            });
        }
        result.push(ProviderDescriptor {
            id: provider.id.into(),
            label: provider.name.into(),
            connections,
        });
    }
    validate_registrations(&result)?;
    Ok(result)
}

/// Provider declarations register through the same typed interface as any
/// externally compiled provider: only the conventions are shared.
impl phenix_provider_contracts::ProviderDefinition for Provider<'_> {
    fn into_descriptor(self) -> Result<ProviderDescriptor, phenix_provider_contracts::ProviderError> {
        let providers = compile(&[self], &crate::BuiltinAuthenticationAdapters).map_err(|_| {
            phenix_provider_contracts::ProviderError {
                code: phenix_provider_contracts::ProviderErrorCode::InvalidInput,
                message: "invalid provider declaration".into(),
            }
        })?;
        providers.into_iter().next().ok_or_else(|| phenix_provider_contracts::ProviderError {
            code: phenix_provider_contracts::ProviderErrorCode::InvalidInput,
            message: "empty provider declaration".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BuiltinAuthenticationAdapters;

    const THIRD_PARTY: &[Provider<'static>] = &[Provider {
        id: "third-party",
        name: "Third party",
        connections: &[Connection {
            id: "api",
            name: "Third party API",
            endpoint: "https://example.test/v1",
            protocols: &[Protocol::OpenAiResponses],
            auth: Auth::ApiKey {
                env_hint: Some("THIRD_PARTY_KEY"),
                presentation: KeyPresentation::Bearer,
            },
            discovery: Discovery::StandardModels("openai.models"),
        }],
    }];

    #[test]
    fn conventional_provider_requires_only_data_and_has_no_execution_authority() {
        let descriptors = compile(THIRD_PARTY, &BuiltinAuthenticationAdapters).unwrap();
        let connection = &descriptors[0].connections[0];
        assert_eq!(connection.id, "api");
        assert_eq!(
            connection.protocols[0].endpoint.as_deref(),
            Some("https://example.test/v1/responses")
        );
        assert_eq!(
            connection.model_sources[0].endpoint.as_deref(),
            Some("https://example.test/v1/models")
        );
        let options = connection.auth_methods[0]
            .adapter
            .config
            .decode::<ApiKeyOptions>()
            .unwrap();
        assert_eq!(
            options.environment_variable.as_deref(),
            Some("THIRD_PARTY_KEY")
        );
        assert!(connection.model_sources[0].adapter.id == "openai.models");
    }

    #[test]
    fn explicit_conventions_and_absent_discovery_are_not_inferred() {
        let definitions = [Provider {
            id: "custom",
            name: "Custom",
            connections: &[Connection {
                id: "api",
                name: "Custom API",
                endpoint: "https://example.test/v1",
                protocols: &[Protocol::Custom {
                    id: "custom-protocol",
                    endpoint: None,
                }],
                auth: Auth::None,
                discovery: Discovery::None,
            }],
        }];
        let compiled = compile(&definitions, &BuiltinAuthenticationAdapters).unwrap();
        let conn = &compiled[0].connections[0];
        assert_eq!(conn.protocols[0].endpoint, None);
        assert!(conn.model_sources.is_empty());
        assert!(conn.auth_methods.is_empty());
    }

    #[test]
    fn unsafe_endpoints_and_credentials_fail_closed() {
        let definitions = [Provider {
            id: "invalid",
            name: "Invalid",
            connections: &[Connection {
                id: "api",
                name: "API",
                endpoint: "https://example.test/v1",
                protocols: &[Protocol::OpenAiResponses],
                auth: Auth::ApiKey {
                    env_hint: Some("INVALID-VARIABLE"),
                    presentation: KeyPresentation::Bearer,
                },
                discovery: Discovery::None,
            }],
        }];
        assert!(compile(&definitions, &BuiltinAuthenticationAdapters).is_err());

        let unsafe_endpoint = [Provider {
            id: "invalid",
            name: "Invalid",
            connections: &[Connection {
                id: "api",
                name: "API",
                endpoint: "https://user@example.test/v1",
                protocols: &[Protocol::OpenAiResponses],
                auth: Auth::None,
                discovery: Discovery::None,
            }],
        }];
        assert!(compile(&unsafe_endpoint, &BuiltinAuthenticationAdapters).is_err());
    }

    #[test]
    fn absent_protocols_or_duplicate_connections_are_rejected() {
        let invalid = [Provider {
            id: "test",
            name: "Test",
            connections: &[Connection {
                id: "api",
                name: "API",
                endpoint: "https://example.test/v1",
                protocols: &[],
                auth: Auth::None,
                discovery: Discovery::None,
            }],
        }];
        assert!(compile(&invalid, &BuiltinAuthenticationAdapters).is_err());
        assert!(compile(&[invalid[0], invalid[0]], &BuiltinAuthenticationAdapters).is_err());
    }
}
