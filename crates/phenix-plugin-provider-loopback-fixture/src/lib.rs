//! TEST-ONLY executable provider catalog for local ACP model-selection
//! product acceptance. Never part of production provider registrations, Basic,
//! default compositions, or distributed plug-in sets.
use phenix_provider_contracts::{
    AuthenticationStatus, ConnectionDescriptor, ModelOrigin, ModelPage, ProtocolBinding,
    ProviderDescriptor, ProviderError, ProviderErrorCode, ProviderModel,
};

fn failed(code: ProviderErrorCode, reason: &str) -> ProviderError {
    ProviderError {
        code,
        message: reason.into(),
    }
}

fn endpoint() -> Result<String, ProviderError> {
    let address = std::env::var("PHENIX_C9_TEST_HTTP_ENDPOINT")
        .map_err(|_| failed(ProviderErrorCode::Unavailable, "local test endpoint absent"))?;
    // Local fixture only: a literal loopback IPv4 endpoint, no credentials,
    // user-info, redirects, DNS, or externally supplied provider URL.
    let port = address
        .strip_prefix("http://127.0.0.1:")
        .and_then(|rest| rest.strip_suffix("/completions"))
        .filter(|port| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|port| port.parse::<u16>().ok())
        .filter(|port| *port != 0)
        .ok_or_else(|| {
            failed(
                ProviderErrorCode::Denied,
                "fixture endpoint must be loopback",
            )
        })?;
    Ok(format!("http://127.0.0.1:{port}/completions"))
}

fn descriptor() -> Result<ProviderDescriptor, ProviderError> {
    Ok(ProviderDescriptor {
        id: "c9-test-loopback".into(),
        label: "C9 test loopback (not a user provider)".into(),
        connections: vec![ConnectionDescriptor {
            id: "local".into(),
            label: "Local HTTP".into(),
            protocols: vec![ProtocolBinding {
                id: "openai-chat".into(),
                endpoint: Some(endpoint()?),
            }],
            base_url: None,
            auth_methods: vec![],
            model_sources: vec![],
        }],
    })
}

fn authorized_models() -> Result<ModelPage, ProviderError> {
    if std::env::var("PHENIX_C9_TEST_REVOKED").as_deref() == Ok("1") {
        return Err(failed(ProviderErrorCode::Denied, "fixture model revoked"));
    }
    endpoint()?;
    Ok(ModelPage {
        profile_id: None,
        revision: 7,
        next_cursor: None,
        models: ["fixture-a", "fixture-b"]
            .into_iter()
            .map(|id| ProviderModel {
                id: id.into(),
                label: id.into(),
                connection_id: "local".into(),
                protocol_id: "openai-chat".into(),
                endpoint_id: "chat".into(),
                capabilities: vec![],
                origin: ModelOrigin::Discovered,
                selectable: true,
            })
            .collect(),
    })
}

#[phenix_sdk::plugin(
    id = "phenix.provider.loopback-fixture",
    version = "1.0.0",
    profile = "state",
    provide(phenix_provider_contracts::ProviderCatalog, slot = "Catalog"),
    provide(phenix_provider_contracts::ProviderAuth, slot = "Authentication"),
    provide(phenix_provider_contracts::ProviderRuntime, slot = "Runtime")
)]
mod plugin {
    use super::*;
    use phenix_provider_contracts::{
        AuthAttemptQuery, AuthAttemptState, AuthChallenge, AuthQuery, AuthStart,
        AuthenticationCommand, AuthenticationEvent, ModelQuery, ProviderAuth, ProviderCatalog,
        ProviderCompletion, ProviderDetailsQuery, ProviderPage, ProviderQuery, ProviderRuntime,
    };
    use phenix_sdk::{CallError, CallResult, Host};

    #[derive(Default)]
    pub struct State;

    fn check(host: &Host<'_>, scope: &str) -> CallResult<(), ProviderError> {
        if scope != "user" || host.resource()?.as_deref() != Some("phenix.providers.user") {
            return Err(CallError::Domain(failed(
                ProviderErrorCode::Denied,
                "fixture provider resource denied",
            )));
        }
        Ok(())
    }

    fn identity(provider: &str, connection: &str) -> CallResult<(), ProviderError> {
        if provider != "c9-test-loopback" || connection != "local" {
            return Err(CallError::Domain(failed(
                ProviderErrorCode::NotFound,
                "unknown fixture provider identity",
            )));
        }
        Ok(())
    }

    // Canonical provider interface, backed by the *same* internal handlers as
    // the transitional catalog/auth ports. A protocol is not a second provider.
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
        request: ModelQuery,
    ) -> CallResult<ModelPage, ProviderError> {
        models(state, host, request)
    }

    // The loopback fixture advertises models for ACP selection, but it has no
    // installed invocation adapter in its own private state. Never claim a
    // successful model response by fabricating an inference transport.
    #[provide(ProviderRuntime::complete)]
    fn runtime_complete(
        host: &Host<'_>,
        request: ProviderCompletion,
    ) -> CallResult<phenix_basic_contracts::ModelReply, ProviderError> {
        check(host, &request.scope)?;
        identity(&request.provider_id, &request.connection_id)?;
        if request.profile_id.is_some()
            || request.model_id.is_empty()
            || request.request.session.is_empty()
            || request.request.run.is_empty()
        {
            return Err(CallError::Domain(failed(
                ProviderErrorCode::InvalidInput,
                "invalid fixture model selection",
            )));
        }
        let page = authorized_models().map_err(CallError::Domain)?;
        if !page.models.iter().any(|model| {
            model.id == request.model_id
                && model.connection_id == request.connection_id
                && model.selectable
        }) {
            return Err(CallError::Domain(failed(
                ProviderErrorCode::NotFound,
                "model is not selectable",
            )));
        }
        Err(CallError::Domain(failed(
            ProviderErrorCode::Unavailable,
            "selected protocol execution adapter is not installed",
        )))
    }

    #[provide(ProviderCatalog::list)]
    fn list(
        _: &mut State,
        host: &Host<'_>,
        request: ProviderQuery,
    ) -> CallResult<ProviderPage, ProviderError> {
        check(host, &request.scope)?;
        Ok(ProviderPage {
            revision: 7,
            providers: vec![descriptor().map_err(CallError::Domain)?],
        })
    }

    #[provide(ProviderCatalog::describe)]
    fn describe(
        _: &mut State,
        host: &Host<'_>,
        request: ProviderDetailsQuery,
    ) -> CallResult<ProviderDescriptor, ProviderError> {
        check(host, &request.scope)?;
        identity(&request.provider_id, "local")?;
        descriptor().map_err(CallError::Domain)
    }

    #[provide(ProviderCatalog::models)]
    fn models(
        _: &mut State,
        host: &Host<'_>,
        request: ModelQuery,
    ) -> CallResult<ModelPage, ProviderError> {
        check(host, &request.scope)?;
        identity(&request.provider_id, &request.connection_id)?;
        if request.profile_id.is_some()
            || request.cursor.is_some()
            || request.limit == 0
            || request.limit > 256
        {
            return Err(CallError::Domain(failed(
                ProviderErrorCode::InvalidInput,
                "fixture model query invalid",
            )));
        }
        authorized_models().map_err(CallError::Domain)
    }

    #[provide(ProviderAuth::status)]
    fn status(
        _: &mut State,
        host: &Host<'_>,
        request: AuthQuery,
    ) -> CallResult<AuthenticationStatus, ProviderError> {
        check(host, &request.scope)?;
        identity(&request.provider_id, &request.connection_id)?;
        Ok(AuthenticationStatus {
            provider_id: request.provider_id,
            connection_id: request.connection_id,
            profiles: vec![],
        })
    }

    #[provide(ProviderAuth::begin)]
    fn begin(
        _: &mut State,
        host: &Host<'_>,
        request: AuthStart,
    ) -> CallResult<AuthChallenge, ProviderError> {
        check(host, &request.scope)?;
        identity(&request.provider_id, &request.connection_id)?;
        Err(CallError::Domain(failed(
            ProviderErrorCode::Unavailable,
            "credential-free fixture has no login",
        )))
    }

    #[provide(ProviderAuth::poll)]
    fn poll(
        _: &mut State,
        host: &Host<'_>,
        request: AuthAttemptQuery,
    ) -> CallResult<AuthAttemptState, ProviderError> {
        check(host, &request.scope)?;
        identity(&request.provider_id, &request.connection_id)?;
        Err(CallError::Domain(failed(
            ProviderErrorCode::NotFound,
            "fixture has no authentication attempt",
        )))
    }

    #[provide(ProviderAuth::cancel)]
    fn cancel(
        _: &mut State,
        host: &Host<'_>,
        request: AuthAttemptQuery,
    ) -> CallResult<AuthAttemptState, ProviderError> {
        check(host, &request.scope)?;
        identity(&request.provider_id, &request.connection_id)?;
        Err(CallError::Domain(failed(
            ProviderErrorCode::NotFound,
            "fixture has no authentication attempt",
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_provider_contracts::{AuthQuery, AuthenticationCommand, AuthenticationEvent};
    use phenix_sdk::{ContractMetadata, Portable};

    #[test]
    fn canonical_interface_is_native_and_authentication_is_not_boolean() {
        let metadata = ContractMetadata::decode(plugin::METADATA.bytes()).unwrap();
        assert_eq!(metadata.provides.len(), 3);
        assert_eq!(metadata.requires.len(), 0);
        let command = AuthenticationCommand::Status {
            query: AuthQuery {
                scope: "user".into(),
                provider_id: "c9-test-loopback".into(),
                connection_id: "local".into(),
            },
        };
        let decoded = AuthenticationCommand::from_value(command.to_value()).unwrap();
        assert_eq!(decoded, command);
        let event = AuthenticationEvent::Status {
            value: AuthenticationStatus {
                provider_id: "c9-test-loopback".into(),
                connection_id: "local".into(),
                profiles: vec![],
            },
        };
        assert_eq!(
            AuthenticationEvent::from_value(event.to_value()).unwrap(),
            event
        );
    }
}
