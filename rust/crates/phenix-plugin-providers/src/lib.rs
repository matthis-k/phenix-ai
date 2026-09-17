#![forbid(unsafe_code)]

use phenix_core::{Authority, PluginExecution, PluginId, PluginManifest};
use phenix_provider_sdk::{auth, Auth, Endpoint, Protocol, ProviderDefinition};

pub const PROVIDERS_PLUGIN: &str = "phenix.providers";
const OPENAI_CODEX_PROVIDER: &str = "openai-codex";
const OPENAI_CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ApiTokenAuth {
    Bearer,
    Header(&'static str),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderPreset {
    id: &'static str,
    endpoint: &'static str,
    protocol: Protocol,
    api_token: ApiTokenAuth,
    environment: &'static str,
}

impl ProviderPreset {
    const fn bearer(
        id: &'static str,
        endpoint: &'static str,
        protocol: Protocol,
        environment: &'static str,
    ) -> Self {
        Self {
            id,
            endpoint,
            protocol,
            api_token: ApiTokenAuth::Bearer,
            environment,
        }
    }

    const fn header(
        id: &'static str,
        endpoint: &'static str,
        protocol: Protocol,
        header: &'static str,
        environment: &'static str,
    ) -> Self {
        Self {
            id,
            endpoint,
            protocol,
            api_token: ApiTokenAuth::Header(header),
            environment,
        }
    }

    pub const fn id(self) -> &'static str {
        self.id
    }

    pub const fn endpoint(self) -> &'static str {
        self.endpoint
    }

    pub const fn protocol(self) -> Protocol {
        self.protocol
    }

    pub const fn environment(self) -> &'static str {
        self.environment
    }

    #[must_use]
    pub fn auth(self) -> auth::Definition {
        let api_token = match self.api_token {
            ApiTokenAuth::Bearer => auth::ApiTokenMethod::bearer(),
            ApiTokenAuth::Header(header) => {
                auth::ApiTokenMethod::header(header).expect("common provider header name is valid")
            }
        };
        auth::Definition::api_token(api_token)
    }

    #[must_use]
    pub fn definition(self) -> ProviderDefinition {
        self.definition_with_auth(self.auth())
            .with_default_auth(Auth::api_token(
                auth::ApiToken::env(self.environment)
                    .expect("common provider environment variable is valid"),
            ))
    }

    #[must_use]
    pub fn definition_with_auth(self, auth: auth::Definition) -> ProviderDefinition {
        ProviderDefinition::new(
            PluginId::parse(self.id).expect("common provider plugin id is valid"),
            Endpoint::parse(self.endpoint).expect("common provider endpoint is valid"),
            self.protocol,
            auth,
        )
    }
}

pub const COMMON_PROVIDERS: [ProviderPreset; 10] = [
    ProviderPreset::bearer(
        "openai-api",
        "https://api.openai.com/v1",
        Protocol::OpenAiResponses,
        "OPENAI_API_KEY",
    ),
    ProviderPreset::header(
        "anthropic",
        "https://api.anthropic.com/v1",
        Protocol::AnthropicMessages,
        "x-api-key",
        "ANTHROPIC_API_KEY",
    ),
    ProviderPreset::bearer(
        "open-router",
        "https://openrouter.ai/api/v1",
        Protocol::OpenAiChatCompletions,
        "OPEN_ROUTER_API_KEY",
    ),
    ProviderPreset::bearer(
        "groq",
        "https://api.groq.com/openai/v1",
        Protocol::OpenAiResponses,
        "GROQ_API_KEY",
    ),
    ProviderPreset::bearer(
        "gemini",
        "https://generativelanguage.googleapis.com/v1beta/openai/",
        Protocol::OpenAiChatCompletions,
        "GEMINI_API_KEY",
    ),
    ProviderPreset::bearer(
        "deepseek",
        "https://api.deepseek.com",
        Protocol::OpenAiChatCompletions,
        "DEEPSEEK_API_KEY",
    ),
    ProviderPreset::bearer(
        "together",
        "https://api.together.xyz/v1",
        Protocol::OpenAiChatCompletions,
        "TOGETHER_API_KEY",
    ),
    ProviderPreset::bearer(
        "mistral",
        "https://api.mistral.ai/v1",
        Protocol::OpenAiChatCompletions,
        "MISTRAL_API_KEY",
    ),
    ProviderPreset::bearer(
        "xai",
        "https://api.x.ai/v1",
        Protocol::OpenAiResponses,
        "XAI_API_KEY",
    ),
    ProviderPreset::bearer(
        "fireworks",
        "https://api.fireworks.ai/inference/v1",
        Protocol::OpenAiChatCompletions,
        "FIREWORKS_API_KEY",
    ),
];

fn openai_codex_oauth() -> auth::OAuthMethod {
    auth::OAuthMethod::authorization_code(
        "https://auth.openai.com/oauth/authorize",
        "https://auth.openai.com/oauth/token",
        OPENAI_CODEX_CLIENT_ID,
    )
    .expect("static OpenAI OAuth endpoints are valid")
    .with_scope("openid")
    .with_scope("profile")
    .with_scope("email")
    .with_scope("offline_access")
    .with_scope("api.connectors.read")
    .with_scope("api.connectors.invoke")
    .with_authorization_param("id_token_add_organizations", "true")
    .with_authorization_param("codex_cli_simplified_flow", "true")
    .with_authorization_param("originator", "phenix")
    .with_callback_ports([1455, 1457])
    .with_static_header("originator", "phenix")
    .with_static_header("version", env!("CARGO_PKG_VERSION"))
    .with_account_id_header(
        "ChatGPT-Account-ID",
        [
            vec!["chatgpt_account_id".to_owned()],
            vec![
                "https://api.openai.com/auth".to_owned(),
                "chatgpt_account_id".to_owned(),
            ],
        ],
    )
}

#[must_use]
pub fn openai_codex_definition() -> ProviderDefinition {
    ProviderDefinition::new(
        PluginId::parse(OPENAI_CODEX_PROVIDER).expect("static provider id is valid"),
        Endpoint::parse("https://chatgpt.com/backend-api/codex/")
            .expect("static Codex endpoint is valid"),
        Protocol::OpenAiResponses,
        auth::Definition::oauth(openai_codex_oauth()),
    )
}

#[must_use]
pub fn common_provider_definitions() -> Vec<ProviderDefinition> {
    COMMON_PROVIDERS
        .into_iter()
        .map(ProviderPreset::definition)
        .chain(std::iter::once(openai_codex_definition()))
        .collect()
}

#[must_use]
pub fn providers_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(PROVIDERS_PLUGIN).expect("static provider bundle id is valid"),
        version: 1,
        execution: PluginExecution::ResourceOnly,
        dependencies: common_provider_definitions()
            .into_iter()
            .map(|provider| provider.plugin_id().clone())
            .collect(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn common_provider_ids_and_endpoints_are_unique() {
        let definitions = common_provider_definitions();
        let ids = definitions
            .iter()
            .map(|provider| provider.plugin_id().as_str())
            .collect::<BTreeSet<_>>();
        let endpoints = definitions
            .iter()
            .map(|provider| provider.endpoint().as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), definitions.len());
        assert_eq!(endpoints.len(), definitions.len());
    }

    #[test]
    fn runtime_openai_providers_are_present() {
        let openai = COMMON_PROVIDERS
            .into_iter()
            .find(|provider| provider.id() == "openai-api")
            .expect("runtime OpenAI API provider is part of the common catalog");
        assert_eq!(openai.environment(), "OPENAI_API_KEY");
        assert_eq!(openai.protocol(), Protocol::OpenAiResponses);

        let codex = openai_codex_definition();
        assert_eq!(codex.plugin_id().as_str(), OPENAI_CODEX_PROVIDER);
        assert_eq!(codex.auth_kinds(), vec![phenix_provider_sdk::AuthKind::OAuth]);
        assert_eq!(codex.protocol_name(), "openai_responses");
    }

    #[test]
    fn provider_bundle_depends_on_every_common_provider() {
        let expected = common_provider_definitions()
            .into_iter()
            .map(|provider| provider.plugin_id().as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let actual = providers_manifest()
            .dependencies
            .into_iter()
            .map(|id| id.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, expected);
    }

    #[test]
    fn common_catalog_covers_every_builtin_protocol() {
        let protocols = COMMON_PROVIDERS
            .into_iter()
            .map(ProviderPreset::protocol)
            .collect::<Vec<_>>();
        for protocol in [
            Protocol::AnthropicMessages,
            Protocol::OpenAiChatCompletions,
            Protocol::OpenAiResponses,
        ] {
            assert!(protocols.contains(&protocol));
        }
    }

    #[test]
    fn every_common_provider_exposes_auth_and_model_services() {
        for definition in common_provider_definitions() {
            assert_eq!(definition.manifest().services.len(), 2);
            assert_eq!(definition.component_manifest().exports.len(), 2);
        }
    }

    #[test]
    fn preset_auth_can_be_composed_with_oauth() {
        let preset = COMMON_PROVIDERS[0];
        let oauth = auth::OAuthMethod::authorization_code(
            "https://example.com/oauth/authorize",
            "https://example.com/oauth/token",
            "client",
        )
        .unwrap();
        let definition = preset.definition_with_auth(preset.auth().with_oauth(oauth));

        assert_eq!(
            definition.auth_kinds(),
            vec![
                phenix_provider_sdk::AuthKind::ApiToken,
                phenix_provider_sdk::AuthKind::OAuth
            ]
        );
    }
}
