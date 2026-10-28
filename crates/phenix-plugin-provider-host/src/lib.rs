//! Shared M1 provider catalog/auth host. Provider definitions are separate
//! immutable registration contributions, not logic in this plugin.
//!
//! Until an executable strategy is selected, this host deliberately refuses
//! authentication and model discovery. It never grants authority from metadata.

mod auth;
mod models;

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
    require(
        phenix_provider_contracts::ProviderCredentials,
        slot = "Credentials",
        many(min = 0)
    ),
    provide(phenix_provider_contracts::ProviderAuth, slot = "Authentication")
)]
mod plugin {
    use super::*;
    use phenix_provider_contracts::{
        AuthAttemptQuery, AuthAttemptState, AuthChallenge, AuthQuery, AuthStart,
        AuthenticatedProfile, CredentialIdentity, CredentialProfile, CredentialProfileStatus,
        ModelQuery, ProviderAuth, ProviderCatalog, ProviderCredentials, ProviderDetailsQuery,
        ProviderPage, ProviderQuery, ProviderRegistrations,
    };
    use phenix_sdk::{CallError, CallResult, Host};

    #[derive(Default)]
    pub struct State {
        /// One immutable snapshot per plugin generation, not mutable Storage.
        selected: Option<Vec<ProviderDescriptor>>,
        /// Shared, account-isolated attempt lifecycle owned by this host.
        attempts: phenix_provider_auth_strategies::ApiKeyAttempts,
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

    fn connection_descriptor(
        state: &mut State,
        host: &Host<'_>,
        provider_id: &str,
        connection_id: &str,
    ) -> CallResult<phenix_provider_contracts::ConnectionDescriptor, ProviderError> {
        selected_provider(state, host, provider_id)?
            .connections
            .into_iter()
            .find(|item| item.id == connection_id)
            .ok_or_else(|| {
                CallError::Domain(error(
                    ProviderErrorCode::NotFound,
                    "unknown provider connection",
                ))
            })
    }

    fn selected_connection(
        state: &mut State,
        host: &Host<'_>,
        provider_id: &str,
        connection_id: &str,
    ) -> CallResult<bool, ProviderError> {
        Ok(
            !connection_descriptor(state, host, provider_id, connection_id)?
                .auth_methods
                .is_empty(),
        )
    }

    /// Revalidate selected credentials against the real declared catalog.
    /// Stored credentials alone never constitute successful authentication.
    fn verified_key(
        host: &Host<'_>,
        connection: &phenix_provider_contracts::ConnectionDescriptor,
        identity: &CredentialIdentity,
    ) -> CallResult<bool, ProviderError> {
        if !auth::standard_bearer_key(connection, "api-key") || !models::can_discover(connection) {
            return Ok(false);
        }
        if imports::Credentials::count(host)? != 1 {
            return Ok(false);
        }
        let scope = identity.resolve_scope().map_err(CallError::Domain)?;
        let secret = match host.invoke_scoped::<ProviderCredentials::resolve>(
            "Credentials#0",
            &scope,
            identity,
        ) {
            Ok(value) => value,
            Err(CallError::Domain(_)) => return Ok(false),
            Err(failure) => return Err(failure),
        };
        Ok(models::discover(connection, identity.profile_id(), &secret, 256).is_ok())
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
        // The management root is intentionally not a credential root. A
        // profile-bearing query must be invoked under the exact selected
        // credential resource; otherwise a provider-list grant could be
        // abused to choose an arbitrary named account.
        let selected_identity = match request.profile_id.as_deref() {
            Some(profile) => {
                if request.scope != "user" {
                    return Err(CallError::Domain(error(
                        ProviderErrorCode::Denied,
                        "provider management scope unauthorized",
                    )));
                }
                let identity =
                    CredentialIdentity::new(&request.provider_id, &request.connection_id, profile)
                        .map_err(CallError::Domain)?;
                let expected = identity.resource().map_err(CallError::Domain)?;
                if host.resource()?.as_deref() != Some(expected.as_str()) {
                    return Err(CallError::Domain(error(
                        ProviderErrorCode::Denied,
                        "model account credential root not authorized",
                    )));
                }
                Some(identity)
            }
            None => {
                check(host, &request.scope)?;
                None
            }
        };
        let connection =
            connection_descriptor(state, host, &request.provider_id, &request.connection_id)?;
        if request.cursor.is_some() || !(1..=256).contains(&request.limit) {
            return Err(CallError::Domain(error(
                ProviderErrorCode::InvalidInput,
                "invalid authorized model page request",
            )));
        }
        let Some(identity) = selected_identity else {
            return Err(CallError::Domain(missing_model_catalog(
                !connection.auth_methods.is_empty(),
            )));
        };
        if !super::models::can_discover(&connection) {
            return Err(CallError::Domain(error(
                ProviderErrorCode::Unavailable,
                "selected model discovery strategy is not installed",
            )));
        }
        let count = imports::Credentials::count(host)?;
        if count == 0 {
            return Err(CallError::Domain(error(
                ProviderErrorCode::AuthenticationRequired,
                "credential strategy is not installed",
            )));
        }
        if count != 1 {
            return Err(CallError::Domain(error(
                ProviderErrorCode::Unavailable,
                "ambiguous credential strategies",
            )));
        }
        let scope = identity.resolve_scope().map_err(CallError::Domain)?;
        // The selected caller root is already this exact account; the
        // attenuated #0 credential import cannot broaden resources or ops.
        let credential =
            host.invoke_scoped::<ProviderCredentials::resolve>("Credentials#0", &scope, &identity)?;
        super::models::discover(
            &connection,
            identity.profile_id(),
            &credential,
            request.limit,
        )
        .map_err(CallError::Domain)
    }

    #[provide(ProviderAuth::status)]
    fn status(
        state: &mut State,
        host: &Host<'_>,
        request: AuthQuery,
    ) -> CallResult<AuthenticationStatus, ProviderError> {
        let account = auth::account(
            host.resource()?.as_deref(),
            &request.scope,
            &request.provider_id,
            &request.connection_id,
        )
        .map_err(CallError::Domain)?;
        let connection =
            connection_descriptor(state, host, &request.provider_id, &request.connection_id)?;
        let profiles = if let Some(identity) = account {
            if auth::standard_bearer_key(&connection, "api-key") {
                let verified = verified_key(host, &connection, &identity)?;
                vec![CredentialProfile {
                    id: identity.profile_id().into(),
                    label: identity.profile_id().into(),
                    method_id: "api-key".into(),
                    status: if verified {
                        CredentialProfileStatus::Authenticated
                    } else {
                        CredentialProfileStatus::Unavailable {
                            reason: "account credential verification unavailable".into(),
                        }
                    },
                }]
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        Ok(AuthenticationStatus {
            provider_id: request.provider_id,
            connection_id: request.connection_id,
            profiles,
        })
    }

    #[provide(ProviderAuth::begin)]
    fn begin(
        state: &mut State,
        host: &Host<'_>,
        request: AuthStart,
    ) -> CallResult<AuthChallenge, ProviderError> {
        let account = auth::account(
            host.resource()?.as_deref(),
            &request.scope,
            &request.provider_id,
            &request.connection_id,
        )
        .map_err(CallError::Domain)?;
        let chosen =
            connection_descriptor(state, host, &request.provider_id, &request.connection_id)?;
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
        if let Some(identity) = account {
            if auth::standard_bearer_key(&chosen, &request.method_id)
                && models::can_discover(&chosen)
                && imports::Credentials::count(host)? == 1
            {
                return state
                    .attempts
                    .begin(
                        identity,
                        "phenix-key",
                        "Import an API key into this authorized account and poll the attempt",
                    )
                    .map_err(CallError::Domain);
            }
        }
        Err(CallError::Domain(error(
            ProviderErrorCode::Unavailable,
            "selected authentication strategy is not installed",
        )))
    }

    #[provide(ProviderAuth::poll)]
    fn poll(
        state: &mut State,
        host: &Host<'_>,
        request: AuthAttemptQuery,
    ) -> CallResult<AuthAttemptState, ProviderError> {
        let account = auth::account(
            host.resource()?.as_deref(),
            &request.scope,
            &request.provider_id,
            &request.connection_id,
        )
        .map_err(CallError::Domain)?;
        let connection =
            connection_descriptor(state, host, &request.provider_id, &request.connection_id)?;
        let identity = account.ok_or_else(|| {
            CallError::Domain(error(
                ProviderErrorCode::NotFound,
                "no authentication attempt registered",
            ))
        })?;
        if !auth::standard_bearer_key(&connection, "api-key") {
            return Err(CallError::Domain(error(
                ProviderErrorCode::NotFound,
                "no authentication attempt registered",
            )));
        }
        if let Some(done) = state
            .attempts
            .check(&identity, &request.attempt_id)
            .map_err(CallError::Domain)?
        {
            return Ok(done);
        }
        if verified_key(host, &connection, &identity)? {
            state
                .attempts
                .complete(
                    &identity,
                    &request.attempt_id,
                    AuthenticatedProfile {
                        id: identity.profile_id().into(),
                        label: identity.profile_id().into(),
                        method_id: "api-key".into(),
                    },
                )
                .map_err(CallError::Domain)
        } else {
            Ok(AuthAttemptState::Pending)
        }
    }

    #[provide(ProviderAuth::cancel)]
    fn cancel(
        state: &mut State,
        host: &Host<'_>,
        request: AuthAttemptQuery,
    ) -> CallResult<AuthAttemptState, ProviderError> {
        let account = auth::account(
            host.resource()?.as_deref(),
            &request.scope,
            &request.provider_id,
            &request.connection_id,
        )
        .map_err(CallError::Domain)?;
        connection_descriptor(state, host, &request.provider_id, &request.connection_id)?;
        let identity = account.ok_or_else(|| {
            CallError::Domain(error(
                ProviderErrorCode::NotFound,
                "no authentication attempt registered",
            ))
        })?;
        state
            .attempts
            .cancel(&identity, &request.attempt_id)
            .map_err(CallError::Domain)
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
