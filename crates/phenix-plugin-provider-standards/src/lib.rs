//! Standard provider definitions: plain data and one population hook.
//! No catalog, authentication state machine, credential store, or transport.
//! OpenAI is owned by phenix-plugin-provider-openai, not registered twice.

use phenix_provider_builtins::defaults::standard_provider_definitions;
use phenix_provider_contracts::{ProviderError, ProviderPopulate, Providers, ProvidersPopulate};
use phenix_sdk::{
    CallResult, FailurePolicy, Host, JoinPolicy, StaticGraphContribution, StaticGraphNode,
};

/// The providers manager declares and emits this event. Each independently
/// selected definition plugin contributes its data through the same contract.
pub const GRAPH: StaticGraphContribution = StaticGraphContribution {
    plugin: "phenix.provider.standards",
    events: &[],
    actions: &[],
    nodes: &[StaticGraphNode {
        id: "populate",
        triggers: &["phenix.providers.populate"],
        after: &[],
        selector: "",
        operation: "phenix.provider.populate@1/register",
        writes: &[],
        join: JoinPolicy::AllActive,
        failure: FailurePolicy::FailRoot,
    }],
};

#[phenix_sdk::plugin(
    id = "phenix.provider.standards",
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
        Providers::new(host, "Registry", &event.scope).register_all(standard_provider_definitions())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_provider_contracts::ProviderDefinition;
    use phenix_sdk::ContractMetadata;
    use std::collections::BTreeSet;

    #[test]
    fn only_plain_registration_and_one_import_are_exported() {
        let metadata = ContractMetadata::decode(plugin::METADATA.bytes()).unwrap();
        assert_eq!(metadata.provides.len(), 1);
        assert_eq!(metadata.requires.len(), 1);
        assert_eq!(metadata.requires[0].contract, "phenix.provider.registry@1");
        assert!(GRAPH.events.is_empty());
        assert!(GRAPH.actions.is_empty());
        assert_eq!(GRAPH.nodes[0].triggers, ["phenix.providers.populate"]);
    }

    #[test]
    fn shipped_non_openai_providers_are_unique_and_valid() {
        let descriptors = standard_provider_definitions()
            .map(ProviderDefinition::into_descriptor)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(descriptors.len(), 5);
        let ids = descriptors
            .iter()
            .map(|p| p.id.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), 5);
        assert!(!ids.contains("openai"));
        assert!(ids.contains("anthropic"));
        assert!(ids.contains("openrouter"));
    }
}
