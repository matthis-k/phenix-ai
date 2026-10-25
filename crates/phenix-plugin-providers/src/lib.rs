//! Shared M1 provider catalog/auth host. Provider definitions are separate
//! immutable registration contributions, not logic in this plugin.
//!
//! Until an executable strategy is selected, this host deliberately refuses
//! authentication and model discovery. It never grants authority from metadata.

mod auth;
mod models;
mod registration;

use phenix_provider_contracts::{PopulateProviders, ProvidersPopulate, ProvidersReady};
use phenix_sdk::{FailurePolicy, JoinPolicy, StaticGraphContribution, StaticGraphNode};

/// Declare the shared event once. Contributors add their own nodes without
/// duplicating the event or any scheduling mechanism.
pub const GRAPH: StaticGraphContribution = StaticGraphContribution {
    plugin: "phenix.provider.host",
    events: &[ProvidersPopulate::EVENT, ProvidersReady::EVENT],
    actions: &[PopulateProviders::ACTION],
    nodes: &[
        StaticGraphNode {
            id: "on-begin",
            triggers: &[phenix_sdk::KERNEL_BEGIN],
            after: &[],
            selector: "",
            operation: "phenix.provider.startup@1/begin",
            writes: &[],
            join: JoinPolicy::AllActive,
            failure: FailurePolicy::FailRoot,
        },
        StaticGraphNode {
            id: "on-populated",
            triggers: &["settled:phenix.providers.populate"],
            after: &[],
            selector: "",
            operation: "phenix.provider.settlement@1/complete",
            writes: &[],
            join: JoinPolicy::AllActive,
            failure: FailurePolicy::FailRoot,
        },
    ],
};

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
    provide(phenix_provider_contracts::ProviderAuth, slot = "Authentication"),
    provide(phenix_provider_contracts::ProviderRuntime, slot = "Runtime"),
    provide(phenix_provider_contracts::ProviderRegistry, slot = "Registry"),
    provide(phenix_provider_contracts::ProviderStartup, slot = "Startup"),
    provide(phenix_provider_contracts::ProviderSettlement, slot = "Settlement"),
    graph(super::GRAPH)
)]
mod plugin {
    use super::*;
    use phenix_provider_contracts::{
        AuthAttemptQuery, AuthAttemptState, AuthChallenge, AuthQuery, AuthStart,
        AuthenticatedProfile, AuthenticationCommand, AuthenticationEvent, CredentialIdentity,
        CredentialProfile, CredentialProfileStatus, ModelQuery, ProviderAuth, ProviderCatalog,
        ProviderCompletion, ProviderCredentials, ProviderDetailsQuery, ProviderKernelBegin,
        ProviderPage, ProviderQuery, ProviderRegistration, ProviderRegistrations, ProviderRegistry,
        ProviderRuntime, ProviderSettlement, ProviderStartup, ProvidersPopulated,
    };
    use phenix_sdk::{CallError, CallResult, Host};

    #[derive(Default)]
    pub struct State {
        /// Only an explicitly authorized population event can stage declarations.
        registry: registration::Registry,
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

    /// Read the single already-authorized provider resource from the live
    /// call. A contributor cannot choose or forge its own target scope.
    fn current_scope(host: &Host<'_>) -> CallResult<String, ProviderError> {
        let resource = host.resource()?.ok_or_else(|| {
            CallError::Domain(error(
                ProviderErrorCode::Denied,
                "provider event requires an authorized resource",
            ))
        })?;
        let scope = resource.strip_prefix("phenix.providers.").ok_or_else(|| {
            CallError::Domain(error(
                ProviderErrorCode::Denied,
                "provider event resource is not a provider scope",
            ))
        })?;
        check(host, scope)?;
        Ok(scope.into())
    }

    /// Read only the preselected, verified M1 contributions and pin their
    /// validated snapshot to the plugin-owned state for this generation.
    fn selected_catalog<'a>(
        state: &'a mut State,
        host: &Host<'_>,
    ) -> CallResult<&'a [ProviderDescriptor], ProviderError> {
        if state.registry.selected().is_none() {
            // An active or failed event wave can never expose an uncommitted
            // snapshot or silently select the legacy pull adapter.
            if state.registry.blocks_pull_catalog() {
                return Err(CallError::Domain(error(
                    ProviderErrorCode::Unavailable,
                    "provider population has not been published",
                )));
            }
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
            state
                .registry
                .select_legacy(selected)
                .map_err(CallError::Domain)?;
        }
        state.registry.selected().ok_or_else(|| {
            CallError::Domain(error(
                ProviderErrorCode::Unavailable,
                "provider catalog has not been published",
            ))
        })
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

    // The shared host is the canonical provider execution-facing interface.
    // Legacy auth/catalog ports are simple aliases to these exact handlers,
    // rather than distinct provider-specific implementations.
    #[provide(ProviderRuntime::authenticate)]
    fn runtime_authenticate(
        state: &mut State,
        host: &Host<'_>,
        command: AuthenticationCommand,
    ) -> CallResult<AuthenticationEvent, ProviderError> {
        match command {
            AuthenticationCommand::Status { query } => {
                status(state, host, query).map(|value| AuthenticationEvent::Status { value })
            }
            AuthenticationCommand::Begin { request } => {
                begin(state, host, request).map(|value| AuthenticationEvent::Challenge { value })
            }
            AuthenticationCommand::Poll { request } => {
                poll(state, host, request).map(|value| AuthenticationEvent::Attempt { value })
            }
            AuthenticationCommand::Cancel { request } => {
                cancel(state, host, request).map(|value| AuthenticationEvent::Attempt { value })
            }
        }
    }

    #[provide(ProviderRuntime::models)]
    fn runtime_models(
        state: &mut State,
        host: &Host<'_>,
        query: ModelQuery,
    ) -> CallResult<ModelPage, ProviderError> {
        models(state, host, query)
    }

    #[provide(ProviderRuntime::complete)]
    fn runtime_complete(
        state: &mut State,
        host: &Host<'_>,
        request: ProviderCompletion,
    ) -> CallResult<phenix_basic_contracts::ModelReply, ProviderError> {
        if request.request.session.is_empty() || request.request.run.is_empty() {
            return Err(CallError::Domain(error(
                ProviderErrorCode::InvalidInput,
                "missing canonical model session or run",
            )));
        }
        // Run precisely the same account-bound, verified catalog path used
        // for UI selection. A stale or cross-account model ID is not a grant.
        // A listed model is still not an executable protocol strategy.
        let authorized = models(
            state,
            host,
            ModelQuery {
                scope: request.scope.clone(),
                provider_id: request.provider_id.clone(),
                connection_id: request.connection_id.clone(),
                profile_id: request.profile_id.clone(),
                cursor: None,
                limit: 256,
            },
        )?;
        if request.profile_id != authorized.profile_id {
            return Err(CallError::Domain(error(
                ProviderErrorCode::Denied,
                "selected model account does not match verified account",
            )));
        }
        if !authorized.models.iter().any(|model| {
            model.id == request.model_id
                && model.connection_id == request.connection_id
                && model.selectable
        }) {
            return Err(CallError::Domain(error(
                ProviderErrorCode::NotFound,
                "model is not selectable for the authorized account",
            )));
        }
        Err(CallError::Domain(error(
            ProviderErrorCode::Unavailable,
            "selected session protocol adapter is not installed",
        )))
    }

    /// Kernel.begin is a fixed graph trigger. This manager owns the event
    /// definition and initiates the next wave inside the SAME graph root.
    /// Provider-definition plugins only subscribe and call register.
    #[provide(ProviderStartup::begin)]
    fn on_begin(
        state: &mut State,
        host: &Host<'_>,
        _: ProviderKernelBegin,
    ) -> CallResult<(), ProviderError> {
        let scope = current_scope(host)?;
        if state.registry.selected().is_some() {
            return Err(CallError::Domain(error(
                ProviderErrorCode::Conflict,
                "provider catalog already selected",
            )));
        }
        state.registry.begin().map_err(CallError::Domain)?;
        if let Err(failure) = host.emit_event(&ProvidersPopulate { scope }) {
            // No partially initialized catalog if admission of the event
            // itself fails. A graph failure after emission is handled by the
            // controller's abort path and never becomes a selected snapshot.
            let _ = state.registry.abort();
            return Err(CallError::Kernel(failure));
        }
        Ok(())
    }

    /// Owner-only completion trigger. Runs after EVERY providers.populate
    /// subscriber (including pending work) has finished successfully.
    /// No host-side provider-specific publish coordination is needed.
    #[provide(ProviderSettlement::complete)]
    fn on_populated(
        state: &mut State,
        host: &Host<'_>,
        _: ProvidersPopulated,
    ) -> CallResult<(), ProviderError> {
        let scope = current_scope(host)?;
        if state.registry.selected().is_some() {
            return Err(CallError::Domain(error(
                ProviderErrorCode::Conflict,
                "provider catalog is already selected",
            )));
        }
        // Emission is deferred to the next graph wave: if publication fails,
        // the parent root stops and the pending ready notice is discarded.
        // Enqueue before committing so a full queue cannot expose a partial
        // initialization as successful.
        host.emit_event(&ProvidersReady { scope, revision: 1 })?;
        state.registry.publish().map_err(CallError::Domain)?;
        Ok(())
    }

    /// Controller-only: prevent catalog reads before the first graph node runs.
    #[provide(ProviderRegistry::begin)]
    fn begin_population(
        state: &mut State,
        host: &Host<'_>,
        request: ProviderQuery,
    ) -> CallResult<(), ProviderError> {
        check(host, &request.scope)?;
        if state.registry.selected().is_some() {
            return Err(CallError::Domain(error(
                ProviderErrorCode::Conflict,
                "provider catalog is already selected",
            )));
        }
        state.registry.begin().map_err(CallError::Domain)
    }

    /// Existing graph handlers call this ordinary typed contract operation.
    /// The kernel checks the selected import, scope and caller grant.
    #[provide(ProviderRegistry::register)]
    fn register(
        state: &mut State,
        host: &Host<'_>,
        request: ProviderRegistration,
    ) -> CallResult<(), ProviderError> {
        check(host, &request.scope)?;
        if state.registry.selected().is_some() {
            return Err(CallError::Domain(error(
                ProviderErrorCode::Conflict,
                "provider catalog is already selected",
            )));
        }
        state
            .registry
            .register(request.provider)
            .map_err(CallError::Domain)
    }

    /// The population controller invokes publish *after* the graph completes.
    /// This operation needs its own explicit grant, separate from register.
    #[provide(ProviderRegistry::publish)]
    fn publish(
        state: &mut State,
        host: &Host<'_>,
        request: ProviderQuery,
    ) -> CallResult<ProviderPage, ProviderError> {
        check(host, &request.scope)?;
        if state.registry.selected().is_some() {
            return Err(CallError::Domain(error(
                ProviderErrorCode::Conflict,
                "provider catalog is already selected",
            )));
        }
        state.registry.publish().map_err(CallError::Domain)
    }

    /// Controller-only: discard the failed event wave without publishing data.
    #[provide(ProviderRegistry::abort)]
    fn abort_population(
        state: &mut State,
        host: &Host<'_>,
        request: ProviderQuery,
    ) -> CallResult<(), ProviderError> {
        check(host, &request.scope)?;
        state.registry.abort().map_err(CallError::Domain)
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
        assert_eq!(metadata.provides.len(), 6);
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
