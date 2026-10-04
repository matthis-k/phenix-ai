use crate::transport::SdkApplicationService;
use phenix_application_interface::{
    types::{
        ApplicationError, CallableInvokeInput, CallableResult, ExecutionChange, PermissionRequest,
        PermissionResponse,
    },
    InvokeCallable, Operation,
};
use phenix_core::{ModelToolCall, ModelToolDescriptor, SessionId, ValueCodec};
use std::collections::BTreeMap;

/// Builds the ordinary model-visible tool surface for one session.
///
/// Existing runtime/plugin tools and ephemeral client admissions share one
/// backend-neutral descriptor type. Duplicate effective ids fail before the
/// model sees the surface, and the result is deterministically ordered by id.
pub fn model_tool_surface(
    service: &SdkApplicationService,
    session_id: &SessionId,
    tools: impl IntoIterator<Item = ModelToolDescriptor>,
) -> Result<Vec<ModelToolDescriptor>, ApplicationError> {
    let mut merged = BTreeMap::new();
    for tool in tools {
        let id = tool.id.clone();
        if merged.insert(id.clone(), tool).is_some() {
            return Err(ApplicationError::Conflict {
                message: format!("duplicate model-visible callable {id}"),
            });
        }
    }
    for descriptor in service.client_tool_descriptors(session_id) {
        let tool = ModelToolDescriptor {
            id: descriptor.id.clone(),
            description: descriptor.description,
            input_schema: descriptor.input_schema,
            output_schema: descriptor.output_schema,
        };
        if merged.insert(descriptor.id.clone(), tool).is_some() {
            return Err(ApplicationError::Conflict {
                message: format!("duplicate model-visible callable {}", descriptor.id),
            });
        }
    }
    Ok(merged.into_values().collect())
}

/// Executes one model-emitted call that resolved to a session-admitted client tool.
///
/// This is the application-host side of the ordinary tool lifecycle. Permission
/// runs before the client callback can execute. Input and output validation stay
/// in the generic callable dispatcher reached through `InvokeCallable`.
pub fn execute_admitted_client_tool_call<F>(
    service: &SdkApplicationService,
    session_id: &SessionId,
    execution_id: &str,
    call: ModelToolCall,
    mut request_permission: F,
) -> ExecutionChange
where
    F: FnMut(PermissionRequest) -> Result<PermissionResponse, ApplicationError>,
{
    let call_id = call.call_id.clone();
    let descriptor = service
        .client_tool_descriptors(session_id)
        .into_iter()
        .find(|descriptor| descriptor.id == call.callable_id);
    let Some(descriptor) = descriptor else {
        return tool_failed(
            call_id,
            ApplicationError::NotFound {
                resource: call.callable_id.to_string(),
            },
        );
    };

    if descriptor.policy.requires_permission {
        let permission = request_permission(PermissionRequest {
            session_id: session_id.clone(),
            execution_id: execution_id.to_owned(),
            call_id: call_id.clone(),
            description: descriptor.description,
        });
        match permission {
            Ok(PermissionResponse::AllowOnce) => {}
            Ok(PermissionResponse::Deny) => {
                return tool_failed(
                    call_id,
                    ApplicationError::PermissionDenied {
                        message: format!("permission denied for client tool {}", call.callable_id),
                    },
                );
            }
            Ok(PermissionResponse::Cancelled) => {
                return tool_failed(call_id, ApplicationError::Cancelled);
            }
            Err(error) => return tool_failed(call_id, error),
        }
    }

    let invocation = CallableInvokeInput {
        session_id: session_id.clone(),
        callable_id: call.callable_id,
        input: call.input,
    };
    let result = service
        .invoke(
            &phenix_core::ContractId::parse(InvokeCallable::ID)
                .expect("static application operation id"),
            invocation.to_value(),
        )
        .and_then(|value| {
            CallableResult::from_value(&value).map_err(|error| ApplicationError::InvalidResponse {
                message: error.to_string(),
            })
        });
    match result {
        Ok(result) => ExecutionChange::ToolResult {
            call_id,
            output: result.output,
        },
        Err(error) => tool_failed(call_id, error),
    }
}

fn tool_failed(call_id: String, error: ApplicationError) -> ExecutionChange {
    ExecutionChange::ToolFailed { call_id, error }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{ClientCapabilityCallbacks, ClientPermissionIdentity};
    use phenix_application_interface::{
        types::{CapabilityInvokeResult, ClientToolAddInput, ClientToolDefinition},
        AddClientTool,
    };
    use phenix_core::{
        CallableId, CallableRef, CapabilityGenerationId, CapabilityOwnerId, ClientConnectionId,
        ContractId, ObservableStore, PhenixValue, ReferenceId, ResolvedSdkContributions, PluginRuntimeId,
        SdkContribution, SharedCapabilityRegistry, Type,
    };

    fn admitted_service(
        requires_permission: bool,
    ) -> (
        SdkApplicationService,
        tokio::sync::mpsc::Receiver<crate::transport::ClientCapabilityInvocation>,
        SessionId,
    ) {
        let sdk =
            ResolvedSdkContributions::resolve(&[], &[], Vec::<SdkContribution>::new()).unwrap();
        let capabilities = SharedCapabilityRegistry::default();
        let (callbacks, receiver) = ClientCapabilityCallbacks::bounded(1);
        let owner = ClientConnectionId::parse("fixture-client").unwrap();
        let generation = CapabilityGenerationId::parse("fixture-generation").unwrap();
        let service = SdkApplicationService::new(
            &sdk,
            &ObservableStore::default(),
            capabilities,
            PluginRuntimeId::parse("fixture-runtime").unwrap(),
            CapabilityGenerationId::parse("fixture-runtime-generation").unwrap(),
            callbacks,
            ClientPermissionIdentity::new(owner.clone(), generation.clone()),
        )
        .unwrap();
        let session_id = SessionId::parse("session-a").unwrap();
        let callable = CallableRef::new(
            ContractId::parse("fixture.client-tool-handler@1").unwrap(),
            CapabilityOwnerId::Client(owner),
            generation,
            ReferenceId::parse("handler").unwrap(),
        );
        service
            .invoke(
                &ContractId::parse(AddClientTool::ID).unwrap(),
                ClientToolAddInput {
                    session_id: session_id.clone(),
                    tool: ClientToolDefinition {
                        id: CallableId::parse("fixture.client.echo").unwrap(),
                        description: "Echo from the client".to_owned(),
                        input: Type::U64,
                        output: Type::String,
                        capabilities: Vec::new(),
                        requires_permission,
                        invoke: PhenixValue::Callable(callable),
                    },
                }
                .to_value(),
            )
            .unwrap();
        (service, receiver, session_id)
    }

    fn tool_call() -> ModelToolCall {
        ModelToolCall {
            call_id: "call-1".to_owned(),
            callable_id: CallableId::parse("fixture.client.echo").unwrap(),
            input: PhenixValue::U64(7),
        }
    }

    fn runtime_tool(id: &str) -> ModelToolDescriptor {
        ModelToolDescriptor {
            id: CallableId::parse(id).unwrap(),
            description: format!("{id} runtime tool"),
            input_schema: Type::U64,
            output_schema: Type::String,
        }
    }

    #[test]
    fn model_surface_merges_runtime_and_client_tools_without_origin_metadata() {
        let (service, _callbacks, session_id) = admitted_service(false);
        let tools =
            model_tool_surface(&service, &session_id, [runtime_tool("fixture.runtime")]).unwrap();

        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].id.as_str(), "fixture.client.echo");
        assert_eq!(tools[0].description, "Echo from the client");
        assert_eq!(tools[0].input_schema, Type::U64);
        assert_eq!(tools[0].output_schema, Type::String);
        assert_eq!(tools[1].id.as_str(), "fixture.runtime");
    }

    #[test]
    fn model_surface_rejects_duplicate_effective_ids() {
        let (service, _callbacks, session_id) = admitted_service(false);
        let error =
            model_tool_surface(&service, &session_id, [runtime_tool("fixture.client.echo")])
                .unwrap_err();

        assert!(matches!(error, ApplicationError::Conflict { .. }));
    }

    #[test]
    fn permission_denial_prevents_client_handler_dispatch() {
        let (service, mut callbacks, session_id) = admitted_service(true);
        let change = execute_admitted_client_tool_call(
            &service,
            &session_id,
            "execution-1",
            tool_call(),
            |request| {
                assert_eq!(request.call_id, "call-1");
                assert_eq!(request.description, "Echo from the client");
                Ok(PermissionResponse::Deny)
            },
        );

        assert!(matches!(
            change,
            ExecutionChange::ToolFailed {
                call_id,
                error: ApplicationError::PermissionDenied { .. },
            } if call_id == "call-1"
        ));
        assert!(matches!(
            callbacks.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn allowed_client_tool_call_uses_generic_capability_dispatch() {
        let (service, mut callbacks, session_id) = admitted_service(true);
        let service_for_call = service.clone();
        let session_for_call = session_id.clone();
        let worker = tokio::task::spawn_blocking(move || {
            execute_admitted_client_tool_call(
                &service_for_call,
                &session_for_call,
                "execution-1",
                tool_call(),
                |_| Ok(PermissionResponse::AllowOnce),
            )
        });

        let callback = callbacks.recv().await.unwrap();
        assert_eq!(callback.request().input, PhenixValue::U64(7));
        callback.respond(Ok(CapabilityInvokeResult {
            output: PhenixValue::String("ok".to_owned()),
        }));

        assert_eq!(
            worker.await.unwrap(),
            ExecutionChange::ToolResult {
                call_id: "call-1".to_owned(),
                output: PhenixValue::String("ok".to_owned()),
            }
        );
    }

    #[tokio::test]
    async fn permission_free_client_tool_skips_permission_callback() {
        let (service, mut callbacks, session_id) = admitted_service(false);
        let service_for_call = service.clone();
        let session_for_call = session_id.clone();
        let worker = tokio::task::spawn_blocking(move || {
            execute_admitted_client_tool_call(
                &service_for_call,
                &session_for_call,
                "execution-1",
                tool_call(),
                |_| panic!("permission callback must not run for permission-free tools"),
            )
        });

        callbacks
            .recv()
            .await
            .unwrap()
            .respond(Ok(CapabilityInvokeResult {
                output: PhenixValue::String("ok".to_owned()),
            }));
        assert!(matches!(
            worker.await.unwrap(),
            ExecutionChange::ToolResult { call_id, .. } if call_id == "call-1"
        ));
    }
}
