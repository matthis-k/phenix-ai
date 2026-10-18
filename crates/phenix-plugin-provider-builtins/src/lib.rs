//! Runtime provider registration plugin. The same contracts can be supplied
//! by external plugins without modifying the kernel or Neovim.
//!
//! This initial plugin exposes offline metadata only. It refuses to claim
//! authentication or model entitlement until an authorized credentials and
//! discovery implementation is installed. It is not a mock successful login.

use phenix_provider_builtins::{registrations, validate_registrations};
use phenix_provider_contracts::{
    AuthenticationStatus, ModelPage, ProviderDescriptor, ProviderError, ProviderErrorCode,
};

fn error(code: ProviderErrorCode, message: &str) -> ProviderError {
    ProviderError {
        code,
        message: message.into(),
    }
}

fn catalog() -> Result<Vec<ProviderDescriptor>, ProviderError> {
    let providers = registrations();
    validate_registrations(&providers).map_err(|_| {
        error(
            ProviderErrorCode::InvalidInput,
            "invalid installed provider catalog",
        )
    })?;
    Ok(providers)
}

fn provider(id: &str) -> Result<ProviderDescriptor, ProviderError> {
    catalog()?
        .into_iter()
        .find(|item| item.id == id)
        .ok_or_else(|| error(ProviderErrorCode::NotFound, "unknown installed provider"))
}

fn connection(provider_id: &str, connection_id: &str) -> Result<bool, ProviderError> {
    let provider = provider(provider_id)?;
    provider
        .connections
        .iter()
        .find(|item| item.id == connection_id)
        .map(|item| !item.auth_methods.is_empty())
        .ok_or_else(|| error(ProviderErrorCode::NotFound, "unknown provider connection"))
}

#[phenix_sdk::plugin(
    id = "phenix.provider.builtins",
    version = "1.0.0",
    profile = "state",
    provide(phenix_provider_contracts::ProviderCatalog, slot = "Catalog"),
    provide(phenix_provider_contracts::ProviderAuth, slot = "Authentication")
)]
mod plugin {
    use super::*;
    use phenix_provider_contracts::{
        AuthChallenge, AuthQuery, AuthStart, ModelQuery, ProviderAuth, ProviderCatalog,
        ProviderDetailsQuery, ProviderPage, ProviderQuery,
    };
    use phenix_sdk::{CallError, CallResult, Host};

    #[derive(Default)]
    pub struct State;

    fn check(host: &Host<'_>, scope: &str) -> CallResult<(), ProviderError> {
        if scope.is_empty()
            || scope.len() > 128
            || !scope
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'-' || c == b'_')
        {
            return Err(CallError::Domain(error(
                ProviderErrorCode::InvalidInput,
                "invalid provider scope",
            )));
        }
        let resource = format!("phenix.providers.{scope}");
        if host.resource()?.as_deref() != Some(&resource) {
            return Err(CallError::Domain(error(
                ProviderErrorCode::Denied,
                "provider resource not authorized",
            )));
        }
        Ok(())
    }

    #[provide(ProviderCatalog::list)]
    fn list(
        _: &mut State,
        host: &Host<'_>,
        request: ProviderQuery,
    ) -> CallResult<ProviderPage, ProviderError> {
        check(host, &request.scope)?;
        Ok(ProviderPage {
            providers: catalog().map_err(CallError::Domain)?,
            revision: 1,
        })
    }

    #[provide(ProviderCatalog::describe)]
    fn describe(
        _: &mut State,
        host: &Host<'_>,
        request: ProviderDetailsQuery,
    ) -> CallResult<ProviderDescriptor, ProviderError> {
        check(host, &request.scope)?;
        provider(&request.provider_id).map_err(CallError::Domain)
    }

    #[provide(ProviderCatalog::models)]
    fn models(
        _: &mut State,
        host: &Host<'_>,
        request: ModelQuery,
    ) -> CallResult<ModelPage, ProviderError> {
        check(host, &request.scope)?;
        connection(&request.provider_id, &request.connection_id).map_err(CallError::Domain)?;
        // A static provider entry does not prove credential readiness or a
        // model's inference transport. Runtime may later import an authorized
        // discovery plugin, which supplies the actual catalog.
        Err(CallError::Domain(error(
            ProviderErrorCode::AuthenticationRequired,
            "authenticated model discovery is not installed",
        )))
    }

    #[provide(ProviderAuth::status)]
    fn status(
        _: &mut State,
        host: &Host<'_>,
        request: AuthQuery,
    ) -> CallResult<AuthenticationStatus, ProviderError> {
        check(host, &request.scope)?;
        connection(&request.provider_id, &request.connection_id).map_err(CallError::Domain)?;
        Ok(AuthenticationStatus {
            provider_id: request.provider_id,
            connection_id: request.connection_id,
            profiles: Vec::new(),
        })
    }

    #[provide(ProviderAuth::begin)]
    fn begin(
        _: &mut State,
        host: &Host<'_>,
        request: AuthStart,
    ) -> CallResult<AuthChallenge, ProviderError> {
        check(host, &request.scope)?;
        let provider = provider(&request.provider_id).map_err(CallError::Domain)?;
        let chosen = provider
            .connections
            .iter()
            .find(|item| item.id == request.connection_id)
            .ok_or_else(|| {
                CallError::Domain(error(
                    ProviderErrorCode::NotFound,
                    "unknown provider connection",
                ))
            })?;
        if !chosen
            .auth_methods
            .iter()
            .any(|method| method.id == request.method_id)
        {
            return Err(CallError::Domain(error(
                ProviderErrorCode::InvalidInput,
                "authentication method not offered",
            )));
        }
        Err(CallError::Domain(error(
            ProviderErrorCode::Unavailable,
            "provider authentication backend is not installed",
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_sdk::{ContractMetadata, MetadataExecution};

    #[test]
    fn registered_providers_are_visible_without_authentication() {
        assert_eq!(catalog().unwrap().len(), 6);
        assert_eq!(provider("openai").unwrap().connections.len(), 2);
        assert_eq!(
            provider("missing").unwrap_err().code,
            ProviderErrorCode::NotFound
        );
    }

    #[test]
    fn an_unconfigured_connection_is_not_an_authenticated_model() {
        assert!(connection("openai", "api").unwrap());
        assert!(connection("anthropic", "api").unwrap());
        assert_eq!(
            connection("openai", "unknown").unwrap_err().code,
            ProviderErrorCode::NotFound
        );
    }

    #[test]
    fn native_plugin_defines_two_typed_operations() {
        let metadata = ContractMetadata::decode(plugin::METADATA.bytes()).unwrap();
        assert_ne!(metadata.execution, MetadataExecution::None);
        assert_eq!(metadata.provides.len(), 2);
    }
}
