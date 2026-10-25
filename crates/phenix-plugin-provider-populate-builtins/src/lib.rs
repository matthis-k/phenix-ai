//! Ordinary M1 graph-triggered registration of default provider declarations.
//! This is a migration fixture for the event registration path. It contains
//! no auth flow, protocol implementation, registry, credential or new runtime.

use phenix_provider_contracts::ProvidersPopulate;
use phenix_sdk::{FailurePolicy, JoinPolicy, StaticGraphContribution, StaticGraphNode};

pub const GRAPH: StaticGraphContribution = StaticGraphContribution {
    plugin: "phenix.provider.populate-builtins",
    events: &[],
    actions: &[],
    nodes: &[StaticGraphNode {
        id: "register",
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
    id = "phenix.provider.populate-builtins",
    version = "1.0.0",
    provide(phenix_provider_contracts::ProviderPopulate, slot = "Populate"),
    require(phenix_provider_contracts::ProviderRegistry, slot = "Registry"),
    graph(super::GRAPH)
)]
mod plugin {
    use super::*;
    use phenix_provider_builtins::defaults::PROVIDERS;
    use phenix_provider_contracts::{ProviderError, ProviderPopulate, Providers};
    use phenix_sdk::{CallResult, Host};

    /// The event and registry are standard Phenix graph/contract mechanisms.
    #[provide(ProviderPopulate::register)]
    fn register(host: &Host<'_>, event: ProvidersPopulate) -> CallResult<(), ProviderError> {
        let registry = Providers::new(host, "Registry", &event.scope);
        for provider in PROVIDERS.iter().copied() {
            registry.register(provider)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_sdk::ContractMetadata;

    #[test]
    fn ordinary_trigger_declares_registry_binding_and_typed_handler() {
        let metadata = ContractMetadata::decode(plugin::METADATA.bytes()).unwrap();
        assert_eq!(metadata.provides.len(), 1);
        assert_eq!(metadata.requires.len(), 1);
        assert_eq!(metadata.requires[0].contract, "phenix.provider.registry@1");
        let graph = GRAPH.owned().unwrap();
        assert_eq!(graph.events.len(), 0);
        assert_eq!(graph.actions.len(), 0);
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].triggers, vec!["phenix.providers.populate"]);
        assert_eq!(graph.nodes[0].operation, "phenix.provider.populate@1/register");
    }
}
