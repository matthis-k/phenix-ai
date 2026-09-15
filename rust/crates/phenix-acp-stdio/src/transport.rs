#![forbid(unsafe_code)]

//! ACP stdio transport for the fixed Phenix application interface.
//!
//! Protocol translation stays in `phenix-adapter-acp`. This crate owns only
//! process transport and the channel boundary used by the configured runtime.

use agent_client_protocol::{schema::v1::*, Agent, Error, ErrorCode, Stdio};
use phenix_adapter_acp::ApplicationAdapter;
use phenix_application_interface::{
    types::{
        ApplicationError, CallableInfo as ApplicationCallableInfo,
        CallableInvokeInput as ApplicationCallableInvokeInput,
        CallableResult as ApplicationCallableResult, Callables as ApplicationCallables,
        CapabilityInvokeInput as ApplicationCapabilityInvokeInput,
        CapabilityInvokeResult as ApplicationCapabilityInvokeResult,
        ClientToolAddInput as ApplicationClientToolAddInput,
        ClientToolAdmission as ApplicationClientToolAdmission,
        ClientToolDefinition as ApplicationClientToolDefinition,
        ClientToolRemoveInput as ApplicationClientToolRemoveInput, Empty,
        SdkValue as ApplicationSdkValue, SessionInput,
    },
    AddClientTool, ApplicationTransport, GetSdk, InvokeCallable, InvokeCapability, ListCallables,
    Operation, RemoveClientTool,
};
use phenix_core::{
    CallableRef, CapabilityError, CapabilityGenerationId,
    CapabilityInvokeInput as CoreCapabilityInvokeInput, CapabilityOwnerId, ClientConnectionId,
    ContractId, ObservableStore, PhenixValue, ResolvedSdkContributions, RuntimeId,
    SharedCapabilityRegistry, Type, ValueCodec,
};
use phenix_domain::{
    CallableDescriptor, CallableKind, CallablePolicy, CapabilitySet, ClientToolAdmissionId,
    ClientToolAdmissions, ClientToolDefinition, SessionId,
};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

pub struct ApplicationInvocation {
    pub operation: ContractId,
    pub input: PhenixValue,
    response: oneshot::Sender<Result<PhenixValue, ApplicationError>>,
}

pub struct ApplicationEvent {
    pub event: ContractId,
    pub payload: PhenixValue,
}

/// One generic runtime-to-client capability invocation waiting for its ACP response.
pub struct ClientCapabilityInvocation {
    request: ApplicationCapabilityInvokeInput,
    response: oneshot::Sender<Result<ApplicationCapabilityInvokeResult, CapabilityError>>,
}

impl ClientCapabilityInvocation {
    #[must_use]
    pub fn request(&self) -> &ApplicationCapabilityInvokeInput {
        &self.request
    }

    pub fn respond(self, response: Result<ApplicationCapabilityInvokeResult, CapabilityError>) {
        let _ = self.response.send(response);
    }
}

/// Bounded bridge from synchronous runtime capability handlers to one ACP client.
#[derive(Clone)]
pub struct ClientCapabilityCallbacks {
    sender: mpsc::Sender<ClientCapabilityInvocation>,
}

/// The authenticated identity represented by one ACP callback queue.
#[derive(Clone)]
pub struct ClientCapabilityIdentity {
    owner: ClientConnectionId,
    generation: CapabilityGenerationId,
}

impl ClientCapabilityIdentity {
    #[must_use]
    pub fn new(owner: ClientConnectionId, generation: CapabilityGenerationId) -> Self {
        Self { owner, generation }
    }
}

impl ClientCapabilityCallbacks {
    #[must_use]
    pub fn bounded(capacity: usize) -> (Self, mpsc::Receiver<ClientCapabilityInvocation>) {
        let (sender, receiver) = mpsc::channel(capacity);
        (Self { sender }, receiver)
    }

    fn invoke(
        &self,
        callable: CallableRef,
        input: PhenixValue,
    ) -> Result<PhenixValue, CapabilityError> {
        let (response, receiver) = oneshot::channel();
        self.sender
            .try_send(ClientCapabilityInvocation {
                request: ApplicationCapabilityInvokeInput {
                    callable: PhenixValue::Callable(callable),
                    input,
                },
                response,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => CapabilityError::QueueFull,
                mpsc::error::TrySendError::Closed(_) => CapabilityError::Disconnected,
            })?;
        receiver
            .blocking_recv()
            .map_err(|_| CapabilityError::Disconnected)?
            .map(|response| response.output)
    }
}

impl ApplicationInvocation {
    pub fn respond(self, response: Result<PhenixValue, ApplicationError>) {
        let _ = self.response.send(response);
    }
}

#[derive(Clone)]
pub struct ChannelTransport {
    sender: mpsc::Sender<ApplicationInvocation>,
}

impl ChannelTransport {
    #[must_use]
    pub fn new(capacity: usize) -> (Self, mpsc::Receiver<ApplicationInvocation>) {
        let (sender, receiver) = mpsc::channel(capacity);
        (Self { sender }, receiver)
    }
}

impl ApplicationTransport for ChannelTransport {
    fn invoke(
        &self,
        operation: &ContractId,
        input: PhenixValue,
    ) -> impl std::future::Future<Output = Result<PhenixValue, ApplicationError>> {
        let sender = self.sender.clone();
        let operation = operation.clone();
        async move {
            let (response, receive) = oneshot::channel();
            sender
                .send(ApplicationInvocation {
                    operation,
                    input,
                    response,
                })
                .await
                .map_err(|_| ApplicationError::Disconnected)?;
            receive.await.map_err(|_| ApplicationError::Disconnected)?
        }
    }
}

/// Live application handler for the value SDK operations owned by #503.
///
/// SDK materialization and generic invocation share one capability registry.
/// The service materializes the SDK once so repeated `GetSdk` calls return the
/// same callable identities instead of re-registering them.
#[derive(Clone)]
pub struct SdkApplicationService {
    sdk: ApplicationSdkValue,
    capabilities: SharedCapabilityRegistry,
    client_callbacks: ClientCapabilityCallbacks,
    client_owner: ClientConnectionId,
    client_generation: CapabilityGenerationId,
    admissions: Arc<Mutex<ClientToolAdmissions>>,
}

impl SdkApplicationService {
    pub fn new(
        sdk: &ResolvedSdkContributions,
        store: &ObservableStore,
        capabilities: SharedCapabilityRegistry,
        runtime: RuntimeId,
        generation: CapabilityGenerationId,
        client_callbacks: ClientCapabilityCallbacks,
        client: ClientCapabilityIdentity,
    ) -> Result<Self, phenix_core::SdkResolutionError> {
        let sdk = sdk.value_with_observables(store, &capabilities, &runtime, generation)?;
        Ok(Self {
            sdk: ApplicationSdkValue {
                schema: sdk.schema,
                value: sdk.value,
            },
            capabilities,
            client_callbacks,
            client_owner: client.owner,
            client_generation: client.generation,
            admissions: Arc::new(Mutex::new(ClientToolAdmissions::default())),
        })
    }

    #[must_use]
    pub fn capabilities(&self) -> &SharedCapabilityRegistry {
        &self.capabilities
    }

    #[must_use]
    pub fn client_tool_descriptors(
        &self,
        session_id: &phenix_core::SessionId,
    ) -> Vec<CallableDescriptor> {
        let Ok(session_id) = SessionId::parse(session_id.as_str()) else {
            return Vec::new();
        };
        self.admissions
            .lock()
            .map(|admissions| admissions.descriptors(&session_id))
            .unwrap_or_default()
    }

    /// Retire this ACP connection's ephemeral callables and session tool admissions.
    pub fn retire_client(&self) {
        let owner = CapabilityOwnerId::Client(self.client_owner.clone());
        self.capabilities
            .retire(owner.clone(), self.client_generation.clone());
        if let Ok(mut admissions) = self.admissions.lock() {
            admissions.retire(&owner, &self.client_generation);
        }
    }

    pub fn invoke(
        &self,
        operation: &ContractId,
        input: PhenixValue,
    ) -> Result<PhenixValue, ApplicationError> {
        if operation.as_str() == GetSdk::ID {
            Empty::from_value(&input).map_err(|error| ApplicationError::InvalidInput {
                message: error.to_string(),
            })?;
            return Ok(self.sdk.to_value());
        }
        if operation.as_str() == AddClientTool::ID {
            let request = ApplicationClientToolAddInput::from_value(&input).map_err(|error| {
                ApplicationError::InvalidInput {
                    message: error.to_string(),
                }
            })?;
            return self
                .admit_client_tool(request)
                .map(|admission| admission.to_value());
        }
        if operation.as_str() == RemoveClientTool::ID {
            let request =
                ApplicationClientToolRemoveInput::from_value(&input).map_err(|error| {
                    ApplicationError::InvalidInput {
                        message: error.to_string(),
                    }
                })?;
            self.remove_client_tool(request)?;
            return Ok(phenix_application_interface::types::Acknowledged {}.to_value());
        }
        if operation.as_str() == ListCallables::ID {
            let request = SessionInput::from_value(&input).map_err(|error| {
                ApplicationError::InvalidInput {
                    message: error.to_string(),
                }
            })?;
            return Ok(self.list_client_tools(&request.session_id).to_value());
        }
        if operation.as_str() == InvokeCallable::ID {
            let request = ApplicationCallableInvokeInput::from_value(&input).map_err(|error| {
                ApplicationError::InvalidInput {
                    message: error.to_string(),
                }
            })?;
            return self
                .invoke_client_tool(request)
                .map(|result| result.to_value());
        }
        if operation.as_str() == InvokeCapability::ID {
            let request =
                ApplicationCapabilityInvokeInput::from_value(&input).map_err(|error| {
                    ApplicationError::InvalidInput {
                        message: error.to_string(),
                    }
                })?;
            let PhenixValue::Callable(callable) = request.callable else {
                return Err(ApplicationError::InvalidInput {
                    message: "capability invocation requires a callable reference".to_owned(),
                });
            };
            let schema = self.capabilities.schema(&callable)?;
            CoreCapabilityInvokeInput {
                callable: callable.clone(),
                input: request.input.clone(),
            }
            .validate(&schema)
            .map_err(|error| ApplicationError::SchemaMismatch {
                message: error.to_string(),
            })?;
            let Type::Callable {
                input: input_schema,
                ..
            } = &schema
            else {
                return Err(ApplicationError::SchemaMismatch {
                    message: format!(
                        "capability {} is registered with a non-callable schema",
                        callable.id()
                    ),
                });
            };
            self.admit_client_callables(input_schema, &request.input)?;
            let result = self.capabilities.invoke(CoreCapabilityInvokeInput {
                callable,
                input: request.input,
            })?;
            return Ok(ApplicationCapabilityInvokeResult {
                output: result.output,
            }
            .to_value());
        }
        Err(ApplicationError::InvalidInput {
            message: format!("unsupported SDK application operation {operation}"),
        })
    }

    pub fn handle(&self, invocation: ApplicationInvocation) {
        let response = self.invoke(&invocation.operation, invocation.input.clone());
        invocation.respond(response);
    }

    fn admit_client_tool(
        &self,
        request: ApplicationClientToolAddInput,
    ) -> Result<ApplicationClientToolAdmission, ApplicationError> {
        let ApplicationClientToolAddInput { session_id, tool } = request;
        let session_id = SessionId::parse(session_id.as_str()).map_err(|error| {
            ApplicationError::InvalidInput {
                message: error.to_string(),
            }
        })?;
        let ApplicationClientToolDefinition {
            id,
            description,
            input,
            output,
            capabilities,
            requires_permission,
            invoke,
        } = tool;
        let PhenixValue::Callable(invoke) = invoke else {
            return Err(ApplicationError::SchemaMismatch {
                message: "client tool invoke must be a callable capability".to_owned(),
            });
        };
        if !self.is_current_client_callable(&invoke) {
            return Err(ApplicationError::SchemaMismatch {
                message: "client tool invoke must belong to the connected client generation"
                    .to_owned(),
            });
        }
        let callable_schema = Type::Callable {
            contract: invoke.contract().clone(),
            input: Box::new(input.clone()),
            output: Box::new(output.clone()),
        };
        if description.trim().is_empty() {
            return Err(ApplicationError::InvalidInput {
                message: "client tool description must not be empty".to_owned(),
            });
        }
        let tool = ClientToolDefinition::new(
            CallableDescriptor {
                id,
                kind: CallableKind::Tool,
                description,
                input_schema: input,
                output_schema: output,
                capabilities: CapabilitySet(capabilities.into_iter().collect()),
                policy: CallablePolicy {
                    requires_permission,
                },
            },
            invoke,
        )
        .map_err(|error| ApplicationError::InvalidInput {
            message: error.to_string(),
        })?;
        let mut admissions = self
            .admissions
            .lock()
            .map_err(|_| ApplicationError::Failed {
                message: "client tool admission registry lock is poisoned".to_owned(),
            })?;
        if admissions
            .admitted(&session_id, &tool.descriptor.id)
            .is_some()
        {
            return Err(ApplicationError::Conflict {
                message: format!(
                    "session {session_id} already admits callable {}",
                    tool.descriptor.id
                ),
            });
        }
        // Keep admission and capability registration atomic with explicit removal.
        // Registry registration never calls provider code.
        self.admit_current_client_callable(&tool.invoke, callable_schema)?;
        let admission =
            admissions
                .admit(session_id, tool)
                .map_err(|error| ApplicationError::Conflict {
                    message: error.to_string(),
                })?;
        Ok(ApplicationClientToolAdmission {
            admission_id: admission.id.to_string(),
            callable_id: admission.tool.descriptor.id,
        })
    }

    fn remove_client_tool(
        &self,
        request: ApplicationClientToolRemoveInput,
    ) -> Result<(), ApplicationError> {
        let session_id = SessionId::parse(request.session_id.as_str()).map_err(|error| {
            ApplicationError::InvalidInput {
                message: error.to_string(),
            }
        })?;
        let admission_id = ClientToolAdmissionId::parse(request.admission_id).map_err(|error| {
            ApplicationError::InvalidInput {
                message: error.to_string(),
            }
        })?;
        let mut admissions = self
            .admissions
            .lock()
            .map_err(|_| ApplicationError::Failed {
                message: "client tool admission registry lock is poisoned".to_owned(),
            })?;
        let admission = admissions
            .remove_from_session(&session_id, &admission_id)
            .map_err(|error| ApplicationError::StaleReference {
                value: error.to_string(),
            })?;
        if !admissions.contains_invoke(&admission.tool.invoke) {
            self.capabilities.unregister(&admission.tool.invoke);
        }
        Ok(())
    }

    fn list_client_tools(&self, session_id: &phenix_core::SessionId) -> ApplicationCallables {
        ApplicationCallables {
            callables: self
                .client_tool_descriptors(session_id)
                .into_iter()
                .map(|descriptor| ApplicationCallableInfo {
                    id: descriptor.id,
                    description: descriptor.description,
                    input: descriptor.input_schema,
                    output: descriptor.output_schema,
                })
                .collect(),
        }
    }

    fn invoke_client_tool(
        &self,
        request: ApplicationCallableInvokeInput,
    ) -> Result<ApplicationCallableResult, ApplicationError> {
        let session_id = SessionId::parse(request.session_id.as_str()).map_err(|error| {
            ApplicationError::InvalidInput {
                message: error.to_string(),
            }
        })?;
        let admission = self
            .admissions
            .lock()
            .map_err(|_| ApplicationError::Failed {
                message: "client tool admission registry lock is poisoned".to_owned(),
            })?
            .admitted(&session_id, &request.callable_id)
            .cloned()
            .ok_or_else(|| ApplicationError::NotFound {
                resource: request.callable_id.to_string(),
            })?;
        let schema = Type::Callable {
            contract: admission.tool.invoke.contract().clone(),
            input: Box::new(admission.tool.descriptor.input_schema),
            output: Box::new(admission.tool.descriptor.output_schema),
        };
        let invocation = CoreCapabilityInvokeInput {
            callable: admission.tool.invoke,
            input: request.input,
        };
        invocation
            .validate(&schema)
            .map_err(|error| ApplicationError::SchemaMismatch {
                message: error.to_string(),
            })?;
        let output = self.capabilities.invoke(invocation)?.output;
        Ok(ApplicationCallableResult { output })
    }

    fn admit_client_callables(
        &self,
        schema: &Type,
        value: &PhenixValue,
    ) -> Result<(), ApplicationError> {
        match (schema, value) {
            (Type::Callable { .. }, PhenixValue::Callable(callable)) => {
                if matches!(callable.owner(), CapabilityOwnerId::Client(_)) {
                    self.admit_current_client_callable(callable, schema.clone())?;
                }
            }
            (Type::Option(schema), PhenixValue::Option(Some(value))) => {
                self.admit_client_callables(schema, value)?;
            }
            (Type::Array { item, .. } | Type::List(item), PhenixValue::List(values)) => {
                for value in values {
                    self.admit_client_callables(item, value)?;
                }
            }
            (Type::Map(item), PhenixValue::Map(values)) => {
                for value in values.values() {
                    self.admit_client_callables(item, value)?;
                }
            }
            (Type::Table(fields), PhenixValue::Table(values)) => {
                for (key, schema) in fields {
                    let value =
                        values
                            .get(key)
                            .ok_or_else(|| ApplicationError::SchemaMismatch {
                                message: format!("capability input is missing field {key}"),
                            })?;
                    self.admit_client_callables(schema, value)?;
                }
            }
            (Type::Variant(variants), PhenixValue::Variant { tag, value }) => {
                let schema = variants
                    .get(tag)
                    .ok_or_else(|| ApplicationError::SchemaMismatch {
                        message: format!("capability input has unknown variant {tag}"),
                    })?;
                self.admit_client_callables(schema, value)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Admit one callable owned by this ACP connection's current client generation.
    ///
    /// Application semantics stay outside the transport. Callers provide the exact
    /// callable schema required by the application contract.
    pub fn admit_current_client_callable(
        &self,
        callable: &CallableRef,
        schema: Type,
    ) -> Result<(), ApplicationError> {
        if !self.is_current_client_callable(callable) {
            return Err(ApplicationError::SchemaMismatch {
                message: "client callable must belong to the connected client generation"
                    .to_owned(),
            });
        }
        let callbacks = self.client_callbacks.clone();
        let reference = callable.clone();
        let registry = self.capabilities.clone();
        let admissions = Arc::clone(&self.admissions);
        let owner = callable.owner().clone();
        let generation = callable.generation().clone();
        match self
            .capabilities
            .register(reference.clone(), schema.clone(), move |input| {
                let result = callbacks.invoke(reference.clone(), input);
                if matches!(result, Err(CapabilityError::Disconnected)) {
                    registry.retire(owner.clone(), generation.clone());
                    if let Ok(mut admissions) = admissions.lock() {
                        admissions.retire(&owner, &generation);
                    }
                }
                result
            }) {
            Ok(()) => Ok(()),
            Err(CapabilityError::DuplicateReference(_)) => {
                let registered = self.capabilities.schema(callable)?;
                if registered == schema {
                    Ok(())
                } else {
                    Err(ApplicationError::SchemaMismatch {
                        message: format!(
                            "client callable {} was already admitted with another schema",
                            callable.id()
                        ),
                    })
                }
            }
            Err(error) => Err(error.into()),
        }
    }

    fn is_current_client_callable(&self, callable: &CallableRef) -> bool {
        callable.owner() == &CapabilityOwnerId::Client(self.client_owner.clone())
            && callable.generation() == &self.client_generation
    }
}

pub async fn serve_sdk_application(
    service: SdkApplicationService,
    mut receiver: mpsc::Receiver<ApplicationInvocation>,
) {
    while let Some(invocation) = receiver.recv().await {
        let service = service.clone();
        tokio::task::spawn_blocking(move || service.handle(invocation));
    }
    service.retire_client();
}

pub async fn serve_stdio(
    transport: ChannelTransport,
    advertised: impl IntoIterator<Item = ContractId>,
) -> Result<(), Error> {
    let (keep_events_open, events) = mpsc::channel(1);
    let result = serve_stdio_with_events(transport, advertised, events).await;
    drop(keep_events_open);
    result
}

pub async fn serve_stdio_with_events(
    transport: ChannelTransport,
    advertised: impl IntoIterator<Item = ContractId>,
    events: mpsc::Receiver<ApplicationEvent>,
) -> Result<(), Error> {
    let (keep_callbacks_open, callbacks) = ClientCapabilityCallbacks::bounded(1);
    let result =
        serve_stdio_with_events_and_callbacks(transport, advertised, events, callbacks).await;
    drop(keep_callbacks_open);
    result
}

pub async fn serve_stdio_with_events_and_callbacks(
    transport: ChannelTransport,
    advertised: impl IntoIterator<Item = ContractId>,
    mut events: mpsc::Receiver<ApplicationEvent>,
    mut callbacks: mpsc::Receiver<ClientCapabilityInvocation>,
) -> Result<(), Error> {
    let adapter =
        Arc::new(ApplicationAdapter::new(transport, advertised).map_err(application_error_to_acp)?);

    let initialize = Arc::clone(&adapter);
    let new_session = Arc::clone(&adapter);
    let list_sessions = Arc::clone(&adapter);
    let resume_session = Arc::clone(&adapter);
    let load_session = Arc::clone(&adapter);
    let close_session = Arc::clone(&adapter);
    let prompt = Arc::clone(&adapter);
    let cancel = Arc::clone(&adapter);
    let set_config = Arc::clone(&adapter);
    let event_adapter = Arc::clone(&adapter);
    let extension_adapter = Arc::clone(&adapter);
    let callback_adapter = Arc::clone(&adapter);

    Agent
        .builder()
        .name("phenix-acp")
        .on_receive_request(
            async move |request: InitializeRequest, responder, _cx| {
                responder.respond(initialize.initialize(request))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: NewSessionRequest, responder, _cx| {
                let response = new_session
                    .new_session(request)
                    .await
                    .map_err(application_error_to_acp)?;
                responder.respond(response)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ListSessionsRequest, responder, _cx| {
                let response = list_sessions
                    .list_sessions(request)
                    .await
                    .map_err(application_error_to_acp)?;
                responder.respond(response)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ResumeSessionRequest, responder, _cx| {
                let response = resume_session
                    .resume_session(request)
                    .await
                    .map_err(application_error_to_acp)?;
                responder.respond(response)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: LoadSessionRequest, responder, _cx| {
                let loaded = load_session
                    .load_session(request)
                    .await
                    .map_err(application_error_to_acp)?;
                responder.respond(loaded.response)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: CloseSessionRequest, responder, _cx| {
                let response = close_session
                    .close_session(request)
                    .await
                    .map_err(application_error_to_acp)?;
                responder.respond(response)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: PromptRequest, responder, _cx| {
                let response = prompt
                    .prompt(request)
                    .await
                    .map_err(application_error_to_acp)?;
                responder.respond(response)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: SetSessionConfigOptionRequest, responder, _cx| {
                let response = set_config
                    .set_session_config_option(request)
                    .await
                    .map_err(application_error_to_acp)?;
                responder.respond(response)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ClientRequest, responder, connection| {
                let ClientRequest::ExtMethodRequest(request) = request else {
                    return Err(Error::method_not_found());
                };
                let extension_adapter = Arc::clone(&extension_adapter);
                connection.spawn(async move {
                    let response = extension_adapter
                        .extension_request(request)
                        .await
                        .map_err(application_error_to_acp)
                        .and_then(|response| {
                            serde_json::to_value(response)
                                .map_err(|error| Error::internal_error().data(error.to_string()))
                        });
                    match response {
                        Ok(response) => responder.respond(response),
                        Err(error) => responder.respond_with_error(error),
                    }
                })?;
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            async move |notification: CancelNotification, _cx| {
                cancel
                    .cancel(notification)
                    .await
                    .map_err(application_error_to_acp)
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_with(
            Stdio::new(),
            move |connection: agent_client_protocol::ConnectionTo<agent_client_protocol::Client>| async move {
            loop {
                tokio::select! {
                    event = events.recv() => match event {
                        Some(event) => {
                            let notification = event_adapter
                                .extension_event(&event.event, &event.payload)
                                .map_err(application_error_to_acp)?;
                            connection.send_notification(AgentNotification::ExtNotification(notification))?;
                        }
                        None => return Ok(()),
                    },
                    callback = callbacks.recv() => match callback {
                        Some(callback) => {
                            let callable = match &callback.request().callable {
                                PhenixValue::Callable(callable) => callable.clone(),
                                _ => {
                                    callback.respond(Err(CapabilityError::SchemaMismatch {
                                        message: "client capability callback requires a callable reference".to_owned(),
                                    }));
                                    continue;
                                }
                            };
                            let (callback_id, request) = match callback_adapter
                                .extension_callback_request(callback.request())
                            {
                                Ok(request) => request,
                                Err(error) => {
                                    callback.respond(Err(application_error_to_capability(error, callable)));
                                    continue;
                                }
                            };
                            let response = match connection
                                .send_request(AgentRequest::ExtMethodRequest(request))
                                .block_task()
                                .await
                            {
                                Ok(response) => response,
                                Err(error) => {
                                    callback.respond(Err(acp_error_to_capability(error, callable)));
                                    continue;
                                }
                            };
                            let response = match serde_json::from_value(response) {
                                Ok(response) => response,
                                Err(error) => {
                                    callback.respond(Err(CapabilityError::SchemaMismatch {
                                        message: format!(
                                            "cannot decode ACP extension callback response: {error}"
                                        ),
                                    }));
                                    continue;
                                }
                            };
                            callback.respond(
                                callback_adapter
                                    .extension_callback_response(&callback_id, &response)
                                    .map_err(|error| application_error_to_capability(error, callable)),
                            );
                        }
                        None => return Ok(()),
                    },
                    () = connection.incoming_closed() => return Ok(()),
                }
            }
        },
        )
        .await
}

fn application_error_to_acp(error: ApplicationError) -> Error {
    let data = json!({
        "phenix.class": error.class(),
        "phenix.details": application_error_details(&error),
    });
    let error = match &error {
        ApplicationError::UnsupportedCapability { .. } => Error::method_not_found(),
        ApplicationError::InvalidInput { .. } => Error::invalid_params(),
        ApplicationError::InvalidResponse { .. }
        | ApplicationError::PermissionDenied { .. }
        | ApplicationError::Conflict { .. }
        | ApplicationError::Failed { .. }
        | ApplicationError::Disconnected => Error::internal_error(),
        ApplicationError::NotFound { resource } => {
            Error::resource_not_found(Some(resource.clone()))
        }
        ApplicationError::UnknownValue { value } | ApplicationError::StaleReference { value } => {
            Error::resource_not_found(Some(value.clone()))
        }
        ApplicationError::InvalidPath { .. } | ApplicationError::SchemaMismatch { .. } => {
            Error::invalid_params()
        }
        ApplicationError::UnsupportedSnapshotPolicy { .. }
        | ApplicationError::TransactionConflict { .. }
        | ApplicationError::SubscriptionCapacity
        | ApplicationError::Closed => Error::internal_error(),
        ApplicationError::Unauthenticated { .. } => Error::auth_required(),
        ApplicationError::Cancelled => Error::request_cancelled(),
    };
    error.data(data)
}

fn application_error_details(error: &ApplicationError) -> serde_json::Value {
    match error {
        ApplicationError::UnsupportedCapability { capability } => {
            json!({ "capability": capability.as_str() })
        }
        ApplicationError::InvalidInput { message }
        | ApplicationError::InvalidResponse { message }
        | ApplicationError::Unauthenticated { message }
        | ApplicationError::PermissionDenied { message }
        | ApplicationError::Conflict { message }
        | ApplicationError::Failed { message }
        | ApplicationError::InvalidPath { message }
        | ApplicationError::SchemaMismatch { message }
        | ApplicationError::UnsupportedSnapshotPolicy { message }
        | ApplicationError::TransactionConflict { message } => json!({ "message": message }),
        ApplicationError::NotFound { resource } => json!({ "resource": resource }),
        ApplicationError::UnknownValue { value } | ApplicationError::StaleReference { value } => {
            json!({ "value": value })
        }
        ApplicationError::Cancelled
        | ApplicationError::Disconnected
        | ApplicationError::SubscriptionCapacity
        | ApplicationError::Closed => serde_json::Value::Null,
    }
}

fn application_error_to_capability(
    error: ApplicationError,
    callable: CallableRef,
) -> CapabilityError {
    match error {
        ApplicationError::Cancelled => CapabilityError::Cancelled,
        ApplicationError::Disconnected => CapabilityError::Disconnected,
        ApplicationError::StaleReference { .. } => CapabilityError::StaleReference(callable),
        ApplicationError::NotFound { .. } => CapabilityError::UnknownReference(callable),
        ApplicationError::SchemaMismatch { message }
        | ApplicationError::InvalidInput { message }
        | ApplicationError::InvalidResponse { message } => {
            CapabilityError::SchemaMismatch { message }
        }
        other => CapabilityError::ProviderFailed {
            message: other.to_string(),
        },
    }
}

fn acp_error_to_capability(error: Error, callable: CallableRef) -> CapabilityError {
    let class = error
        .data
        .as_ref()
        .and_then(|data| data.get("phenix.class"))
        .and_then(serde_json::Value::as_str);
    if error.code == ErrorCode::RequestCancelled || class == Some("cancelled") {
        return CapabilityError::Cancelled;
    }
    match class {
        Some("disconnected") => CapabilityError::Disconnected,
        Some("queue_full") => CapabilityError::QueueFull,
        Some("stale_reference") => CapabilityError::StaleReference(callable),
        Some("not_found") => CapabilityError::UnknownReference(callable),
        Some("schema_mismatch") | Some("invalid_input") | Some("invalid_response") => {
            CapabilityError::SchemaMismatch {
                message: error.to_string(),
            }
        }
        _ => CapabilityError::ProviderFailed {
            message: error.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{
        Authority, CapabilityOwnerId, ClientConnectionId, Key, ObservableRegistration, PhenixValue,
        PluginExecution, PluginId, PluginManifest, ReferenceId, SdkContribution, SdkNamespace,
        SdkObservableResource, SdkResourceId, SnapshotPolicy, Type, ValueId, ValuePath,
    };

    fn client_callable() -> CallableRef {
        CallableRef::new(
            ContractId::parse("fixture.client-callback@1").unwrap(),
            CapabilityOwnerId::Client(ClientConnectionId::parse("fixture-client").unwrap()),
            CapabilityGenerationId::parse("fixture-generation").unwrap(),
            ReferenceId::parse("callback").unwrap(),
        )
    }

    #[tokio::test]
    async fn channel_transport_preserves_typed_operation_and_response() {
        let (transport, mut receiver) = ChannelTransport::new(1);
        let operation = ContractId::parse("phenix.application.session-create@1").unwrap();
        let expected_operation = operation.clone();
        let worker = tokio::spawn(async move {
            let invocation = receiver.recv().await.unwrap();
            assert_eq!(invocation.operation, expected_operation);
            assert_eq!(invocation.input, PhenixValue::String("input".to_owned()));
            invocation.respond(Ok(PhenixValue::String("output".to_owned())));
        });

        let output = transport
            .invoke(&operation, PhenixValue::String("input".to_owned()))
            .await
            .unwrap();
        assert_eq!(output, PhenixValue::String("output".to_owned()));
        worker.await.unwrap();
    }

    #[tokio::test]
    async fn client_capability_bridge_forwards_the_canonical_invocation() {
        let (callbacks, mut receiver) = ClientCapabilityCallbacks::bounded(1);
        let callable = client_callable();
        let expected = callable.clone();
        let worker =
            tokio::task::spawn_blocking(move || callbacks.invoke(callable, PhenixValue::U64(7)));

        let invocation = receiver.recv().await.unwrap();
        assert_eq!(
            invocation.request().callable,
            PhenixValue::Callable(expected)
        );
        assert_eq!(invocation.request().input, PhenixValue::U64(7));
        invocation.respond(Ok(ApplicationCapabilityInvokeResult {
            output: PhenixValue::String("ok".to_owned()),
        }));
        assert_eq!(
            worker.await.unwrap().unwrap(),
            PhenixValue::String("ok".to_owned())
        );
    }

    #[tokio::test]
    async fn client_capability_bridge_preserves_structural_failures() {
        let cases = [
            CapabilityError::Cancelled,
            CapabilityError::Disconnected,
            CapabilityError::QueueFull,
        ];
        for expected in cases {
            let (callbacks, mut receiver) = ClientCapabilityCallbacks::bounded(1);
            let callable = client_callable();
            let worker = tokio::task::spawn_blocking(move || {
                callbacks.invoke(callable, PhenixValue::U64(7))
            });
            receiver
                .recv()
                .await
                .unwrap()
                .respond(Err(expected.clone()));
            assert_eq!(worker.await.unwrap().unwrap_err(), expected);
        }
    }

    #[tokio::test]
    async fn generic_callable_input_admits_a_client_owned_reference() {
        let capabilities = SharedCapabilityRegistry::default();
        let (callbacks, mut receiver) = ClientCapabilityCallbacks::bounded(1);
        let service = SdkApplicationService {
            sdk: ApplicationSdkValue {
                schema: Type::Table(Default::default()),
                value: PhenixValue::Table(Default::default()),
            },
            capabilities: capabilities.clone(),
            client_callbacks: callbacks,
            client_owner: ClientConnectionId::parse("fixture-client").unwrap(),
            client_generation: CapabilityGenerationId::parse("fixture-generation").unwrap(),
            admissions: Arc::new(Mutex::new(ClientToolAdmissions::default())),
        };
        let callable = client_callable();
        let schema = Type::Callable {
            contract: callable.contract().clone(),
            input: Box::new(Type::U64),
            output: Box::new(Type::String),
        };
        service
            .admit_client_callables(&schema, &PhenixValue::Callable(callable.clone()))
            .unwrap();

        let worker = tokio::task::spawn_blocking(move || {
            capabilities.invoke(CoreCapabilityInvokeInput {
                callable,
                input: PhenixValue::U64(7),
            })
        });
        let invocation = receiver.recv().await.unwrap();
        invocation.respond(Ok(ApplicationCapabilityInvokeResult {
            output: PhenixValue::String("ok".to_owned()),
        }));
        assert_eq!(
            worker.await.unwrap().unwrap().output,
            PhenixValue::String("ok".to_owned())
        );
    }

    #[tokio::test]
    async fn client_tool_operations_admit_and_remove_a_client_callable() {
        let capabilities = SharedCapabilityRegistry::default();
        let (callbacks, _receiver) = ClientCapabilityCallbacks::bounded(1);
        let service = SdkApplicationService {
            sdk: ApplicationSdkValue {
                schema: Type::Table(Default::default()),
                value: PhenixValue::Table(Default::default()),
            },
            capabilities,
            client_callbacks: callbacks,
            client_owner: ClientConnectionId::parse("fixture-client").unwrap(),
            client_generation: CapabilityGenerationId::parse("fixture-generation").unwrap(),
            admissions: Arc::new(Mutex::new(ClientToolAdmissions::default())),
        };
        let session = phenix_core::SessionId::parse("session-a").unwrap();
        let callable = client_callable();
        let input = ApplicationClientToolAddInput {
            session_id: session.clone(),
            tool: ApplicationClientToolDefinition {
                id: phenix_core::CallableId::parse("fixture.client.echo").unwrap(),
                description: "Echo a client value".to_owned(),
                input: Type::U64,
                output: Type::String,
                capabilities: Vec::new(),
                requires_permission: false,
                invoke: PhenixValue::Callable(callable.clone()),
            },
        };
        let value = service
            .invoke(
                &ContractId::parse(AddClientTool::ID).unwrap(),
                input.to_value(),
            )
            .unwrap();
        let admission = ApplicationClientToolAdmission::from_value(&value).unwrap();
        assert_eq!(admission.callable_id.as_str(), "fixture.client.echo");
        assert_eq!(service.client_tool_descriptors(&session).len(), 1);
        let rejected = CallableRef::new(
            callable.contract().clone(),
            callable.owner().clone(),
            callable.generation().clone(),
            ReferenceId::parse("rejected-handler").unwrap(),
        );
        let mut duplicate = input.clone();
        duplicate.tool.invoke = PhenixValue::Callable(rejected.clone());
        assert!(matches!(
            service.admit_client_tool(duplicate.clone()),
            Err(ApplicationError::Conflict { .. })
        ));
        assert!(matches!(
            service.capabilities.schema(&rejected),
            Err(CapabilityError::UnknownReference(_))
        ));
        duplicate.tool.description.clear();
        assert!(matches!(
            service.admit_client_tool(duplicate),
            Err(ApplicationError::InvalidInput { .. })
        ));
        assert!(matches!(
            service.capabilities.schema(&rejected),
            Err(CapabilityError::UnknownReference(_))
        ));
        let listed = service
            .invoke(
                &ContractId::parse(ListCallables::ID).unwrap(),
                SessionInput {
                    session_id: session.clone(),
                }
                .to_value(),
            )
            .unwrap();
        assert_eq!(
            ApplicationCallables::from_value(&listed)
                .unwrap()
                .callables
                .len(),
            1
        );

        service
            .invoke(
                &ContractId::parse(RemoveClientTool::ID).unwrap(),
                ApplicationClientToolRemoveInput {
                    session_id: session.clone(),
                    admission_id: admission.admission_id,
                }
                .to_value(),
            )
            .unwrap();
        assert!(service.client_tool_descriptors(&session).is_empty());
        assert!(matches!(
            service.capabilities.invoke(CoreCapabilityInvokeInput {
                callable: callable.clone(),
                input: PhenixValue::U64(7),
            }),
            Err(CapabilityError::UnknownReference(reference)) if reference == callable
        ));
    }

    #[tokio::test]
    async fn client_tool_invocation_uses_the_generic_capability_dispatcher() {
        let capabilities = SharedCapabilityRegistry::default();
        let (callbacks, mut receiver) = ClientCapabilityCallbacks::bounded(1);
        let service = SdkApplicationService {
            sdk: ApplicationSdkValue {
                schema: Type::Table(Default::default()),
                value: PhenixValue::Table(Default::default()),
            },
            capabilities,
            client_callbacks: callbacks,
            client_owner: ClientConnectionId::parse("fixture-client").unwrap(),
            client_generation: CapabilityGenerationId::parse("fixture-generation").unwrap(),
            admissions: Arc::new(Mutex::new(ClientToolAdmissions::default())),
        };
        let session = phenix_core::SessionId::parse("session-a").unwrap();
        let callable_id = phenix_core::CallableId::parse("fixture.client.echo").unwrap();
        let definition = ApplicationClientToolAddInput {
            session_id: session.clone(),
            tool: ApplicationClientToolDefinition {
                id: callable_id.clone(),
                description: "Echo a client value".to_owned(),
                input: Type::U64,
                output: Type::String,
                capabilities: Vec::new(),
                requires_permission: false,
                invoke: PhenixValue::Callable(client_callable()),
            },
        };
        let admission = service
            .invoke(
                &ContractId::parse(AddClientTool::ID).unwrap(),
                definition.to_value(),
            )
            .unwrap();
        let admission = ApplicationClientToolAdmission::from_value(&admission).unwrap();

        let service_for_call = service.clone();
        let session_for_call = session.clone();
        let callable_for_call = callable_id.clone();
        let worker = tokio::task::spawn_blocking(move || {
            service_for_call.invoke(
                &ContractId::parse(InvokeCallable::ID).unwrap(),
                ApplicationCallableInvokeInput {
                    session_id: session_for_call,
                    callable_id: callable_for_call,
                    input: PhenixValue::U64(7),
                }
                .to_value(),
            )
        });
        let callback = receiver.recv().await.unwrap();
        assert_eq!(callback.request().input, PhenixValue::U64(7));
        callback.respond(Ok(ApplicationCapabilityInvokeResult {
            output: PhenixValue::String("ok".to_owned()),
        }));
        let output = worker.await.unwrap().unwrap();
        assert_eq!(
            ApplicationCallableResult::from_value(&output)
                .unwrap()
                .output,
            PhenixValue::String("ok".to_owned())
        );

        service
            .invoke(
                &ContractId::parse(RemoveClientTool::ID).unwrap(),
                ApplicationClientToolRemoveInput {
                    session_id: session.clone(),
                    admission_id: admission.admission_id,
                }
                .to_value(),
            )
            .unwrap();
        assert!(matches!(
            service.invoke(
                &ContractId::parse(InvokeCallable::ID).unwrap(),
                ApplicationCallableInvokeInput {
                    session_id: session,
                    callable_id,
                    input: PhenixValue::U64(7),
                }
                .to_value(),
            ),
            Err(ApplicationError::NotFound { .. })
        ));
    }

    #[tokio::test]
    async fn closing_the_application_transport_retires_client_tool_admissions() {
        let capabilities = SharedCapabilityRegistry::default();
        let (callbacks, _receiver) = ClientCapabilityCallbacks::bounded(1);
        let service = SdkApplicationService {
            sdk: ApplicationSdkValue {
                schema: Type::Table(Default::default()),
                value: PhenixValue::Table(Default::default()),
            },
            capabilities,
            client_callbacks: callbacks,
            client_owner: ClientConnectionId::parse("fixture-client").unwrap(),
            client_generation: CapabilityGenerationId::parse("fixture-generation").unwrap(),
            admissions: Arc::new(Mutex::new(ClientToolAdmissions::default())),
        };
        let session = phenix_core::SessionId::parse("session-a").unwrap();
        service
            .invoke(
                &ContractId::parse(AddClientTool::ID).unwrap(),
                ApplicationClientToolAddInput {
                    session_id: session.clone(),
                    tool: ApplicationClientToolDefinition {
                        id: phenix_core::CallableId::parse("fixture.client.echo").unwrap(),
                        description: "Echo a client value".to_owned(),
                        input: Type::U64,
                        output: Type::String,
                        capabilities: Vec::new(),
                        requires_permission: false,
                        invoke: PhenixValue::Callable(client_callable()),
                    },
                }
                .to_value(),
            )
            .unwrap();
        let observer = service.clone();
        let (transport, receiver) = ChannelTransport::new(1);
        let worker = tokio::spawn(serve_sdk_application(service, receiver));

        drop(transport);
        worker.await.unwrap();

        assert!(observer.client_tool_descriptors(&session).is_empty());
        assert_eq!(
            observer.capabilities().schema(&client_callable()),
            Err(CapabilityError::StaleReference(client_callable()))
        );
    }

    #[test]
    fn retired_client_references_are_rejected_during_admission() {
        let capabilities = SharedCapabilityRegistry::default();
        let (callbacks, _receiver) = ClientCapabilityCallbacks::bounded(1);
        let service = SdkApplicationService {
            sdk: ApplicationSdkValue {
                schema: Type::Table(Default::default()),
                value: PhenixValue::Table(Default::default()),
            },
            capabilities,
            client_callbacks: callbacks,
            client_owner: ClientConnectionId::parse("fixture-client").unwrap(),
            client_generation: CapabilityGenerationId::parse("fixture-generation").unwrap(),
            admissions: Arc::new(Mutex::new(ClientToolAdmissions::default())),
        };
        service.retire_client();

        assert!(matches!(
            service.invoke(
                &ContractId::parse(AddClientTool::ID).unwrap(),
                ApplicationClientToolAddInput {
                    session_id: phenix_core::SessionId::parse("session-a").unwrap(),
                    tool: ApplicationClientToolDefinition {
                        id: phenix_core::CallableId::parse("fixture.client.echo").unwrap(),
                        description: "Echo a client value".to_owned(),
                        input: Type::U64,
                        output: Type::String,
                        capabilities: Vec::new(),
                        requires_permission: false,
                        invoke: PhenixValue::Callable(client_callable()),
                    },
                }
                .to_value(),
            ),
            Err(ApplicationError::StaleReference { .. })
        ));
    }

    #[test]
    fn client_tool_admission_rejects_malformed_and_foreign_references() {
        let capabilities = SharedCapabilityRegistry::default();
        let (callbacks, _receiver) = ClientCapabilityCallbacks::bounded(1);
        let service = SdkApplicationService {
            sdk: ApplicationSdkValue {
                schema: Type::Table(Default::default()),
                value: PhenixValue::Table(Default::default()),
            },
            capabilities,
            client_callbacks: callbacks,
            client_owner: ClientConnectionId::parse("fixture-client").unwrap(),
            client_generation: CapabilityGenerationId::parse("fixture-generation").unwrap(),
            admissions: Arc::new(Mutex::new(ClientToolAdmissions::default())),
        };
        let definition = |invoke| ApplicationClientToolAddInput {
            session_id: phenix_core::SessionId::parse("session-a").unwrap(),
            tool: ApplicationClientToolDefinition {
                id: phenix_core::CallableId::parse("fixture.client.echo").unwrap(),
                description: "Echo a client value".to_owned(),
                input: Type::U64,
                output: Type::String,
                capabilities: Vec::new(),
                requires_permission: false,
                invoke,
            },
        };

        assert!(matches!(
            service.invoke(
                &ContractId::parse(AddClientTool::ID).unwrap(),
                definition(PhenixValue::String("not-a-callable".to_owned())).to_value(),
            ),
            Err(ApplicationError::SchemaMismatch { .. })
        ));
        let foreign = CallableRef::new(
            ContractId::parse("fixture.client-callback@1").unwrap(),
            CapabilityOwnerId::Client(ClientConnectionId::parse("other-client").unwrap()),
            CapabilityGenerationId::parse("other-generation").unwrap(),
            ReferenceId::parse("callback").unwrap(),
        );
        assert!(matches!(
            service.invoke(
                &ContractId::parse(AddClientTool::ID).unwrap(),
                definition(PhenixValue::Callable(foreign)).to_value(),
            ),
            Err(ApplicationError::SchemaMismatch { .. })
        ));
    }

    #[test]
    fn reconnect_starts_without_the_previous_generation_admissions() {
        let capabilities = SharedCapabilityRegistry::default();
        let (callbacks, _receiver) = ClientCapabilityCallbacks::bounded(1);
        let first = SdkApplicationService {
            sdk: ApplicationSdkValue {
                schema: Type::Table(Default::default()),
                value: PhenixValue::Table(Default::default()),
            },
            capabilities: capabilities.clone(),
            client_callbacks: callbacks,
            client_owner: ClientConnectionId::parse("fixture-client").unwrap(),
            client_generation: CapabilityGenerationId::parse("fixture-generation").unwrap(),
            admissions: Arc::new(Mutex::new(ClientToolAdmissions::default())),
        };
        let session = phenix_core::SessionId::parse("session-a").unwrap();
        first
            .invoke(
                &ContractId::parse(AddClientTool::ID).unwrap(),
                ApplicationClientToolAddInput {
                    session_id: session.clone(),
                    tool: ApplicationClientToolDefinition {
                        id: phenix_core::CallableId::parse("fixture.client.echo").unwrap(),
                        description: "Echo a client value".to_owned(),
                        input: Type::U64,
                        output: Type::String,
                        capabilities: Vec::new(),
                        requires_permission: false,
                        invoke: PhenixValue::Callable(client_callable()),
                    },
                }
                .to_value(),
            )
            .unwrap();
        first.retire_client();

        let (callbacks, _receiver) = ClientCapabilityCallbacks::bounded(1);
        let reconnected = SdkApplicationService {
            sdk: ApplicationSdkValue {
                schema: Type::Table(Default::default()),
                value: PhenixValue::Table(Default::default()),
            },
            capabilities,
            client_callbacks: callbacks,
            client_owner: ClientConnectionId::parse("fixture-client").unwrap(),
            client_generation: CapabilityGenerationId::parse("fixture-generation-2").unwrap(),
            admissions: Arc::new(Mutex::new(ClientToolAdmissions::default())),
        };

        assert!(reconnected.client_tool_descriptors(&session).is_empty());
        assert!(matches!(
            reconnected.invoke(
                &ContractId::parse(InvokeCallable::ID).unwrap(),
                ApplicationCallableInvokeInput {
                    session_id: session,
                    callable_id: phenix_core::CallableId::parse("fixture.client.echo").unwrap(),
                    input: PhenixValue::U64(7),
                }
                .to_value(),
            ),
            Err(ApplicationError::NotFound { .. })
        ));
    }

    #[tokio::test]
    async fn sdk_get_and_capability_invoke_share_the_live_production_registry() {
        let manifest = PluginManifest {
            id: PluginId::parse("testing").unwrap(),
            version: 1,
            execution: PluginExecution::ResourceOnly,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        };
        let value_id = ValueId::parse("testing.state@1").unwrap();
        let mut contribution =
            SdkContribution::new(manifest.id.clone(), SdkNamespace::parse("testing").unwrap());
        contribution.insert_observable(SdkObservableResource::new(
            SdkResourceId::parse("sdk/testing/state").unwrap(),
            ["state"],
            value_id.clone(),
            ValuePath::root(),
            Type::U64,
        ));
        let resolved = ResolvedSdkContributions::resolve(&[manifest], &[], [contribution]).unwrap();
        let store = ObservableStore::default();
        store
            .register(ObservableRegistration {
                id: value_id,
                owner: PluginId::parse("testing").unwrap(),
                schema: Type::U64,
                snapshot_policy: SnapshotPolicy::CopyOnChange,
                initial: PhenixValue::U64(7),
            })
            .unwrap();
        let capabilities = SharedCapabilityRegistry::default();
        let (client_callbacks, _callback_receiver) = ClientCapabilityCallbacks::bounded(1);
        let runtime = RuntimeId::parse("phenix.application-runtime").unwrap();
        let generation = CapabilityGenerationId::parse("application-generation-1").unwrap();
        let service = SdkApplicationService::new(
            &resolved,
            &store,
            capabilities,
            runtime.clone(),
            generation.clone(),
            client_callbacks,
            ClientCapabilityIdentity::new(
                ClientConnectionId::parse("fixture-client").unwrap(),
                CapabilityGenerationId::parse("fixture-generation").unwrap(),
            ),
        )
        .unwrap();
        let (transport, receiver) = ChannelTransport::new(2);
        let worker = tokio::spawn(serve_sdk_application(service, receiver));

        let sdk = transport
            .invoke(&ContractId::parse(GetSdk::ID).unwrap(), Empty {}.to_value())
            .await
            .unwrap();
        let sdk = ApplicationSdkValue::from_value(&sdk).unwrap();
        let PhenixValue::Table(namespaces) = sdk.value else {
            panic!("SDK root is a table");
        };
        let PhenixValue::Table(resources) = namespaces.get("testing").unwrap() else {
            panic!("namespace is a table");
        };
        let PhenixValue::Table(state) = resources.get("state").unwrap() else {
            panic!("state resource is a table");
        };
        let PhenixValue::Callable(get) = state.get("get").unwrap() else {
            panic!("get is callable");
        };
        assert_eq!(get.owner(), &CapabilityOwnerId::Runtime(runtime));
        assert_eq!(get.generation(), &generation);

        let result = transport
            .invoke(
                &ContractId::parse(InvokeCapability::ID).unwrap(),
                ApplicationCapabilityInvokeInput {
                    callable: PhenixValue::Callable(get.clone()),
                    input: PhenixValue::Unit,
                }
                .to_value(),
            )
            .await
            .unwrap();
        let result = ApplicationCapabilityInvokeResult::from_value(&result).unwrap();
        assert_eq!(
            result.output,
            PhenixValue::Table(std::collections::BTreeMap::from([
                (Key::parse("value").unwrap(), PhenixValue::U64(7)),
                (Key::parse("version").unwrap(), PhenixValue::U64(0)),
            ]))
        );

        drop(transport);
        worker.await.unwrap();
    }

    #[test]
    fn application_error_bridge_preserves_structural_class_and_details() {
        let error = application_error_to_acp(ApplicationError::PermissionDenied {
            message: "same display text".to_owned(),
        });
        let data = error.data.expect("structured ACP error data");
        assert_eq!(data["phenix.class"], "permission_denied");
        assert_eq!(data["phenix.details"]["message"], "same display text");
    }

    #[test]
    fn cancellation_keeps_its_application_error_class() {
        let error = application_error_to_acp(ApplicationError::Cancelled);
        let data = error.data.expect("structured ACP error data");
        assert_eq!(data["phenix.class"], "cancelled");
        assert!(data["phenix.details"].is_null());
    }

    #[test]
    fn callback_request_errors_keep_capability_classes() {
        let callable = client_callable();
        let cancelled = Error::request_cancelled().data(json!({
            "phenix.class": "cancelled",
            "phenix.details": null,
        }));
        assert_eq!(
            acp_error_to_capability(cancelled, callable.clone()),
            CapabilityError::Cancelled
        );

        let full = Error::internal_error().data(json!({
            "phenix.class": "queue_full",
            "phenix.details": null,
        }));
        assert_eq!(
            acp_error_to_capability(full, callable.clone()),
            CapabilityError::QueueFull
        );

        let disconnected = Error::internal_error().data(json!({
            "phenix.class": "disconnected",
            "phenix.details": null,
        }));
        assert_eq!(
            acp_error_to_capability(disconnected, callable),
            CapabilityError::Disconnected
        );
    }
}
