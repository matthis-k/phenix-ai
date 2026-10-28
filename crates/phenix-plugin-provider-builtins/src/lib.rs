//! Data-only built-in provider registration contributor.
//!
//! The provider definitions are immutable Rust values in `defaults::PROVIDERS`.
//! This tiny generic M1 bridge exports metadata and no authentication,
//! discovery, model execution, credential or catalog-management contracts.

use phenix_provider_builtins::registrations;
use phenix_provider_contracts::{ProviderError, ProviderPage};

#[phenix_sdk::plugin(
    id = "phenix.provider.builtins",
    version = "1.0.0",
    profile = "state",
    provide(
        phenix_provider_contracts::ProviderRegistrations,
        slot = "Registrations"
    )
)]
mod plugin {
    use super::*;
    use phenix_provider_contracts::ProviderRegistrations;
    use phenix_sdk::CallResult;

    #[derive(Default)]
    pub struct State;

    #[provide(ProviderRegistrations::list)]
    fn list(_: &mut State) -> CallResult<ProviderPage, ProviderError> {
        Ok(ProviderPage {
            providers: registrations(),
            revision: 1,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_sdk::{ContractMetadata, MetadataExecution};

    #[test]
    fn contains_only_declarative_provider_registration_contract() {
        let metadata = ContractMetadata::decode(plugin::METADATA.bytes()).unwrap();
        assert_ne!(metadata.execution, MetadataExecution::None);
        assert_eq!(metadata.provides.len(), 1);
        assert_eq!(metadata.requires.len(), 0);
        assert_eq!(registrations().len(), 6);
    }
}
