//! OpenAI provider declarations: data and one existing M1 event hook.
//! No provider catalog state, credential flows, model transport or dispatch.
//! The shared phenix-plugin-providers runtime owns those mechanisms.
use phenix_provider_builtins::declaration::{
    Auth, AuthPreset, Connection, Discovery, KeyPresentation, Protocol, Provider,
};
use phenix_provider_contracts::{ProviderError, ProviderPopulate, Providers, ProvidersPopulate};
use phenix_sdk::{CallResult, Host, StaticGraphContribution};

/// Both methods share one logical OpenAI provider without sharing accounts.
/// Strategies, request codecs, key storage and discovery are owned by the
/// providers SDK/runtime, not this declaration plugin.
const OPENAI: Provider<'static> = Provider {
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
};

/// Existing graph triggers are Phenix's equivalent of Neovim autocmds.
/// The shared providers plugin declares the event; this plugin subscribes.
pub const GRAPH: StaticGraphContribution = StaticGraphContribution {
    plugin: "phenix.provider.openai",
    events: &[],
    actions: &[],
    nodes: &[phenix_sdk::on_event!(
        "populate",
        ProvidersPopulate,
        "phenix.provider.populate@1/register",
    )],
};

#[phenix_sdk::plugin(
    id = "phenix.provider.openai",
    version = "1.0.0",
    provide(phenix_provider_contracts::ProviderPopulate, slot = "Populate"),
    require(phenix_provider_contracts::ProviderRegistry, slot = "Registry"),
    graph(super::GRAPH)
)]
mod plugin {
    use super::*;

    #[derive(Default)]
    pub struct State;

    #[provide(ProviderPopulate::register)]
    fn register(
        _: &mut State,
        host: &Host<'_>,
        event: ProvidersPopulate,
    ) -> CallResult<(), ProviderError> {
        Providers::new(host, "Registry", &event.scope).register(OPENAI)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_provider_contracts::ProviderDefinition;
    use phenix_sdk::ContractMetadata;

    #[test]
    fn metadata_is_one_handler_one_registry_import() {
        let meta = ContractMetadata::decode(plugin::METADATA.bytes()).unwrap();
        assert_eq!(meta.provides.len(), 1);
        assert_eq!(meta.requires.len(), 1);
        assert_eq!(meta.requires[0].contract, "phenix.provider.registry@1");
        let graph = GRAPH.owned().unwrap();
        assert!(graph.events.is_empty());
        assert!(graph.actions.is_empty());
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].triggers, vec!["phenix.providers.populate"]);
    }

    #[test]
    fn data_declares_two_disjoint_auth_connections() {
        let descriptor = OPENAI.into_descriptor().unwrap();
        assert_eq!(descriptor.id, "openai");
        assert_eq!(descriptor.connections.len(), 2);
        assert_eq!(descriptor.connections[0].id, "api");
        assert_eq!(descriptor.connections[1].id, "chatgpt");
        assert_ne!(
            descriptor.connections[0].auth_methods[0].id,
            descriptor.connections[1].auth_methods[0].id
        );
    }
}
