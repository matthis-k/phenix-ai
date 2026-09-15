#![forbid(unsafe_code)]

use agent_client_protocol::schema::{
    v1::{
        AgentNotification, AgentRequest, CancelNotification, CloseSessionRequest,
        CloseSessionResponse, ExtNotification, ExtRequest, ExtResponse, InitializeRequest,
        ListSessionsRequest, ListSessionsResponse, LoadSessionRequest, LoadSessionResponse,
        NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse, ResumeSessionRequest,
        ResumeSessionResponse, SessionNotification, SetSessionConfigOptionRequest,
        SetSessionConfigOptionResponse,
    },
    ProtocolVersion,
};
use agent_client_protocol::{
    AcpAgent, AcpAgentConfig, Agent, Client as AcpRole, ConnectTo, ConnectionTo, ErrorCode,
};
use futures::channel::oneshot;
use phenix_application_interface::{
    ApplicationClient, ApplicationTransport, Capabilities, Operation,
};
use phenix_core::{ContractId, PhenixSchema, PhenixValue};
use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    num::NonZeroUsize,
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
};

pub use phenix_application_interface::{
    application_descriptor, types::ApplicationError, INTERFACE_ID,
};

pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/application.rs"));
}

pub struct ApplicationApi<T> {
    application: ApplicationClient<T>,
}

impl<T: ApplicationTransport> ApplicationApi<T> {
    #[must_use]
    pub fn new(transport: T, capabilities: Capabilities) -> Self {
        Self {
            application: ApplicationClient::new(transport, capabilities),
        }
    }

    #[must_use]
    pub fn application(&self) -> &ApplicationClient<T> {
        &self.application
    }

    pub async fn invoke<O: Operation>(
        &self,
        input: O::Input,
    ) -> Result<O::Output, ApplicationError> {
        self.application.invoke::<O>(input).await
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StdioConfig {
    command: PathBuf,
    args: Vec<String>,
    env: BTreeMap<String, String>,
}

impl StdioConfig {
    #[must_use]
    pub fn new(command: impl Into<PathBuf>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            env: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    #[must_use]
    pub fn env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(name.into(), value.into());
        self
    }

    fn agent(&self) -> AcpAgent {
        AcpAgent::new(
            AcpAgentConfig::new(self.command.clone())
                .args(self.args.clone())
                .envs(self.env.clone()),
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestRejection {
    pub code: ErrorCode,
    pub class: Option<String>,
    pub message: String,
    pub details: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClientError {
    Transport(String),
    Protocol(String),
    Cancelled {
        message: String,
        details: Box<Option<serde_json::Value>>,
    },
    Rejected(Box<RequestRejection>),
    UnsupportedCapability {
        operation: ContractId,
        capability: ContractId,
    },
    OutOfOrderUpdate {
        session_id: String,
        expected: u64,
        received: u64,
    },
    UpdateQueueFull,
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(message) => write!(formatter, "ACP transport failure: {message}"),
            Self::Protocol(message) => write!(formatter, "ACP protocol failure: {message}"),
            Self::Cancelled { message, .. } => write!(formatter, "ACP request cancelled: {message}"),
            Self::Rejected(rejection) => {
                if let Some(class) = &rejection.class {
                    write!(
                        formatter,
                        "ACP peer rejected request ({class}, {}): {}",
                        rejection.code, rejection.message
                    )
                } else {
                    write!(
                        formatter,
                        "ACP peer rejected request ({}): {}",
                        rejection.code, rejection.message
                    )
                }
            }
            Self::UnsupportedCapability {
                operation,
                capability,
            } => write!(
                formatter,
                "ACP peer does not support operation {operation}; capability {capability} is unavailable"
            ),
            Self::OutOfOrderUpdate {
                session_id,
                expected,
                received,
            } => write!(
                formatter,
                "ACP session {session_id} update sequence is {received}; expected {expected}"
            ),
            Self::UpdateQueueFull => write!(formatter, "ACP update queue is full"),
        }
    }
}

impl std::error::Error for ClientError {}

fn request_error(error: agent_client_protocol::Error) -> ClientError {
    let message = error.to_string();
    let class = error.data.as_ref().and_then(application_error_class);
    let details = error.data.as_ref().and_then(application_error_details);
    let code = error.code;
    if code == ErrorCode::RequestCancelled || class.as_deref() == Some("cancelled") {
        ClientError::Cancelled {
            message,
            details: Box::new(details),
        }
    } else {
        ClientError::Rejected(Box::new(RequestRejection {
            code,
            class,
            message,
            details,
        }))
    }
}

fn application_error_class(data: &serde_json::Value) -> Option<String> {
    data.get("phenix.class")
        .or_else(|| data.get("phenix").and_then(|phenix| phenix.get("class")))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

fn application_error_details(data: &serde_json::Value) -> Option<serde_json::Value> {
    data.get("phenix.details")
        .or_else(|| data.get("phenix").and_then(|phenix| phenix.get("detail")))
        .cloned()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionMethod {
    pub operation: ContractId,
    pub capability: ContractId,
    pub method: String,
    pub input: PhenixSchema,
    pub output: PhenixSchema,
    pub error: PhenixSchema,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionEvent {
    pub event: ContractId,
    pub capability: ContractId,
    pub method: String,
    pub payload: PhenixSchema,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionCallback {
    pub callback: ContractId,
    pub capability: ContractId,
    pub method: String,
    pub request: PhenixSchema,
    pub response: PhenixSchema,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ApplicationEvent {
    pub event: ContractId,
    pub payload: PhenixValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DescriptorExtensions {
    interface: ContractId,
    methods: BTreeMap<ContractId, ExtensionMethod>,
    events: BTreeMap<String, ExtensionEvent>,
    callbacks: BTreeMap<String, ExtensionCallback>,
    advertised_capabilities: BTreeSet<ContractId>,
}

impl DescriptorExtensions {
    #[must_use]
    pub fn from_descriptor(capabilities: impl IntoIterator<Item = ContractId>) -> Self {
        let descriptor = application_descriptor();
        let advertised_capabilities = capabilities.into_iter().collect::<BTreeSet<_>>();
        let methods = descriptor
            .operations
            .iter()
            .filter(|(_, operation)| advertised_capabilities.contains(&operation.capability))
            .map(|(operation, definition)| {
                (
                    operation.clone(),
                    ExtensionMethod {
                        operation: operation.clone(),
                        capability: definition.capability.clone(),
                        method: extension_name(operation),
                        input: schema(&descriptor, &definition.input),
                        output: schema(&descriptor, &definition.output),
                        error: schema(&descriptor, &definition.error),
                    },
                )
            })
            .collect();
        let events = descriptor
            .events
            .iter()
            .filter(|(_, event)| advertised_capabilities.contains(&event.capability))
            .map(|(event, definition)| {
                let method = extension_name(event);
                (
                    method.clone(),
                    ExtensionEvent {
                        event: event.clone(),
                        capability: definition.capability.clone(),
                        method,
                        payload: schema(&descriptor, &definition.payload),
                    },
                )
            })
            .collect();
        let callbacks = descriptor
            .callbacks
            .iter()
            .filter(|(_, callback)| advertised_capabilities.contains(&callback.capability))
            .map(|(callback, definition)| {
                let method = extension_name(callback);
                (
                    method.clone(),
                    ExtensionCallback {
                        callback: callback.clone(),
                        capability: definition.capability.clone(),
                        method,
                        request: schema(&descriptor, &definition.request),
                        response: schema(&descriptor, &definition.response),
                    },
                )
            })
            .collect();

        Self {
            interface: descriptor.id,
            methods,
            events,
            callbacks,
            advertised_capabilities,
        }
    }

    #[must_use]
    pub fn interface(&self) -> &ContractId {
        &self.interface
    }

    #[must_use]
    pub fn method(&self, operation: &ContractId) -> Option<&ExtensionMethod> {
        self.methods.get(operation)
    }

    pub fn require(&self, operation: &ContractId) -> Result<&ExtensionMethod, ClientError> {
        self.method(operation).ok_or_else(|| {
            let descriptor = application_descriptor();
            let capability = descriptor
                .operations
                .get(operation)
                .map(|definition| definition.capability.clone())
                .unwrap_or_else(|| operation.clone());
            ClientError::UnsupportedCapability {
                operation: operation.clone(),
                capability,
            }
        })
    }

    #[must_use]
    pub fn event(&self, method: &str) -> Option<&ExtensionEvent> {
        self.events.get(method)
    }

    #[must_use]
    pub fn callback(&self, method: &str) -> Option<&ExtensionCallback> {
        self.callbacks
            .get(method)
            .or_else(|| self.callbacks.get(&format!("_{method}")))
    }

    #[must_use]
    pub fn supports(&self, capability: &ContractId) -> bool {
        self.advertised_capabilities.contains(capability)
    }
}

/// One runtime-to-client callback admitted to a bounded host queue.
pub struct ExtensionCallbackRequest {
    pub callback: ContractId,
    pub input: PhenixValue,
    response: oneshot::Sender<Result<PhenixValue, ApplicationError>>,
}

impl ExtensionCallbackRequest {
    pub fn respond(self, response: Result<PhenixValue, ApplicationError>) {
        let _ = self.response.send(response);
    }
}

#[derive(Clone)]
pub struct ExtensionCallbacks {
    sender: mpsc::SyncSender<ExtensionCallbackRequest>,
}

#[derive(Debug)]
enum CallbackError {
    Protocol(String),
    Disconnected(String),
    QueueFull,
    Application(ApplicationError),
}

impl ExtensionCallbacks {
    #[must_use]
    pub fn bounded(capacity: NonZeroUsize) -> (Self, mpsc::Receiver<ExtensionCallbackRequest>) {
        let (sender, receiver) = mpsc::sync_channel(capacity.get());
        (Self { sender }, receiver)
    }

    async fn receive(
        &self,
        request: ExtRequest,
        extensions: &DescriptorExtensions,
    ) -> Result<ExtResponse, CallbackError> {
        let callback = extensions
            .callback(request.method.as_ref())
            .ok_or_else(|| {
                CallbackError::Protocol(format!(
                    "ACP peer sent an unadvertised Phenix extension callback {}",
                    request.method
                ))
            })?;
        let input = serde_json::from_str::<PhenixValue>(request.params.get()).map_err(|error| {
            CallbackError::Protocol(format!(
                "cannot decode ACP extension callback {}: {error}",
                request.method
            ))
        })?;
        callback.request.parse(&input).map_err(|error| {
            CallbackError::Protocol(format!(
                "ACP extension callback {} violates the application descriptor: {error}",
                request.method
            ))
        })?;
        let (response, received) = oneshot::channel();
        self.sender
            .try_send(ExtensionCallbackRequest {
                callback: callback.callback.clone(),
                input,
                response,
            })
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => CallbackError::QueueFull,
                mpsc::TrySendError::Disconnected(_) => CallbackError::Disconnected(
                    "ACP extension callback receiver disconnected".to_owned(),
                ),
            })?;
        let output = received
            .await
            .map_err(|_| {
                CallbackError::Disconnected("ACP extension callback response dropped".to_owned())
            })?
            .map_err(CallbackError::Application)?;
        callback.response.parse(&output).map_err(|error| {
            CallbackError::Protocol(format!(
                "ACP extension callback {} returned an invalid response: {error}",
                callback.callback
            ))
        })?;
        let raw = serde_json::value::to_raw_value(&output)
            .map_err(|error| CallbackError::Protocol(error.to_string()))?;
        Ok(ExtResponse::new(Arc::from(raw)))
    }
}

fn callback_error_to_acp(error: CallbackError) -> agent_client_protocol::Error {
    match error {
        CallbackError::Application(error) => callback_application_error_to_acp(error),
        CallbackError::QueueFull => {
            agent_client_protocol::Error::internal_error().data(serde_json::json!({
                "phenix.class": "queue_full",
                "phenix.details": null,
            }))
        }
        CallbackError::Disconnected(message) => agent_client_protocol::Error::internal_error()
            .data(serde_json::json!({
                "phenix.class": "disconnected",
                "phenix.details": { "message": message },
            })),
        CallbackError::Protocol(message) => {
            agent_client_protocol::Error::invalid_params().data(serde_json::json!({
                "phenix.class": "schema_mismatch",
                "phenix.details": { "message": message },
            }))
        }
    }
}

fn callback_application_error_to_acp(error: ApplicationError) -> agent_client_protocol::Error {
    let data = serde_json::json!({
        "phenix.class": error.class(),
        "phenix.details": callback_application_error_details(&error),
    });
    let error = match &error {
        ApplicationError::UnsupportedCapability { .. } => {
            agent_client_protocol::Error::method_not_found()
        }
        ApplicationError::InvalidInput { .. } => agent_client_protocol::Error::invalid_params(),
        ApplicationError::InvalidResponse { .. }
        | ApplicationError::PermissionDenied { .. }
        | ApplicationError::Conflict { .. }
        | ApplicationError::Failed { .. }
        | ApplicationError::Disconnected => agent_client_protocol::Error::internal_error(),
        ApplicationError::NotFound { resource } => {
            agent_client_protocol::Error::resource_not_found(Some(resource.clone()))
        }
        ApplicationError::UnknownValue { value } | ApplicationError::StaleReference { value } => {
            agent_client_protocol::Error::resource_not_found(Some(value.clone()))
        }
        ApplicationError::InvalidPath { .. } | ApplicationError::SchemaMismatch { .. } => {
            agent_client_protocol::Error::invalid_params()
        }
        ApplicationError::UnsupportedSnapshotPolicy { .. }
        | ApplicationError::TransactionConflict { .. }
        | ApplicationError::SubscriptionCapacity
        | ApplicationError::Closed => agent_client_protocol::Error::internal_error(),
        ApplicationError::Unauthenticated { .. } => agent_client_protocol::Error::auth_required(),
        ApplicationError::Cancelled => agent_client_protocol::Error::request_cancelled(),
    };
    error.data(data)
}

fn callback_application_error_details(error: &ApplicationError) -> serde_json::Value {
    match error {
        ApplicationError::UnsupportedCapability { capability } => {
            serde_json::json!({ "capability": capability.as_str() })
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
        | ApplicationError::TransactionConflict { message } => {
            serde_json::json!({ "message": message })
        }
        ApplicationError::NotFound { resource } => serde_json::json!({ "resource": resource }),
        ApplicationError::UnknownValue { value } | ApplicationError::StaleReference { value } => {
            serde_json::json!({ "value": value })
        }
        ApplicationError::Cancelled
        | ApplicationError::Disconnected
        | ApplicationError::SubscriptionCapacity
        | ApplicationError::Closed => serde_json::Value::Null,
    }
}

#[derive(Clone)]
pub struct ExtensionUpdates {
    sender: UpdateSender<ApplicationEvent>,
}

#[derive(Clone)]
enum UpdateSender<T> {
    Unbounded(mpsc::Sender<T>),
    Bounded(mpsc::SyncSender<T>),
}

impl ExtensionUpdates {
    #[must_use]
    pub fn channel() -> (Self, mpsc::Receiver<ApplicationEvent>) {
        let (sender, receiver) = mpsc::channel();
        (
            Self {
                sender: UpdateSender::Unbounded(sender),
            },
            receiver,
        )
    }

    #[must_use]
    pub fn bounded(capacity: NonZeroUsize) -> (Self, mpsc::Receiver<ApplicationEvent>) {
        let (sender, receiver) = mpsc::sync_channel(capacity.get());
        (
            Self {
                sender: UpdateSender::Bounded(sender),
            },
            receiver,
        )
    }

    fn receive(
        &self,
        notification: ExtNotification,
        extensions: &DescriptorExtensions,
    ) -> Result<(), ClientError> {
        let event = extensions
            .event(notification.method.as_ref())
            .or_else(|| extensions.event(&format!("_{}", notification.method)))
            .ok_or_else(|| {
                ClientError::Protocol(format!(
                    "ACP peer sent an unadvertised Phenix extension event {}",
                    notification.method
                ))
            })?;
        let payload =
            serde_json::from_str::<PhenixValue>(notification.params.get()).map_err(|error| {
                ClientError::Protocol(format!(
                    "cannot decode ACP extension event {}: {error}",
                    notification.method
                ))
            })?;
        event.payload.parse(&payload).map_err(|error| {
            ClientError::Protocol(format!(
                "ACP extension event {} violates the application descriptor: {error}",
                notification.method
            ))
        })?;
        let update = ApplicationEvent {
            event: event.event.clone(),
            payload,
        };
        match &self.sender {
            UpdateSender::Unbounded(sender) => sender.send(update).map_err(|_| {
                ClientError::Transport("ACP extension event receiver disconnected".to_owned())
            }),
            UpdateSender::Bounded(sender) => sender.try_send(update).map_err(|error| match error {
                mpsc::TrySendError::Full(_) => ClientError::UpdateQueueFull,
                mpsc::TrySendError::Disconnected(_) => {
                    ClientError::Transport("ACP extension event receiver disconnected".to_owned())
                }
            }),
        }
    }
}

fn extension_name(operation: &ContractId) -> String {
    let suffix = operation
        .as_str()
        .strip_prefix("phenix.application.")
        .unwrap_or(operation.as_str());
    format!("_phenix/{suffix}")
}

fn schema(
    descriptor: &phenix_application_interface::ApplicationDescriptor,
    id: &ContractId,
) -> PhenixSchema {
    descriptor
        .types
        .get(id)
        .unwrap_or_else(|| panic!("application descriptor is missing schema {id}"))
        .clone()
}

#[derive(Debug, Default)]
pub struct OrderedUpdates {
    next_sequence: BTreeMap<String, u64>,
}

impl OrderedUpdates {
    pub fn accept(
        &mut self,
        session_id: impl Into<String>,
        sequence: u64,
    ) -> Result<(), ClientError> {
        let session_id = session_id.into();
        let expected = self.next_sequence.entry(session_id.clone()).or_insert(0);
        if sequence != *expected {
            return Err(ClientError::OutOfOrderUpdate {
                session_id,
                expected: *expected,
                received: sequence,
            });
        }
        *expected = expected.saturating_add(1);
        Ok(())
    }

    pub fn resume_at(&mut self, session_id: impl Into<String>, next_sequence: u64) {
        self.next_sequence.insert(session_id.into(), next_sequence);
    }
}

#[derive(Clone)]
pub struct SessionUpdates {
    ordered: Arc<Mutex<OrderedUpdates>>,
    sender: UpdateSender<SessionNotification>,
}

impl SessionUpdates {
    #[must_use]
    pub fn channel() -> (Self, mpsc::Receiver<SessionNotification>) {
        let (sender, receiver) = mpsc::channel();
        (
            Self {
                ordered: Arc::new(Mutex::new(OrderedUpdates::default())),
                sender: UpdateSender::Unbounded(sender),
            },
            receiver,
        )
    }

    /// Creates a bounded update queue for hosts that must control local memory use.
    #[must_use]
    pub fn bounded(capacity: NonZeroUsize) -> (Self, mpsc::Receiver<SessionNotification>) {
        let (sender, receiver) = mpsc::sync_channel(capacity.get());
        (
            Self {
                ordered: Arc::new(Mutex::new(OrderedUpdates::default())),
                sender: UpdateSender::Bounded(sender),
            },
            receiver,
        )
    }

    pub fn resume_at(
        &self,
        session_id: impl Into<String>,
        next_sequence: u64,
    ) -> Result<(), ClientError> {
        self.ordered
            .lock()
            .map_err(|_| ClientError::Protocol("ACP update order lock poisoned".to_owned()))?
            .resume_at(session_id, next_sequence);
        Ok(())
    }

    fn receive(&self, notification: SessionNotification) -> Result<(), ClientError> {
        if let Some(sequence) = notification
            .meta
            .as_ref()
            .and_then(|meta| meta.get("phenix.sequence"))
            .and_then(serde_json::Value::as_u64)
        {
            self.ordered
                .lock()
                .map_err(|_| ClientError::Protocol("ACP update order lock poisoned".to_owned()))?
                .accept(notification.session_id.to_string(), sequence)?;
        }
        match &self.sender {
            UpdateSender::Unbounded(sender) => sender
                .send(notification)
                .map_err(|_| ClientError::Transport("ACP update receiver disconnected".to_owned())),
            UpdateSender::Bounded(sender) => {
                sender.try_send(notification).map_err(|error| match error {
                    mpsc::TrySendError::Full(_) => ClientError::UpdateQueueFull,
                    mpsc::TrySendError::Disconnected(_) => {
                        ClientError::Transport("ACP update receiver disconnected".to_owned())
                    }
                })
            }
        }
    }
}

pub struct StreamClient<T> {
    transport: T,
}

impl<T> StreamClient<T> {
    #[must_use]
    pub const fn new(transport: T) -> Self {
        Self { transport }
    }

    #[must_use]
    pub fn into_transport(self) -> T {
        self.transport
    }
}

impl<T: ConnectTo<AcpRole> + 'static> StreamClient<T> {
    pub async fn connect_with_updates<F, Fut, R>(
        self,
        updates: SessionUpdates,
        use_connection: F,
    ) -> Result<R, ClientError>
    where
        F: FnOnce(AcpConnection) -> Fut,
        Fut: Future<Output = Result<R, ClientError>>,
    {
        agent_client_protocol::Client
            .builder()
            .on_receive_notification(
                async move |notification: SessionNotification, _connection| {
                    updates.receive(notification).map_err(|error| {
                        agent_client_protocol::Error::internal_error().data(error.to_string())
                    })
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .connect_with(
                self.transport,
                move |connection: ConnectionTo<Agent>| async move {
                    let initialized = connection
                        .send_request(InitializeRequest::new(ProtocolVersion::V1))
                        .block_task()
                        .await?;
                    let extensions = descriptor_extensions(&initialized)?;
                    use_connection(AcpConnection {
                        connection,
                        extensions,
                    })
                    .await
                    .map_err(|error| {
                        agent_client_protocol::Error::internal_error().data(error.to_string())
                    })
                },
            )
            .await
            .map_err(|error| ClientError::Transport(error.to_string()))
    }

    pub async fn connect_with<F, Fut, R>(self, use_connection: F) -> Result<R, ClientError>
    where
        F: FnOnce(AcpConnection) -> Fut,
        Fut: Future<Output = Result<R, ClientError>>,
    {
        agent_client_protocol::Client
            .builder()
            .connect_with(
                self.transport,
                move |connection: ConnectionTo<Agent>| async move {
                    let initialized = connection
                        .send_request(InitializeRequest::new(ProtocolVersion::V1))
                        .block_task()
                        .await?;
                    let extensions = descriptor_extensions(&initialized)?;
                    use_connection(AcpConnection {
                        connection,
                        extensions,
                    })
                    .await
                    .map_err(|error| {
                        agent_client_protocol::Error::internal_error().data(error.to_string())
                    })
                },
            )
            .await
            .map_err(|error| ClientError::Transport(error.to_string()))
    }

    pub async fn connect_with_updates_and_extensions<F, Fut, R>(
        self,
        updates: SessionUpdates,
        extension_updates: ExtensionUpdates,
        use_connection: F,
    ) -> Result<R, ClientError>
    where
        F: FnOnce(AcpConnection) -> Fut,
        Fut: Future<Output = Result<R, ClientError>>,
    {
        let negotiated = Arc::new(Mutex::new(None::<DescriptorExtensions>));
        let notifications = Arc::clone(&negotiated);
        agent_client_protocol::Client
            .builder()
            .on_receive_notification(
                async move |notification: AgentNotification, _connection| {
                    let result = match notification {
                        AgentNotification::SessionNotification(notification) => {
                            updates.receive(notification)
                        }
                        AgentNotification::ExtNotification(notification) => notifications
                            .lock()
                            .map_err(|_| {
                                ClientError::Protocol(
                                    "ACP extension metadata lock poisoned".to_owned(),
                                )
                            })
                            .and_then(|extensions| {
                                let extensions = extensions.as_ref().ok_or_else(|| {
                                    ClientError::Protocol(
                                        "ACP peer sent an extension event before initialize completed"
                                            .to_owned(),
                                    )
                                })?;
                                extension_updates.receive(notification, extensions)
                            }),
                        _ => Ok(()),
                    };
                    result.map_err(|error| {
                        agent_client_protocol::Error::internal_error().data(error.to_string())
                    })
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .connect_with(
                self.transport,
                move |connection: ConnectionTo<Agent>| async move {
                    let initialized = connection
                        .send_request(InitializeRequest::new(ProtocolVersion::V1))
                        .block_task()
                        .await?;
                    let extensions = descriptor_extensions(&initialized)?;
                    *negotiated.lock().map_err(|_| {
                        agent_client_protocol::Error::internal_error()
                            .data("ACP extension metadata lock poisoned")
                    })? = Some(extensions.clone());
                    use_connection(AcpConnection {
                        connection,
                        extensions,
                    })
                    .await
                    .map_err(|error| {
                        agent_client_protocol::Error::internal_error().data(error.to_string())
                    })
                },
            )
            .await
            .map_err(|error| ClientError::Transport(error.to_string()))
    }

    pub async fn connect_with_updates_extensions_and_callbacks<F, Fut, R>(
        self,
        updates: SessionUpdates,
        extension_updates: ExtensionUpdates,
        callbacks: ExtensionCallbacks,
        use_connection: F,
    ) -> Result<R, ClientError>
    where
        F: FnOnce(AcpConnection) -> Fut,
        Fut: Future<Output = Result<R, ClientError>>,
    {
        let negotiated = Arc::new(Mutex::new(None::<DescriptorExtensions>));
        let notifications = Arc::clone(&negotiated);
        let callback_metadata = Arc::clone(&negotiated);
        agent_client_protocol::Client
            .builder()
            .on_receive_notification(
                async move |notification: AgentNotification, _connection| {
                    let result = match notification {
                        AgentNotification::SessionNotification(notification) => {
                            updates.receive(notification)
                        }
                        AgentNotification::ExtNotification(notification) => notifications
                            .lock()
                            .map_err(|_| {
                                ClientError::Protocol(
                                    "ACP extension metadata lock poisoned".to_owned(),
                                )
                            })
                            .and_then(|extensions| {
                                let extensions = extensions.as_ref().ok_or_else(|| {
                                    ClientError::Protocol(
                                        "ACP peer sent an extension event before initialize completed"
                                            .to_owned(),
                                    )
                                })?;
                                extension_updates.receive(notification, extensions)
                            }),
                        _ => Ok(()),
                    };
                    result.map_err(|error| {
                        agent_client_protocol::Error::internal_error().data(error.to_string())
                    })
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .on_receive_request(
                async move |request: AgentRequest, responder, connection| {
                    let AgentRequest::ExtMethodRequest(request) = request else {
                        return Err(agent_client_protocol::Error::invalid_params().data(
                            "ACP peer sent a non-extension request to the Phenix callback handler",
                        ));
                    };
                    let extensions = callback_metadata
                        .lock()
                        .map_err(|_| {
                            agent_client_protocol::Error::internal_error()
                                .data("ACP extension metadata lock poisoned")
                        })?
                        .clone()
                        .ok_or_else(|| {
                            agent_client_protocol::Error::invalid_params()
                                .data("ACP peer sent an extension callback before initialize completed")
                        })?;
                    let callbacks = callbacks.clone();
                    connection.spawn(async move {
                        let response = callbacks
                            .receive(request, &extensions)
                            .await
                            .map_err(callback_error_to_acp)
                            .and_then(|response| {
                                serde_json::to_value(response).map_err(|error| {
                                    agent_client_protocol::Error::internal_error()
                                        .data(error.to_string())
                                })
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
            .connect_with(
                self.transport,
                move |connection: ConnectionTo<Agent>| async move {
                    let initialized = connection
                        .send_request(InitializeRequest::new(ProtocolVersion::V1))
                        .block_task()
                        .await?;
                    let extensions = descriptor_extensions(&initialized)?;
                    *negotiated.lock().map_err(|_| {
                        agent_client_protocol::Error::internal_error()
                            .data("ACP extension metadata lock poisoned")
                    })? = Some(extensions.clone());
                    use_connection(AcpConnection {
                        connection,
                        extensions,
                    })
                    .await
                    .map_err(|error| {
                        agent_client_protocol::Error::internal_error().data(error.to_string())
                    })
                },
            )
            .await
            .map_err(|error| ClientError::Transport(error.to_string()))
    }
}

#[derive(Clone)]
pub struct AcpClient {
    config: StdioConfig,
}

impl AcpClient {
    #[must_use]
    pub fn new(config: StdioConfig) -> Self {
        Self { config }
    }

    #[must_use]
    pub fn config(&self) -> &StdioConfig {
        &self.config
    }

    pub async fn connect_with<F, Fut, R>(&self, use_connection: F) -> Result<R, ClientError>
    where
        F: FnOnce(AcpConnection) -> Fut,
        Fut: Future<Output = Result<R, ClientError>>,
    {
        StreamClient::new(self.config.agent())
            .connect_with(use_connection)
            .await
    }

    pub async fn connect_with_updates<F, Fut, R>(
        &self,
        updates: SessionUpdates,
        use_connection: F,
    ) -> Result<R, ClientError>
    where
        F: FnOnce(AcpConnection) -> Fut,
        Fut: Future<Output = Result<R, ClientError>>,
    {
        StreamClient::new(self.config.agent())
            .connect_with_updates(updates, use_connection)
            .await
    }

    pub async fn connect_with_updates_and_extensions<F, Fut, R>(
        &self,
        updates: SessionUpdates,
        extension_updates: ExtensionUpdates,
        use_connection: F,
    ) -> Result<R, ClientError>
    where
        F: FnOnce(AcpConnection) -> Fut,
        Fut: Future<Output = Result<R, ClientError>>,
    {
        StreamClient::new(self.config.agent())
            .connect_with_updates_and_extensions(updates, extension_updates, use_connection)
            .await
    }

    pub async fn connect_with_updates_extensions_and_callbacks<F, Fut, R>(
        &self,
        updates: SessionUpdates,
        extension_updates: ExtensionUpdates,
        callbacks: ExtensionCallbacks,
        use_connection: F,
    ) -> Result<R, ClientError>
    where
        F: FnOnce(AcpConnection) -> Fut,
        Fut: Future<Output = Result<R, ClientError>>,
    {
        StreamClient::new(self.config.agent())
            .connect_with_updates_extensions_and_callbacks(
                updates,
                extension_updates,
                callbacks,
                use_connection,
            )
            .await
    }

    pub async fn reconnect_with<F, Fut, R>(&self, use_connection: F) -> Result<R, ClientError>
    where
        F: FnOnce(AcpConnection) -> Fut,
        Fut: Future<Output = Result<R, ClientError>>,
    {
        self.connect_with(use_connection).await
    }
}

fn descriptor_extensions(
    initialized: &agent_client_protocol::schema::v1::InitializeResponse,
) -> Result<DescriptorExtensions, agent_client_protocol::Error> {
    let value = serde_json::to_value(initialized)
        .map_err(agent_client_protocol::Error::into_internal_error)?;
    let extension = value
        .get("_meta")
        .and_then(|meta| meta.get("phenix.extensions"))
        .ok_or_else(|| {
            agent_client_protocol::Error::invalid_params()
                .data("ACP peer did not advertise phenix.extensions metadata")
        })?;
    let interface = extension
        .get("interface")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            agent_client_protocol::Error::invalid_params()
                .data("ACP peer omitted its Phenix application interface id")
        })?;
    if interface != INTERFACE_ID {
        return Err(agent_client_protocol::Error::invalid_params().data(format!(
            "ACP peer advertises application interface {interface}; expected {INTERFACE_ID}"
        )));
    }
    let capabilities = ["methods", "events", "callbacks"]
        .into_iter()
        .flat_map(|kind| {
            extension
                .get(kind)
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|entry| entry.get("capability"))
        .filter_map(serde_json::Value::as_str)
        .map(ContractId::parse)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            agent_client_protocol::Error::invalid_params().data(format!(
                "ACP peer advertised an invalid Phenix capability: {error}"
            ))
        })?;
    Ok(DescriptorExtensions::from_descriptor(capabilities))
}

pub struct AcpConnection {
    connection: ConnectionTo<Agent>,
    extensions: DescriptorExtensions,
}

impl AcpConnection {
    #[must_use]
    pub fn extensions(&self) -> &DescriptorExtensions {
        &self.extensions
    }

    pub async fn new_session(
        &self,
        request: NewSessionRequest,
    ) -> Result<NewSessionResponse, ClientError> {
        self.connection
            .send_request(request)
            .block_task()
            .await
            .map_err(request_error)
    }

    pub async fn list_sessions(
        &self,
        request: ListSessionsRequest,
    ) -> Result<ListSessionsResponse, ClientError> {
        self.connection
            .send_request(request)
            .block_task()
            .await
            .map_err(request_error)
    }

    pub async fn resume_session(
        &self,
        request: ResumeSessionRequest,
    ) -> Result<ResumeSessionResponse, ClientError> {
        self.connection
            .send_request(request)
            .block_task()
            .await
            .map_err(request_error)
    }

    pub async fn load_session(
        &self,
        request: LoadSessionRequest,
    ) -> Result<LoadSessionResponse, ClientError> {
        self.connection
            .send_request(request)
            .block_task()
            .await
            .map_err(request_error)
    }

    pub async fn close_session(
        &self,
        request: CloseSessionRequest,
    ) -> Result<CloseSessionResponse, ClientError> {
        self.connection
            .send_request(request)
            .block_task()
            .await
            .map_err(request_error)
    }

    pub async fn prompt(&self, request: PromptRequest) -> Result<PromptResponse, ClientError> {
        self.connection
            .send_request(request)
            .block_task()
            .await
            .map_err(request_error)
    }

    pub async fn set_session_config_option(
        &self,
        request: SetSessionConfigOptionRequest,
    ) -> Result<SetSessionConfigOptionResponse, ClientError> {
        self.connection
            .send_request(request)
            .block_task()
            .await
            .map_err(request_error)
    }

    pub fn cancel(&self, notification: CancelNotification) {
        let _ = self.connection.send_notification(notification);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::InitializeResponse;
    use phenix_application_interface::types::{SessionChange, SessionUpdate};
    use phenix_core::{PhenixContract, PhenixValue, ValueCodec};

    #[derive(phenix_sdk_macros::PhenixValue)]
    struct Request;

    impl PhenixContract for Request {
        fn contract_id() -> ContractId {
            ContractId::parse("fixture.client.request@1").expect("static contract id is valid")
        }
    }

    #[derive(Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
    struct Response;

    impl PhenixContract for Response {
        fn contract_id() -> ContractId {
            ContractId::parse("fixture.client.response@1").expect("static contract id is valid")
        }
    }

    struct FixtureOperation;

    impl Operation for FixtureOperation {
        const ID: &'static str = "phenix.application.capabilities@1";
        const CAPABILITY: &'static str = "phenix.application.capability.discovery@1";
        type Input = Request;
        type Output = Response;
    }

    #[derive(Clone)]
    struct RejectingTransport(ApplicationError);

    impl ApplicationTransport for RejectingTransport {
        fn invoke(
            &self,
            _operation: &ContractId,
            _input: PhenixValue,
        ) -> impl Future<Output = Result<PhenixValue, ApplicationError>> {
            std::future::ready(Err(self.0.clone()))
        }
    }

    fn client(error: ApplicationError) -> ApplicationApi<RejectingTransport> {
        let capability =
            ContractId::parse(FixtureOperation::CAPABILITY).expect("static capability id is valid");
        let capabilities = Capabilities::negotiate(&application_descriptor(), [capability])
            .expect("discovery has no missing dependency");
        ApplicationApi::new(RejectingTransport(error), capabilities)
    }

    #[test]
    fn generated_api_reports_the_fixed_interface_identity() {
        assert_eq!(generated::INTERFACE_ID, INTERFACE_ID);
        assert_eq!(generated::DESCRIPTOR_SHA256.len(), 64);
        assert!(generated::type_schemas().contains_key(
            &ContractId::parse("phenix.application.error@1").expect("static contract id is valid"),
        ));
    }

    #[test]
    fn generated_source_is_deterministic_for_the_fixed_descriptor() {
        let first = phenix_application_interface::generate::rust(&application_descriptor())
            .expect("fixed descriptor is generatable");
        let second = phenix_application_interface::generate::rust(&application_descriptor())
            .expect("fixed descriptor is generatable");
        assert_eq!(first, second);
    }

    #[test]
    fn typed_client_preserves_typed_application_failures() {
        let cases = [
            ApplicationError::Conflict {
                message: "same text".to_owned(),
            },
            ApplicationError::PermissionDenied {
                message: "same text".to_owned(),
            },
            ApplicationError::Cancelled,
            ApplicationError::Disconnected,
        ];

        for expected in cases {
            let result = futures::executor::block_on(
                client(expected.clone()).invoke::<FixtureOperation>(Request),
            );
            assert_eq!(result, Err(expected));
        }
    }

    #[test]
    fn request_errors_distinguish_cancellation_from_peer_rejection() {
        let cancelled = request_error(agent_client_protocol::Error::request_cancelled().data(
            serde_json::json!({
                "phenix.class": "cancelled",
                "phenix.details": null,
            }),
        ));
        assert!(matches!(cancelled, ClientError::Cancelled { .. }));

        let rejected = request_error(agent_client_protocol::Error::internal_error().data(
            serde_json::json!({
                "phenix.class": "permission_denied",
                "phenix.details": { "message": "same display text" },
            }),
        ));
        assert!(matches!(
            rejected,
            ClientError::Rejected(ref rejection)
                if rejection.code == ErrorCode::InternalError
                    && rejection.class.as_deref() == Some("permission_denied")
                    && rejection.details.as_ref().is_some_and(
                        |details| details["message"] == "same display text"
                    )
        ));
    }

    #[test]
    fn request_errors_accept_the_adapter_error_data_shape() {
        let rejected = request_error(agent_client_protocol::Error::internal_error().data(
            serde_json::json!({
                "phenix": {
                    "class": "conflict",
                    "detail": { "message": "same display text" },
                }
            }),
        ));
        assert!(matches!(
            rejected,
            ClientError::Rejected(ref rejection)
                if rejection.class.as_deref() == Some("conflict")
                    && rejection.details.as_ref().is_some_and(
                        |details| details["message"] == "same display text"
                    )
        ));
    }

    #[test]
    fn callback_errors_preserve_capability_classes() {
        let cancelled =
            callback_error_to_acp(CallbackError::Application(ApplicationError::Cancelled));
        assert_eq!(cancelled.code, ErrorCode::RequestCancelled);
        assert_eq!(
            cancelled.data.as_ref().unwrap()["phenix.class"],
            "cancelled"
        );

        let full = callback_error_to_acp(CallbackError::QueueFull);
        assert_eq!(full.data.as_ref().unwrap()["phenix.class"], "queue_full");

        let disconnected = callback_error_to_acp(CallbackError::Disconnected("gone".to_owned()));
        assert_eq!(
            disconnected.data.as_ref().unwrap()["phenix.class"],
            "disconnected"
        );
    }

    #[test]
    fn descriptor_extensions_use_the_fixed_operation_and_schema_ids() {
        let capability = ContractId::parse("phenix.application.capability.skills@1")
            .expect("static capability id is valid");
        let extensions = DescriptorExtensions::from_descriptor([capability]);
        let operation = ContractId::parse("phenix.application.skill-list@1")
            .expect("static operation id is valid");
        let method = extensions
            .require(&operation)
            .expect("skills capability supports listing");
        assert_eq!(method.method, "_phenix/skill-list@1");
        assert_eq!(method.operation, operation);
    }

    #[test]
    fn unavailable_descriptor_operations_fail_before_the_request_is_sent() {
        let extensions = DescriptorExtensions::from_descriptor([]);
        let operation = ContractId::parse("phenix.application.session.create@1")
            .expect("static operation id is valid");
        assert!(matches!(
            extensions.require(&operation),
            Err(ClientError::UnsupportedCapability { .. })
        ));
    }

    #[test]
    fn stream_client_initializes_and_maps_a_session_request() {
        let (client_transport, server_transport) = agent_client_protocol::Channel::duplex();
        let server = Agent
            .builder()
            .on_receive_request(
                async move |request: InitializeRequest, responder, _connection| {
                    let mut meta = serde_json::Map::new();
                    meta.insert(
                        "phenix.extensions".to_owned(),
                        serde_json::json!({
                            "interface": INTERFACE_ID,
                            "methods": [],
                        }),
                    );
                    responder.respond(InitializeResponse::new(request.protocol_version).meta(meta))
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |_request: NewSessionRequest, responder, _connection| {
                    responder.respond(NewSessionResponse::new("session-1"))
                },
                agent_client_protocol::on_receive_request!(),
            )
            .connect_to(server_transport);
        let client = StreamClient::new(client_transport).connect_with(|connection| async move {
            let session = connection
                .new_session(NewSessionRequest::new("/workspace"))
                .await?;
            assert_eq!(session.session_id.to_string(), "session-1");
            Ok(())
        });

        let (server_result, client_result) =
            futures::executor::block_on(async { futures::join!(server, client) });
        client_result.expect("client completes the ACP session request");
        server_result.expect("server completes after the client disconnects");
    }

    #[test]
    fn stream_client_delivers_negotiated_extension_events() {
        let (client_transport, server_transport) = agent_client_protocol::Channel::duplex();
        let server = Agent
            .builder()
            .on_receive_request(
                async move |request: InitializeRequest, responder, _connection| {
                    let mut meta = serde_json::Map::new();
                    meta.insert(
                        "phenix.extensions".to_owned(),
                        serde_json::json!({
                            "interface": INTERFACE_ID,
                            "methods": [],
                            "events": [{
                                "event": "phenix.application.session-update@1",
                                "capability": "phenix.application.capability.sessions@1",
                            }],
                        }),
                    );
                    responder.respond(InitializeResponse::new(request.protocol_version).meta(meta))
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |_request: NewSessionRequest, responder, connection| {
                    let update = SessionUpdate {
                        session_id: phenix_core::SessionId::parse("session-1")
                            .expect("static session id is valid"),
                        sequence: 0,
                        update: SessionChange::Renamed {
                            title: "Renamed".to_owned(),
                        },
                    };
                    let raw = serde_json::value::to_raw_value(&update.to_value())
                        .expect("application event is JSON");
                    connection
                        .send_notification(AgentNotification::ExtNotification(
                            ExtNotification::new("_phenix/session-update@1", Arc::from(raw)),
                        ))
                        .expect("extension event is sent");
                    let _ = responder.respond(NewSessionResponse::new("session-1"));
                    Ok::<_, agent_client_protocol::Error>(())
                },
                agent_client_protocol::on_receive_request!(),
            )
            .connect_to(server_transport);
        let (sessions, _session_receiver) = SessionUpdates::channel();
        let (events, receiver) = ExtensionUpdates::channel();
        let client = StreamClient::new(client_transport).connect_with_updates_and_extensions(
            sessions,
            events,
            |connection| async move {
                connection
                    .new_session(NewSessionRequest::new("/workspace"))
                    .await?;
                Ok(())
            },
        );

        let (server_result, client_result) =
            futures::executor::block_on(async { futures::join!(server, client) });
        client_result.expect("client completes the ACP session request");
        server_result.expect("server completes after the client disconnects");
        let event = receiver
            .try_recv()
            .expect("negotiated extension event is delivered");
        assert_eq!(event.event.as_str(), "phenix.application.session-update@1");
        assert_eq!(
            event.payload,
            SessionUpdate {
                session_id: phenix_core::SessionId::parse("session-1")
                    .expect("static session id is valid"),
                sequence: 0,
                update: SessionChange::Renamed {
                    title: "Renamed".to_owned(),
                },
            }
            .to_value()
        );
    }

    #[test]
    fn session_updates_deliver_only_ordered_phenix_notifications() {
        let (updates, receiver) = SessionUpdates::channel();
        let mut meta = serde_json::Map::new();
        meta.insert("phenix.sequence".to_owned(), serde_json::Value::from(0));
        let notification = SessionNotification::new(
            "session-1",
            agent_client_protocol::schema::v1::SessionUpdate::AgentMessageChunk(
                agent_client_protocol::schema::v1::ContentChunk::new(
                    agent_client_protocol::schema::v1::ContentBlock::Text(
                        agent_client_protocol::schema::v1::TextContent::new("hello"),
                    ),
                ),
            ),
        )
        .meta(meta);
        updates
            .receive(notification)
            .expect("first Phenix notification is ordered");
        assert_eq!(
            receiver
                .try_recv()
                .expect("ordered notification is delivered")
                .session_id
                .to_string(),
            "session-1"
        );

        let mut gap_meta = serde_json::Map::new();
        gap_meta.insert("phenix.sequence".to_owned(), serde_json::Value::from(2));
        let gap = SessionNotification::new(
            "session-1",
            agent_client_protocol::schema::v1::SessionUpdate::AgentMessageChunk(
                agent_client_protocol::schema::v1::ContentChunk::new(
                    agent_client_protocol::schema::v1::ContentBlock::Text(
                        agent_client_protocol::schema::v1::TextContent::new("gap"),
                    ),
                ),
            ),
        )
        .meta(gap_meta);
        assert!(matches!(
            updates.receive(gap),
            Err(ClientError::OutOfOrderUpdate {
                expected: 1,
                received: 2,
                ..
            })
        ));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn ordered_updates_reject_gaps_and_allow_resume() {
        let mut updates = OrderedUpdates::default();
        updates
            .accept("session-1", 0)
            .expect("first update is ordered");
        assert!(matches!(
            updates.accept("session-1", 2),
            Err(ClientError::OutOfOrderUpdate {
                expected: 1,
                received: 2,
                ..
            })
        ));
        updates.resume_at("session-1", 7);
        updates
            .accept("session-1", 7)
            .expect("resumed update is ordered");
    }

    #[test]
    fn bounded_session_updates_reject_overflow() {
        let (updates, receiver) =
            SessionUpdates::bounded(std::num::NonZeroUsize::new(1).expect("one is non-zero"));
        let notification = || {
            SessionNotification::new(
                "session-1",
                agent_client_protocol::schema::v1::SessionUpdate::AgentMessageChunk(
                    agent_client_protocol::schema::v1::ContentChunk::new(
                        agent_client_protocol::schema::v1::ContentBlock::Text(
                            agent_client_protocol::schema::v1::TextContent::new("hello"),
                        ),
                    ),
                ),
            )
        };

        updates.receive(notification()).expect("first update fits");
        assert!(matches!(
            updates.receive(notification()),
            Err(ClientError::UpdateQueueFull)
        ));
        drop(receiver);
    }
}
