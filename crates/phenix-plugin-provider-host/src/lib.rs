//! Shared M1 provider catalog/auth host. Provider definitions are separate
//! immutable registration contributions, not logic in this plugin.
//!
//! Until an executable strategy is selected, this host deliberately refuses
//! authentication and model discovery. It never grants authority from metadata.

use phenix_provider_builtins::merge_registration_pages;
use phenix_provider_contracts::{
    AuthenticationStatus, ModelPage, ProviderDescriptor, ProviderError, ProviderErrorCode,
};

fn error(code: ProviderErrorCode, message: &str) -> ProviderError {
    ProviderError {
        code,
        message: message.into(),
    }
}

fn missing_model_catalog(requires_auth: bool) -> ProviderError {
    if requires_auth {
        error(
            ProviderErrorCode::AuthenticationRequired,
            "authenticated model discovery is not installed",
        )
    } else {
        error(
            ProviderErrorCode::Unavailable,
            "model discovery is not installed for this credential-free connection",
        )
    }
}

#[phenix_sdk::plugin(
    id = "phenix.provider.host",
    version = "1.0.0",
    profile = "state",
    provide(phenix_provider_contracts::ProviderCatalog, slot = "Catalog"),
    require(
        phenix_provider_contracts::ProviderRegistrations,
        slot = "Contributors",
        many(min = 0)
    ),
    provide(phenix_provider_contracts::ProviderAuth, slot = "Authentication")
)]
mod plugin {
    use super::*;
    use phenix_provider_contracts::{
        AuthAttemptQuery, AuthAttemptState, AuthChallenge, AuthQuery, AuthStart, ModelQuery,
        ProviderAuth, ProviderCatalog, ProviderDetailsQuery, ProviderPage, ProviderQuery,
        ProviderRegistrations,
    };
    use phenix_sdk::{CallError, CallResult, Host};

    #[derive(Default)]
    pub struct State {
        /// One immutable snapshot per plugin generation, not mutable Storage.
        selected: Option<Vec<ProviderDescriptor>>,
    }

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

    /// Read only the preselected, verified M1 contributions and pin their
    /// validated snapshot to the plugin-owned state for this generation.
    fn selected_catalog<'a>(
        state: &'a mut State,
        host: &Host<'_>,
    ) -> CallResult<&'a [ProviderDescriptor], ProviderError> {
        if state.selected.is_none() {
            let count = imports::Contributors::count(host)?;
            if count > 256 {
                return Err(CallError::Domain(error(
                    ProviderErrorCode::InvalidInput,
                    "too many selected provider contributors",
                )));
            }
            if count == 0 {
                return Err(CallError::Domain(error(
                    ProviderErrorCode::Unavailable,
                    "no provider registration contributor installed",
                )));
            }
            let mut pages = Vec::with_capacity(count);
            for index in 0..count {
                pages.push(imports::Contributors::invoke_at::<
                    ProviderRegistrations::list,
                >(host, index, &())?);
            }
            let selected = merge_registration_pages(pages).map_err(|_| {
                CallError::Domain(error(
                    ProviderErrorCode::InvalidInput,
                    "invalid or conflicting selected provider registrations",
                ))
            })?;
            state.selected = Some(selected);
        }
        Ok(state
            .selected
            .as_deref()
            .expect("initialized provider catalog"))
    }

    fn selected_provider(
        state: &mut State,
        host: &Host<'_>,
        id: &str,
    ) -> CallResult<ProviderDescriptor, ProviderError> {
        selected_catalog(state, host)?
            .iter()
            .find(|item| item.id == id)
            .cloned()
            .ok_or_else(|| {
                CallError::Domain(error(
                    ProviderErrorCode::NotFound,
                    "unknown installed provider",
                ))
            })
    }

    fn selected_connection(
        state: &mut State,
        host: &Host<'_>,
        provider_id: &str,
        connection_id: &str,
    ) -> CallResult<bool, ProviderError> {
        selected_provider(state, host, provider_id)?
            .connections
            .iter()
            .find(|item| item.id == connection_id)
            .map(|item| !item.auth_methods.is_empty())
            .ok_or_else(|| {
                CallError::Domain(error(
                    ProviderErrorCode::NotFound,
                    "unknown provider connection",
                ))
            })
    }

    #[provide(ProviderCatalog::list)]
    fn list(
        state: &mut State,
        host: &Host<'_>,
        request: ProviderQuery,
    ) -> CallResult<ProviderPage, ProviderError> {
        check(host, &request.scope)?;
        Ok(ProviderPage {
            providers: selected_catalog(state, host)?.to_vec(),
            revision: 1,
        })
    }

    #[provide(ProviderCatalog::describe)]
    fn describe(
        state: &mut State,
        host: &Host<'_>,
        request: ProviderDetailsQuery,
    ) -> CallResult<ProviderDescriptor, ProviderError> {
        check(host, &request.scope)?;
        selected_provider(state, host, &request.provider_id)
    }

    #[provide(ProviderCatalog::models)]
    fn models(
        state: &mut State,
        host: &Host<'_>,
        request: ModelQuery,
    ) -> CallResult<ModelPage, ProviderError> {
        check(host, &request.scope)?;
        let requires_auth =
            selected_connection(state, host, &request.provider_id, &request.connection_id)?;
        // Registration alone proves neither model entitlement nor a usable
        // inference transport. A credential-free connection must not be
        // misreported as requiring authentication when discovery is absent.
        Err(CallError::Domain(missing_model_catalog(requires_auth)))
    }

    #[provide(ProviderAuth::status)]
    fn status(
        state: &mut State,
        host: &Host<'_>,
        request: AuthQuery,
    ) -> CallResult<AuthenticationStatus, ProviderError> {
        check(host, &request.scope)?;
        selected_connection(state, host, &request.provider_id, &request.connection_id)?;
        Ok(AuthenticationStatus {
            provider_id: request.provider_id,
            connection_id: request.connection_id,
            profiles: Vec::new(),
        })
    }

    #[provide(ProviderAuth::begin)]
    fn begin(
        state: &mut State,
        host: &Host<'_>,
        request: AuthStart,
    ) -> CallResult<AuthChallenge, ProviderError> {
        check(host, &request.scope)?;
        let provider = selected_provider(state, host, &request.provider_id)?;
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

    #[provide(ProviderAuth::poll)]
    fn poll(
        state: &mut State,
        host: &Host<'_>,
        request: AuthAttemptQuery,
    ) -> CallResult<AuthAttemptState, ProviderError> {
        check(host, &request.scope)?;
        selected_connection(state, host, &request.provider_id, &request.connection_id)?;
        Err(CallError::Domain(error(
            ProviderErrorCode::NotFound,
            "no authentication attempt registered",
        )))
    }

    #[provide(ProviderAuth::cancel)]
    fn cancel(
        state: &mut State,
        host: &Host<'_>,
        request: AuthAttemptQuery,
    ) -> CallResult<AuthAttemptState, ProviderError> {
        check(host, &request.scope)?;
        selected_connection(state, host, &request.provider_id, &request.connection_id)?;
        Err(CallError::Domain(error(
            ProviderErrorCode::NotFound,
            "no authentication attempt registered",
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_sdk::{ContractMetadata, MetadataExecution};

    #[test]
    fn metadata_exposes_shared_host_without_provider_registrations() {
        let metadata = ContractMetadata::decode(plugin::METADATA.bytes()).unwrap();
        assert_ne!(metadata.execution, MetadataExecution::None);
        assert_eq!(metadata.provides.len(), 2);
    }

    #[test]
    fn missing_strategy_fails_closed_without_entitlement() {
        assert_eq!(
            missing_model_catalog(true).code,
            ProviderErrorCode::AuthenticationRequired
        );
        assert_eq!(
            missing_model_catalog(false).code,
            ProviderErrorCode::Unavailable
        );
    }
}
