use crate::{extension_catalog, wire, ApplicationAdapter};
use phenix_application_interface::{
    application_descriptor, types::ApplicationError, ApplicationTransport,
};
use phenix_core::{ContractId, PhenixContract, PhenixValue, ValueCodec};
use std::sync::Arc;
use wire::schema::v1::{ExtRequest, ExtResponse};

impl<T: ApplicationTransport> ApplicationAdapter<T> {
    pub fn extension_callback_request<R: PhenixContract + ValueCodec>(
        &self,
        request: &R,
    ) -> Result<(ContractId, ExtRequest), ApplicationError> {
        let (callback, projection) = callback_projection_for_request::<T, R>(self)?;
        let value = request.to_value();
        projection
            .request
            .parse(&value)
            .map_err(|error| ApplicationError::InvalidInput {
                message: format!(
                    "ACP extension callback request violates the application contract: {error}"
                ),
            })?;
        let raw = serde_json::value::to_raw_value(&value).map_err(|error| {
            ApplicationError::InvalidInput {
                message: format!("cannot encode ACP extension callback request: {error}"),
            }
        })?;
        Ok((callback, ExtRequest::new(projection.method, Arc::from(raw))))
    }

    pub fn extension_callback_response<R: PhenixContract + ValueCodec>(
        &self,
        callback: &ContractId,
        response: &ExtResponse,
    ) -> Result<R, ApplicationError> {
        let descriptor = application_descriptor();
        let declaration = descriptor.callbacks.get(callback).ok_or_else(|| {
            ApplicationError::InvalidResponse {
                message: format!("unknown application callback {callback}"),
            }
        })?;
        let response_contract = R::contract_id();
        if declaration.response != response_contract {
            return Err(ApplicationError::InvalidResponse {
                message: format!(
                    "application callback {callback} returns {}, not {response_contract}",
                    declaration.response
                ),
            });
        }
        let projection = callback_projection_with_descriptor(self, &descriptor, callback)?;
        let encoded =
            serde_json::to_value(response).map_err(|error| ApplicationError::InvalidResponse {
                message: format!("cannot read ACP extension callback response: {error}"),
            })?;
        let value = serde_json::from_value::<PhenixValue>(encoded).map_err(|error| {
            ApplicationError::InvalidResponse {
                message: format!("cannot decode ACP extension callback response: {error}"),
            }
        })?;
        projection
            .response
            .parse(&value)
            .map_err(|error| ApplicationError::InvalidResponse {
                message: format!(
                    "ACP extension callback response violates the application contract: {error}"
                ),
            })?;
        R::from_value(&value).map_err(|error| ApplicationError::InvalidResponse {
            message: format!("cannot decode typed application callback response: {error}"),
        })
    }
}

fn callback_projection_for_request<T, R>(
    adapter: &ApplicationAdapter<T>,
) -> Result<(ContractId, crate::ExtensionCallback), ApplicationError>
where
    T: ApplicationTransport,
    R: PhenixContract,
{
    let descriptor = application_descriptor();
    let request = R::contract_id();
    let callback = descriptor
        .callbacks
        .iter()
        .find_map(|(callback, declaration)| {
            (declaration.request == request).then_some(callback.clone())
        })
        .ok_or_else(|| ApplicationError::InvalidInput {
            message: format!("no application callback accepts request type {request}"),
        })?;
    callback_projection_with_descriptor(adapter, &descriptor, &callback)
        .map(|projection| (callback, projection))
}

fn callback_projection_with_descriptor<T: ApplicationTransport>(
    adapter: &ApplicationAdapter<T>,
    descriptor: &phenix_application_interface::ApplicationDescriptor,
    callback: &ContractId,
) -> Result<crate::ExtensionCallback, ApplicationError> {
    let declaration =
        descriptor
            .callbacks
            .get(callback)
            .ok_or_else(|| ApplicationError::InvalidInput {
                message: format!("unknown application callback {callback}"),
            })?;
    adapter
        .application_capabilities()
        .require(&declaration.capability)?;
    extension_catalog(descriptor, adapter.application_capabilities())
        .callbacks
        .into_iter()
        .find(|candidate| candidate.callback == *callback)
        .ok_or_else(|| ApplicationError::InvalidInput {
            message: format!(
                "application callback {callback} uses standard ACP rather than an extension"
            ),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_application_interface::types::{
        CallableInvocation, CallableInvocationResult, Empty, PermissionRequest,
    };
    use phenix_core::{
        CallableRef, ClientConnectionId, ReferenceGenerationId, ReferenceId, ReferenceOwnerId,
        SessionId,
    };
    use std::future::ready;

    struct NoopTransport;

    impl ApplicationTransport for NoopTransport {
        fn invoke(
            &self,
            _operation: &ContractId,
            _input: PhenixValue,
        ) -> impl std::future::Future<Output = Result<PhenixValue, ApplicationError>> {
            ready(Err(ApplicationError::Failed {
                message: "callback framing must not invoke the application transport".to_owned(),
            }))
        }
    }

    fn contract(value: &str) -> ContractId {
        ContractId::parse(value).expect("valid contract id")
    }

    fn adapter(extra: &[&str]) -> ApplicationAdapter<NoopTransport> {
        let advertised = [
            "phenix.application.capability.discovery@1",
            "phenix.application.capability.sessions@1",
            "phenix.application.capability.prompt@1",
        ]
        .into_iter()
        .chain(extra.iter().copied())
        .map(contract)
        .collect::<Vec<_>>();
        ApplicationAdapter::new(NoopTransport, advertised).expect("adapter capabilities")
    }

    fn callback_id() -> ContractId {
        contract("phenix.application.capability-call@1")
    }

    fn request() -> CallableInvocation {
        CallableInvocation {
            callable: PhenixValue::Callable(CallableRef::new(
                ContractId::parse("fixture.callback@1").expect("callable contract"),
                ReferenceOwnerId::Client(ClientConnectionId::parse("fixture-client").unwrap()),
                ReferenceGenerationId::parse("generation-1").unwrap(),
                ReferenceId::parse("callback-1").unwrap(),
            )),
            input: PhenixValue::String("continue?".to_owned()),
        }
    }

    fn response() -> CallableInvocationResult {
        CallableInvocationResult {
            output: PhenixValue::String("yes".to_owned()),
        }
    }

    fn encoded_response(value: &impl ValueCodec) -> ExtResponse {
        let raw = serde_json::value::to_raw_value(&value.to_value()).expect("callback response");
        ExtResponse::new(Arc::from(raw))
    }

    #[test]
    fn callback_request_resolves_descriptor_identity_and_schema() {
        let adapter = adapter(&["phenix.application.capability.capabilities@1"]);
        let request = request();
        let (callback, translated) = adapter
            .extension_callback_request(&request)
            .expect("extension callback request");

        assert_eq!(callback, callback_id());
        assert_eq!(translated.method.as_ref(), "_phenix/capability-call@1");
        let params: PhenixValue =
            serde_json::from_str(translated.params.get()).expect("callback params");
        assert_eq!(params, request.to_value());
    }

    #[test]
    fn callback_response_round_trips_typed_application_value() {
        let adapter = adapter(&["phenix.application.capability.capabilities@1"]);
        let expected = response();
        let translated: CallableInvocationResult = adapter
            .extension_callback_response(&callback_id(), &encoded_response(&expected))
            .expect("typed callback response");
        assert_eq!(translated, expected);
    }

    #[test]
    fn callback_requires_its_negotiated_capability() {
        let adapter = adapter(&[]);
        let error = adapter
            .extension_callback_request(&request())
            .expect_err("callback capability must be negotiated");

        assert_eq!(
            error,
            ApplicationError::UnsupportedCapability {
                capability: contract("phenix.application.capability.capabilities@1"),
            }
        );
    }

    #[test]
    fn callback_response_rejects_the_wrong_descriptor_shape() {
        let adapter = adapter(&["phenix.application.capability.capabilities@1"]);
        let result = adapter.extension_callback_response::<CallableInvocationResult>(
            &callback_id(),
            &encoded_response(&Empty {}),
        );
        assert!(matches!(
            result,
            Err(ApplicationError::InvalidResponse { .. })
        ));
    }

    #[test]
    fn callback_response_rejects_the_wrong_typed_contract() {
        let adapter = adapter(&["phenix.application.capability.capabilities@1"]);
        let result = adapter
            .extension_callback_response::<Empty>(&callback_id(), &encoded_response(&response()));
        assert!(matches!(
            result,
            Err(ApplicationError::InvalidResponse { .. })
        ));
    }

    #[test]
    fn standard_permission_callback_is_not_framed_as_an_extension() {
        let adapter = adapter(&["phenix.application.capability.permission@1"]);
        let request = PermissionRequest {
            session_id: SessionId::parse("session-1").expect("session id"),
            execution_id: "execution-1".to_owned(),
            call_id: "call-1".to_owned(),
            description: "Write README.md".to_owned(),
        };
        let error = adapter
            .extension_callback_request(&request)
            .expect_err("permission has a standard ACP mapping");

        assert!(matches!(error, ApplicationError::InvalidInput { .. }));
    }
}
