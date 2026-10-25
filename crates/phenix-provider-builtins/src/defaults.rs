//! Built-in provider declarations: data only, no contracts or callbacks.
//!
//! Common transport, key presentation and model endpoint conventions are
//! compiled by `declaration`. Provider-specific OAuth behavior is selected
//! by a named strategy, not reimplemented here.

use crate::declaration::{
    Auth, AuthPreset, Connection, Discovery, KeyPresentation, Protocol, Provider,
};

pub const PROVIDERS: &[Provider<'static>] = &[
    Provider {
        id: "openai",
        name: "OpenAI",
        connections: &[
            Connection {
                id: "api",
                name: "OpenAI API",
                endpoint: "https://api.openai.com/v1",
                protocols: &[Protocol::OpenAiResponses, Protocol::OpenAiChat],
                auth: Auth::ApiKey {
                    env_hint: Some("OPENAI_API_KEY"),
                    presentation: KeyPresentation::Bearer,
                },
                discovery: Discovery::StandardModels("openai.models"),
            },
            Connection {
                id: "chatgpt",
                name: "ChatGPT subscription",
                endpoint: "https://api.openai.com/v1",
                protocols: &[Protocol::OpenAiResponses],
                auth: Auth::Preset(AuthPreset::ChatGptOAuth),
                discovery: Discovery::StandardModels("chatgpt.models"),
            },
        ],
    },
    Provider {
        id: "openrouter",
        name: "OpenRouter",
        connections: &[Connection {
            id: "api",
            name: "OpenRouter API",
            endpoint: "https://openrouter.ai/api/v1",
            protocols: &[Protocol::OpenAiChat, Protocol::OpenAiResponses],
            auth: Auth::ApiKey {
                env_hint: None,
                presentation: KeyPresentation::Bearer,
            },
            discovery: Discovery::StandardModels("openrouter.models"),
        }],
    },
    Provider {
        id: "anthropic",
        name: "Anthropic",
        connections: &[Connection {
            id: "api",
            name: "Anthropic API",
            endpoint: "https://api.anthropic.com/v1",
            protocols: &[Protocol::AnthropicMessages],
            auth: Auth::ApiKey {
                env_hint: None,
                presentation: KeyPresentation::Header {
                    name: "x-api-key",
                    prefix: "",
                },
            },
            discovery: Discovery::StandardModels("anthropic.models"),
        }],
    },
    Provider {
        id: "opencode-zen",
        name: "OpenCode Zen",
        connections: &[Connection {
            id: "api",
            name: "OpenCode Zen",
            endpoint: "https://opencode.ai/zen/v1",
            protocols: &[
                Protocol::OpenAiChat,
                Protocol::OpenAiResponses,
                Protocol::Custom {
                    id: crate::ANTHROPIC_MESSAGES,
                    endpoint: None,
                },
            ],
            auth: Auth::ApiKey {
                env_hint: None,
                presentation: KeyPresentation::Bearer,
            },
            discovery: Discovery::StandardModels("opencode.models"),
        }],
    },
    Provider {
        id: "opencode-go",
        name: "OpenCode Go",
        connections: &[Connection {
            id: "api",
            name: "OpenCode Go",
            endpoint: "https://opencode.ai/zen/go/v1",
            protocols: &[
                Protocol::OpenAiChat,
                Protocol::OpenAiResponses,
                Protocol::Custom {
                    id: crate::ANTHROPIC_MESSAGES,
                    endpoint: None,
                },
            ],
            auth: Auth::ApiKey {
                env_hint: None,
                presentation: KeyPresentation::Bearer,
            },
            discovery: Discovery::StandardModels("opencode.models"),
        }],
    },
    Provider {
        id: "zai",
        name: "Z.ai",
        connections: &[Connection {
            id: "standard",
            name: "Z.ai API",
            endpoint: "https://api.z.ai/api/paas/v4",
            protocols: &[Protocol::OpenAiChat],
            auth: Auth::ApiKey {
                env_hint: None,
                presentation: KeyPresentation::Bearer,
            },
            discovery: Discovery::Declared("zai.declared"),
        }],
    },
];

/// Providers in the shared standard bundle. OpenAI is intentionally omitted:
/// its separately installed declaration plugin owns that provider identity.
/// This partition is a transitional fixture until legacy pull registration is
/// removed; new third-party plugins should declare their own provider values.
pub fn standard_provider_definitions() -> impl Iterator<Item = Provider<'static>> {
    PROVIDERS
        .iter()
        .copied()
        .filter(|provider| provider.id != "openai")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{declaration::compile, BuiltinAuthenticationAdapters};

    #[test]
    fn event_contribution_bundle_never_redefines_openai() {
        let standards = standard_provider_definitions().collect::<Vec<_>>();
        assert_eq!(standards.len(), 5);
        assert!(standards.iter().all(|provider| provider.id != "openai"));
    }

    #[test]
    fn all_shipped_providers_are_described_by_static_data() {
        let providers = compile(PROVIDERS, &BuiltinAuthenticationAdapters).unwrap();
        assert_eq!(providers.len(), 6);
        assert_eq!(providers[0].id, "openai");
        assert_eq!(providers[0].connections.len(), 2);
        assert_eq!(providers[0].connections[0].protocols.len(), 2);
        assert_eq!(
            providers[0].connections[1].auth_methods[0].id,
            "chatgpt-sign-in"
        );
        assert_eq!(providers[2].id, "anthropic");
        let anthropic_key = providers[2].connections[0].auth_methods[0]
            .adapter
            .config
            .decode::<phenix_provider_contracts::ApiKeyOptions>()
            .unwrap();
        assert_eq!(anthropic_key.presentation.header_name(), "x-api-key");
        assert_eq!(providers[5].id, "zai");
        assert_eq!(
            providers[5].connections[0].model_sources[0].kind,
            phenix_provider_contracts::DiscoveryKind::Declared
        );
        assert_eq!(providers[5].connections[0].model_sources[0].endpoint, None);
    }
}
