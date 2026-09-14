use super::WorkerMessage;
use agent_client_protocol::schema::v1::{
    ConnectMcpRequest, ConnectMcpResponse, DisconnectMcpRequest, DisconnectMcpResponse,
    McpConnectionId, McpServer, McpServerAcp, MessageMcpNotification, MessageMcpRequest,
    MessageMcpResponse,
};
use parking_lot::Mutex;
use phenix_backend::{
    BackendError, PreparedToolSurface, ToolCancellation, ToolInvocation, ToolPresentation,
    ToolResult,
};
use phenix_domain::{CallableDescriptor, PhenixSchema};
use rmcp::model::{
    CallToolRequestMethod, CallToolRequestParams, CallToolResult, CancelledNotificationMethod,
    CancelledNotificationParam, ConstString, ContentBlock, EmptyObject, ErrorCode, ErrorData,
    Implementation, InitializeRequestParams, InitializeResult, InitializeResultMethod,
    InitializedNotificationMethod, JsonObject, ListToolsRequestMethod, ListToolsResult,
    PingRequestMethod, ProtocolVersion, ServerCapabilities, Tool, ToolsCapability,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, value::RawValue, Map, Value};
use std::collections::BTreeMap;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

const SERVER_ID: &str = "phenix-tools";
const SERVER_NAME: &str = "Phenix tools";
const SERVER_IMPLEMENTATION_NAME: &str = "phenix-conductor";
const SERVER_IMPLEMENTATION_VERSION: &str = "0.1.0";
const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// The newest stable protocol revision this bridge opts into. The rmcp SDK
/// deliberately keeps `LATEST` below revisions that require newer transport
/// semantics, so the ACP carrier inherits that compatibility boundary instead
/// of maintaining a second local version table.
const MAX_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::LATEST;

#[derive(Clone, Default)]
pub(super) struct ToolBridge {
    state: Arc<Mutex<ToolBridgeState>>,
}

#[derive(Default)]
struct ToolBridgeState {
    callables: BTreeMap<String, CallableDescriptor>,
    worker: Option<mpsc::Sender<WorkerMessage>>,
    connections: BTreeMap<String, ConnectionState>,
    next_connection: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConnectionLifecycle {
    Connected,
    InitializeResponded,
    Initialized,
}

/// The MCP-over-ACP carrier keeps the wrapped MCP request identity in the outer
/// ACP JSON-RPC envelope rather than in `MessageMcpRequest`. The ACP handler
/// passes that id into the bridge so a cancellation can only affect the request
/// it names, including when a late cancellation arrives after a newer call began.
struct ConnectionState {
    lifecycle: ConnectionLifecycle,
    in_flight: Option<InFlightCall>,
}

struct InFlightCall {
    request_id: Value,
    cancellation: ToolCancellation,
}

impl ToolBridge {
    pub(super) fn is_tool_call(request: &MessageMcpRequest) -> bool {
        request.method == CallToolRequestMethod::VALUE
    }

    pub(super) fn server(&self) -> McpServer {
        McpServer::Acp(McpServerAcp::new(SERVER_NAME, SERVER_ID))
    }

    pub(super) fn provision(&self, tools: &PreparedToolSurface) -> Result<(), BackendError> {
        if !tools.is_empty() && tools.presentation() != Some(ToolPresentation::AcpExtension) {
            return Err(BackendError::Unsupported(
                "ACP tool bridge requires the negotiated ACP extension presentation".to_owned(),
            ));
        }
        let mut state = self.state.lock();
        state.callables = tools
            .callables()
            .iter()
            .cloned()
            .map(|callable| (callable.id.as_str().to_owned(), callable))
            .collect();
        Ok(())
    }

    pub(super) fn bind_execution(
        &self,
        tools: &PreparedToolSurface,
        worker: mpsc::Sender<WorkerMessage>,
    ) -> Result<(), BackendError> {
        self.provision(tools)?;
        self.state.lock().worker = Some(worker);
        Ok(())
    }

    pub(super) fn unbind_execution(&self) {
        let mut state = self.state.lock();
        state.worker = None;
        for connection in state.connections.values() {
            cancel_in_flight(connection);
        }
    }

    pub(super) fn connect(
        &self,
        request: ConnectMcpRequest,
    ) -> Result<ConnectMcpResponse, agent_client_protocol::Error> {
        if request.server_id.0.as_ref() != SERVER_ID {
            return Err(agent_client_protocol::Error::invalid_params()
                .data(format!("unknown Phenix MCP server {}", request.server_id)));
        }
        let mut state = self.state.lock();
        state.next_connection += 1;
        let connection_id = format!("phenix-tools-{}", state.next_connection);
        state.connections.insert(
            connection_id.clone(),
            ConnectionState {
                lifecycle: ConnectionLifecycle::Connected,
                in_flight: None,
            },
        );
        Ok(ConnectMcpResponse::new(connection_id))
    }

    pub(super) fn disconnect(
        &self,
        request: DisconnectMcpRequest,
    ) -> Result<DisconnectMcpResponse, agent_client_protocol::Error> {
        if let Some(connection) = self
            .state
            .lock()
            .connections
            .remove(request.connection_id.0.as_ref())
        {
            cancel_in_flight(&connection);
        }
        Ok(DisconnectMcpResponse::new())
    }

    pub(super) fn message(
        &self,
        request: MessageMcpRequest,
    ) -> Result<MessageMcpResponse, agent_client_protocol::Error> {
        self.message_with_request_id(request, json!(0))
    }

    pub(super) fn message_with_request_id(
        &self,
        request: MessageMcpRequest,
        request_id: Value,
    ) -> Result<MessageMcpResponse, agent_client_protocol::Error> {
        validate_mcp_request_id(&request_id)?;
        self.require_connection(&request.connection_id)?;
        let result = match request.method.as_str() {
            InitializeResultMethod::VALUE => {
                self.initialize(&request.connection_id, request.params.as_ref())?
            }
            PingRequestMethod::VALUE => {
                self.require_initialized(&request.connection_id)?;
                to_value(&EmptyObject {})?
            }
            ListToolsRequestMethod::VALUE => {
                self.require_initialized(&request.connection_id)?;
                self.list_tools()?
            }
            CallToolRequestMethod::VALUE => {
                self.require_initialized(&request.connection_id)?;
                self.call_tool(&request.connection_id, request_id, request.params.as_ref())?
            }
            method => {
                return Err(acp_error(ErrorData::new(
                    ErrorCode::METHOD_NOT_FOUND,
                    method.to_owned(),
                    None,
                )));
            }
        };
        Ok(MessageMcpResponse::new(raw_value(result)?))
    }

    pub(super) fn notification(
        &self,
        notification: MessageMcpNotification,
    ) -> Result<(), agent_client_protocol::Error> {
        if !self
            .state
            .lock()
            .connections
            .contains_key(notification.connection_id.0.as_ref())
        {
            // JSON-RPC notifications are fire-and-forget. A late notification
            // for a disconnected MCP connection has no peer response to carry
            // an error and is intentionally ignored.
            return Ok(());
        }
        match notification.method.as_str() {
            InitializedNotificationMethod::VALUE => {
                self.mark_initialized(&notification.connection_id)
            }
            CancelledNotificationMethod::VALUE => {
                // Invalid cancellation notifications are fire-and-forget. A
                // pre-initialization cancellation cannot name a cancellable MCP
                // request, so ignore it instead of surfacing a local-only error.
                if self
                    .require_initialized(&notification.connection_id)
                    .is_err()
                {
                    return Ok(());
                }
                self.cancel_in_flight_call(
                    &notification.connection_id,
                    notification.params.as_ref(),
                )
            }
            // Unknown nested notifications have no response channel. Ignore
            // them like the upstream ACP MCP bridge rather than manufacturing
            // a method-not-found error that can only be logged locally.
            _ => Ok(()),
        }
    }

    fn initialize(
        &self,
        connection_id: &McpConnectionId,
        params: Option<&Map<String, Value>>,
    ) -> Result<Value, agent_client_protocol::Error> {
        let request: InitializeRequestParams = parse_params(params)?;
        let protocol_version = negotiate_protocol_version(request.protocol_version);
        let capabilities = server_capabilities();
        let result = InitializeResult::new(capabilities)
            .with_protocol_version(protocol_version)
            .with_server_info(Implementation::new(
                SERVER_IMPLEMENTATION_NAME,
                SERVER_IMPLEMENTATION_VERSION,
            ));
        let result = to_value(&result)?;
        self.mark_initialize_responded(connection_id)?;
        Ok(result)
    }

    fn mark_initialize_responded(
        &self,
        connection_id: &McpConnectionId,
    ) -> Result<(), agent_client_protocol::Error> {
        let mut state = self.state.lock();
        let connection = state
            .connections
            .get_mut(connection_id.0.as_ref())
            .ok_or_else(|| unknown_connection(connection_id))?;
        if connection.lifecycle != ConnectionLifecycle::Connected {
            return Err(acp_error(ErrorData::invalid_request(
                "MCP initialize request after initialization started",
                None,
            )));
        }
        connection.lifecycle = ConnectionLifecycle::InitializeResponded;
        Ok(())
    }

    fn mark_initialized(
        &self,
        connection_id: &McpConnectionId,
    ) -> Result<(), agent_client_protocol::Error> {
        let mut state = self.state.lock();
        let connection = state
            .connections
            .get_mut(connection_id.0.as_ref())
            .ok_or_else(|| unknown_connection(connection_id))?;
        if connection.lifecycle != ConnectionLifecycle::InitializeResponded {
            return Err(acp_error(ErrorData::invalid_request(
                "MCP initialized notification out of order",
                None,
            )));
        }
        connection.lifecycle = ConnectionLifecycle::Initialized;
        Ok(())
    }

    fn list_tools(&self) -> Result<Value, agent_client_protocol::Error> {
        let state = self.state.lock();
        let tools = state
            .callables
            .values()
            .map(|callable| {
                let input_schema = tool_input_schema(&callable.input_schema)?;
                Ok(Tool::new(
                    callable.id.as_str().to_owned(),
                    callable.description.clone(),
                    input_schema,
                ))
            })
            .collect::<Result<Vec<_>, agent_client_protocol::Error>>()?;
        let mut result = ListToolsResult::with_all_items(tools);
        result.result_type = None;
        to_value(&result)
    }

    fn call_tool(
        &self,
        connection_id: &McpConnectionId,
        request_id: Value,
        params: Option<&Map<String, Value>>,
    ) -> Result<Value, agent_client_protocol::Error> {
        let request: CallToolRequestParams = parse_params(params)?;
        let name = request.name.to_string();
        let arguments = request.arguments.clone().unwrap_or_default();

        let (callable, worker, cancellation) = {
            let mut state = self.state.lock();
            let callable = state
                .callables
                .get(&name)
                .ok_or_else(|| {
                    agent_client_protocol::Error::invalid_params().data(format!(
                        "tool is not provisioned for this execution: {name}"
                    ))
                })?
                .id
                .clone();
            let worker = state.worker.clone().ok_or_else(|| {
                agent_client_protocol::Error::internal_error()
                    .data("ACP tool call arrived outside an active execution")
            })?;
            let connection = state
                .connections
                .get_mut(connection_id.0.as_ref())
                .ok_or_else(|| unknown_connection(connection_id))?;
            if connection.in_flight.is_some() {
                return Err(acp_error(ErrorData::invalid_request(
                    "MCP connection already has an in-flight tool call",
                    None,
                )));
            }
            let cancellation = ToolCancellation::new();
            connection.in_flight = Some(InFlightCall {
                request_id,
                cancellation: cancellation.clone(),
            });
            (callable, worker, cancellation)
        };

        let arguments_json = serde_json::to_string(&arguments)
            .map_err(agent_client_protocol::Error::into_internal_error)?;
        let (response_tx, response_rx) = mpsc::sync_channel(1);
        let tool_request = BridgeToolRequest {
            invocation: ToolInvocation {
                callable,
                arguments_json,
                cancellation: cancellation.clone(),
            },
            response: response_tx,
        };
        if let Err(error) = worker.send(WorkerMessage::ToolCall(tool_request)) {
            self.clear_in_flight(connection_id);
            return Err(agent_client_protocol::Error::internal_error()
                .data(format!("conductor tool host is unavailable: {error}")));
        }

        let result = wait_for_tool_result(&response_rx, &cancellation);
        self.clear_in_flight(connection_id);
        to_value(&call_tool_result(result))
    }

    fn clear_in_flight(&self, connection_id: &McpConnectionId) {
        if let Some(connection) = self
            .state
            .lock()
            .connections
            .get_mut(connection_id.0.as_ref())
        {
            connection.in_flight = None;
        }
    }

    fn cancel_in_flight_call(
        &self,
        connection_id: &McpConnectionId,
        params: Option<&Map<String, Value>>,
    ) -> Result<(), agent_client_protocol::Error> {
        let cancel: CancelledNotificationParam = match parse_params(params) {
            Ok(cancel) => cancel,
            Err(_) => return Ok(()),
        };
        let Some(request_id) = cancel.request_id else {
            return Ok(());
        };
        let request_id = to_value(&request_id)?;
        if let Some(connection) = self.state.lock().connections.get(connection_id.0.as_ref()) {
            if let Some(in_flight) = connection.in_flight.as_ref() {
                if in_flight.request_id == request_id {
                    in_flight.cancellation.cancel();
                }
            }
        }
        Ok(())
    }

    fn require_connection(
        &self,
        connection_id: &McpConnectionId,
    ) -> Result<(), agent_client_protocol::Error> {
        if self
            .state
            .lock()
            .connections
            .contains_key(connection_id.0.as_ref())
        {
            Ok(())
        } else {
            Err(unknown_connection(connection_id))
        }
    }

    fn require_initialized(
        &self,
        connection_id: &McpConnectionId,
    ) -> Result<(), agent_client_protocol::Error> {
        let state = self.state.lock();
        let connection = state
            .connections
            .get(connection_id.0.as_ref())
            .ok_or_else(|| unknown_connection(connection_id))?;
        if connection.lifecycle == ConnectionLifecycle::Initialized {
            Ok(())
        } else {
            Err(acp_error(ErrorData::invalid_request(
                "MCP request before initialization",
                None,
            )))
        }
    }
}

fn wait_for_tool_result(
    response_rx: &mpsc::Receiver<Result<ToolResult, BackendError>>,
    cancellation: &ToolCancellation,
) -> Result<ToolResult, BackendError> {
    loop {
        match response_rx.recv_timeout(CANCELLATION_POLL_INTERVAL) {
            Ok(result) => {
                if cancellation.is_cancelled() {
                    return Err(cancelled_tool_call());
                }
                return result;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if cancellation.is_cancelled() {
                    return Err(cancelled_tool_call());
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if cancellation.is_cancelled() {
                    return Err(cancelled_tool_call());
                }
                return Err(BackendError::Protocol(
                    "conductor tool result channel closed before completion".to_owned(),
                ));
            }
        }
    }
}

fn cancelled_tool_call() -> BackendError {
    BackendError::Protocol("MCP tool call was cancelled".to_owned())
}

fn cancel_in_flight(connection: &ConnectionState) {
    if let Some(in_flight) = connection.in_flight.as_ref() {
        in_flight.cancellation.cancel();
    }
}

fn unknown_connection(connection_id: &McpConnectionId) -> agent_client_protocol::Error {
    agent_client_protocol::Error::invalid_params()
        .data(format!("unknown Phenix MCP connection {connection_id}"))
}

fn validate_mcp_request_id(request_id: &Value) -> Result<(), agent_client_protocol::Error> {
    match request_id {
        Value::String(_) => Ok(()),
        Value::Number(number) if number.is_i64() || number.is_u64() => Ok(()),
        _ => Err(acp_error(ErrorData::invalid_request(
            "MCP request id must be a string or integer",
            None,
        ))),
    }
}

#[derive(Debug)]
pub(super) struct BridgeToolRequest {
    pub(super) invocation: ToolInvocation,
    pub(super) response: mpsc::SyncSender<Result<ToolResult, BackendError>>,
}

/// Negotiate the MCP protocol version against the client's request. The client
/// requests its own version; the bridge echoes it when supported and otherwise
/// responds with its newest supported revision.
fn negotiate_protocol_version(requested: ProtocolVersion) -> ProtocolVersion {
    if ProtocolVersion::known_up_to(&MAX_PROTOCOL_VERSION).contains(&requested) {
        requested
    } else {
        MAX_PROTOCOL_VERSION.clone()
    }
}

fn server_capabilities() -> ServerCapabilities {
    let mut capabilities = ServerCapabilities::default();
    capabilities.tools = Some(ToolsCapability::default());
    capabilities
}

fn call_tool_result(result: Result<ToolResult, BackendError>) -> CallToolResult {
    let mut result = match result {
        Ok(result) if result.success => {
            CallToolResult::success(vec![ContentBlock::text(result.output)])
        }
        Ok(result) => CallToolResult::error(vec![ContentBlock::text(result.output)]),
        Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
    };
    result.result_type = None;
    result
}

fn tool_input_schema(
    schema: &PhenixSchema,
) -> Result<Arc<JsonObject>, agent_client_protocol::Error> {
    let value = json_schema(schema)
        .map_err(|error| agent_client_protocol::Error::internal_error().data(error.to_string()))?;
    let object = value.as_object().cloned().ok_or_else(|| {
        agent_client_protocol::Error::internal_error()
            .data("Phenix callable schema did not project to a JSON object")
    })?;
    Ok(Arc::new(object))
}

fn parse_params<T: DeserializeOwned>(
    params: Option<&Map<String, Value>>,
) -> Result<T, agent_client_protocol::Error> {
    let value = params.cloned().map(Value::Object).unwrap_or(Value::Null);
    serde_json::from_value(value)
        .map_err(|error| agent_client_protocol::Error::invalid_params().data(error.to_string()))
}

fn to_value<T: Serialize>(value: &T) -> Result<Value, agent_client_protocol::Error> {
    serde_json::to_value(value).map_err(agent_client_protocol::Error::into_internal_error)
}

fn acp_error(error: ErrorData) -> agent_client_protocol::Error {
    let mut acp = agent_client_protocol::Error::new(error.code.0, error.message.into_owned());
    if let Some(data) = error.data {
        acp = acp.data(data);
    }
    acp
}

fn json_schema(schema: &PhenixSchema) -> Result<Value, BackendError> {
    let schema = match schema {
        PhenixSchema::Any => json!({}),
        PhenixSchema::Never => json!({"not": {}}),
        PhenixSchema::Unit => json!({"type": "null"}),
        PhenixSchema::Bool => json!({"type": "boolean"}),
        PhenixSchema::I64 => json!({"type": "integer"}),
        PhenixSchema::U64 => json!({"type": "integer", "minimum": 0}),
        PhenixSchema::F64 => json!({"type": "number"}),
        PhenixSchema::String => json!({"type": "string"}),
        PhenixSchema::Bytes => json!({"type": "string", "contentEncoding": "base64"}),
        PhenixSchema::Option(item) => {
            json!({"anyOf": [json_schema(item)?, {"type": "null"}]})
        }
        PhenixSchema::Array { item, len } => json!({
            "type": "array",
            "items": json_schema(item)?,
            "minItems": len,
            "maxItems": len,
        }),
        PhenixSchema::List(item) => {
            json!({"type": "array", "items": json_schema(item)?})
        }
        PhenixSchema::Map(item) => {
            json!({"type": "object", "additionalProperties": json_schema(item)?})
        }
        PhenixSchema::Table(fields) => {
            let properties = fields
                .iter()
                .map(|(key, schema)| Ok((key.as_str().to_owned(), json_schema(schema)?)))
                .collect::<Result<Map<String, Value>, BackendError>>()?;
            let required = fields
                .keys()
                .map(|key| key.as_str().to_owned())
                .collect::<Vec<_>>();
            json!({
                "type": "object",
                "properties": properties,
                "required": required,
                "additionalProperties": false,
            })
        }
        PhenixSchema::Variant(_) | PhenixSchema::Callable { .. } | PhenixSchema::Object { .. } => {
            return Err(BackendError::Unsupported(
                "Phenix callable schema cannot be represented as JSON Schema".to_owned(),
            ));
        }
    };
    Ok(schema)
}

fn raw_value(value: Value) -> Result<Arc<RawValue>, agent_client_protocol::Error> {
    RawValue::from_string(value.to_string())
        .map(Arc::from)
        .map_err(agent_client_protocol::Error::into_internal_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_backend::{BackendCapabilities, ToolProvision};
    use phenix_domain::{CallableId, CallableKind, CallablePolicy, CapabilitySet};
    use std::collections::BTreeSet;

    fn callable() -> CallableDescriptor {
        CallableDescriptor {
            id: CallableId::parse("phenix.echo").unwrap(),
            kind: CallableKind::Agent,
            description: "Echo a value".to_owned(),
            input_schema: PhenixSchema::Table(BTreeMap::from([(
                "value".parse().unwrap(),
                PhenixSchema::String,
            )])),
            output_schema: PhenixSchema::String,
            capabilities: CapabilitySet::default(),
            policy: CallablePolicy::default(),
        }
    }

    fn surface() -> PreparedToolSurface {
        ToolProvision {
            callables: vec![callable()],
        }
        .prepare(&BackendCapabilities {
            tool_presentations: BTreeSet::from([ToolPresentation::AcpExtension]),
            images: false,
            persistent_sessions: false,
        })
        .unwrap()
    }

    fn params(value: Value) -> Map<String, Value> {
        value
            .as_object()
            .cloned()
            .expect("fixture params are objects")
    }

    fn initialize_params(version: &str) -> Map<String, Value> {
        params(json!({
            "protocolVersion": version,
            "capabilities": {},
            "clientInfo": { "name": "fixture", "version": "0.1.0" },
        }))
    }

    fn response(value: MessageMcpResponse) -> Value {
        serde_json::from_str(value.0.get()).expect("fixture MCP response is valid JSON")
    }

    fn initialize_response(bridge: &ToolBridge, version: &str) -> Value {
        let connection = bridge
            .connect(ConnectMcpRequest::new(SERVER_ID))
            .unwrap()
            .connection_id;
        response(
            bridge
                .message(
                    MessageMcpRequest::new(connection, "initialize")
                        .params(initialize_params(version)),
                )
                .unwrap(),
        )
    }

    fn initialize_connection(bridge: &ToolBridge) -> McpConnectionId {
        let connection = bridge
            .connect(ConnectMcpRequest::new(SERVER_ID))
            .unwrap()
            .connection_id;
        bridge
            .message(
                MessageMcpRequest::new(connection.clone(), "initialize")
                    .params(initialize_params("2025-11-25")),
            )
            .unwrap();
        bridge
            .notification(MessageMcpNotification::new(
                connection.clone(),
                "notifications/initialized",
            ))
            .unwrap();
        connection
    }

    #[test]
    fn server_declaration_uses_native_acp_transport() {
        assert!(matches!(ToolBridge::default().server(), McpServer::Acp(_)));
    }

    #[test]
    fn mcp_message_params_omit_id_because_acp_outer_envelope_owns_it() {
        let request = MessageMcpRequest::new("connection", "tools/call")
            .params(params(json!({"name": "phenix.echo", "arguments": {}})));
        let serialized = serde_json::to_value(request).unwrap();

        assert_eq!(serialized["method"], "tools/call");
        assert!(serialized.get("id").is_none());
    }

    #[test]
    fn invalid_outer_request_ids_are_rejected() {
        let bridge = ToolBridge::default();
        let connection = initialize_connection(&bridge);
        for invalid in [Value::Null, json!({"not": "an id"}), json!(1.5)] {
            let error = bridge
                .message_with_request_id(
                    MessageMcpRequest::new(connection.clone(), "ping"),
                    invalid,
                )
                .unwrap_err();
            assert!(error.to_string().contains("request id"));
        }
    }

    #[test]
    fn initialize_negotiates_only_supported_protocol_versions() {
        let bridge = ToolBridge::default();
        bridge.provision(&surface()).unwrap();

        assert_eq!(
            initialize_response(&bridge, "2025-03-26")["protocolVersion"],
            "2025-03-26"
        );
        assert_eq!(
            initialize_response(&bridge, "2025-11-25")["protocolVersion"],
            "2025-11-25"
        );
        assert_eq!(
            initialize_response(&bridge, "2025-01-01")["protocolVersion"],
            ProtocolVersion::LATEST.as_str()
        );
        assert_eq!(
            initialize_response(&bridge, "2026-07-28")["protocolVersion"],
            ProtocolVersion::LATEST.as_str()
        );
        assert_eq!(
            initialize_response(&bridge, "2099-01-01")["protocolVersion"],
            ProtocolVersion::LATEST.as_str()
        );
    }

    #[test]
    fn requests_before_initialization_are_rejected() {
        let bridge = ToolBridge::default();
        bridge.provision(&surface()).unwrap();
        let connection = bridge
            .connect(ConnectMcpRequest::new(SERVER_ID))
            .unwrap()
            .connection_id;

        let error = bridge
            .message(MessageMcpRequest::new(connection, "tools/list"))
            .unwrap_err();
        assert!(error.to_string().contains("before initialization"));
    }

    #[test]
    fn cancellation_before_initialization_is_ignored() {
        let bridge = ToolBridge::default();
        let connection = bridge
            .connect(ConnectMcpRequest::new(SERVER_ID))
            .unwrap()
            .connection_id;
        bridge
            .notification(
                MessageMcpNotification::new(connection, "notifications/cancelled")
                    .params(params(json!({"requestId": 1}))),
            )
            .unwrap();
    }

    #[test]
    fn initialized_notification_requires_initialize_response() {
        let bridge = ToolBridge::default();
        let connection = bridge
            .connect(ConnectMcpRequest::new(SERVER_ID))
            .unwrap()
            .connection_id;

        let error = bridge
            .notification(MessageMcpNotification::new(
                connection.clone(),
                "notifications/initialized",
            ))
            .unwrap_err();
        assert!(error.to_string().contains("out of order"));

        let error = bridge
            .message(MessageMcpRequest::new(connection, "tools/list"))
            .unwrap_err();
        assert!(error.to_string().contains("before initialization"));
    }

    #[test]
    fn unknown_notifications_are_ignored() {
        let bridge = ToolBridge::default();
        bridge
            .notification(MessageMcpNotification::new(
                "missing-connection",
                "notifications/cancelled",
            ))
            .unwrap();

        let connection = initialize_connection(&bridge);
        bridge
            .notification(MessageMcpNotification::new(
                connection,
                "notifications/vendor-extension",
            ))
            .unwrap();
    }

    #[test]
    fn malformed_cancellation_is_ignored() {
        let bridge = ToolBridge::default();
        let connection = initialize_connection(&bridge);
        bridge
            .notification(
                MessageMcpNotification::new(connection, "notifications/cancelled")
                    .params(params(json!({"requestId": {"invalid": true}}))),
            )
            .unwrap();
    }

    #[test]
    fn stale_cancellation_does_not_cancel_a_newer_request() {
        let bridge = ToolBridge::default();
        let connection = initialize_connection(&bridge);
        let cancellation = ToolCancellation::new();
        {
            let mut state = bridge.state.lock();
            state
                .connections
                .get_mut(connection.0.as_ref())
                .unwrap()
                .in_flight = Some(InFlightCall {
                request_id: json!("second"),
                cancellation: cancellation.clone(),
            });
        }

        bridge
            .notification(
                MessageMcpNotification::new(connection.clone(), "notifications/cancelled")
                    .params(params(json!({"requestId": "first"}))),
            )
            .unwrap();
        assert!(!cancellation.is_cancelled());

        bridge
            .notification(
                MessageMcpNotification::new(connection, "notifications/cancelled")
                    .params(params(json!({"requestId": "second"}))),
            )
            .unwrap();
        assert!(cancellation.is_cancelled());
    }

    #[test]
    fn list_tools_adapts_structural_schema_at_the_mcp_boundary() {
        let bridge = ToolBridge::default();
        bridge.provision(&surface()).unwrap();
        let connection = initialize_connection(&bridge);

        let listed = bridge
            .message(MessageMcpRequest::new(connection, "tools/list"))
            .unwrap();
        let listed = response(listed);
        assert_eq!(listed["tools"][0]["name"], "phenix.echo");
        assert_eq!(listed["tools"][0]["inputSchema"]["type"], "object");
        assert_eq!(
            listed["tools"][0]["inputSchema"]["properties"]["value"]["type"],
            "string"
        );
    }

    #[test]
    fn disconnect_removes_connection_and_later_messages_fail() {
        let bridge = ToolBridge::default();
        let connection = bridge
            .connect(ConnectMcpRequest::new(SERVER_ID))
            .unwrap()
            .connection_id;
        bridge
            .disconnect(DisconnectMcpRequest::new(connection.clone()))
            .unwrap();
        let error = bridge
            .message(MessageMcpRequest::new(connection, "ping"))
            .unwrap_err();
        assert!(error.to_string().contains("unknown Phenix MCP connection"));
    }

    #[test]
    fn cancellation_wins_when_worker_result_channel_closes() {
        let cancellation = ToolCancellation::new();
        cancellation.cancel();
        let (response_tx, response_rx) = mpsc::sync_channel(1);
        drop(response_tx);

        let error = match wait_for_tool_result(&response_rx, &cancellation) {
            Ok(_) => panic!("cancelled call unexpectedly returned a worker result"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("cancelled"));
    }

    #[test]
    fn cancellation_wins_over_buffered_worker_success() {
        let cancellation = ToolCancellation::new();
        let (response_tx, response_rx) = mpsc::sync_channel(1);
        response_tx
            .send(Ok(ToolResult {
                output: "late-success".to_owned(),
                success: true,
            }))
            .unwrap();
        cancellation.cancel();

        let error = match wait_for_tool_result(&response_rx, &cancellation) {
            Ok(_) => panic!("cancelled call unexpectedly returned a late success"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("cancelled"));
    }

    #[test]
    fn tools_call_reaches_worker_and_returns_typed_result() {
        let bridge = ToolBridge::default();
        let (worker_tx, worker_rx) = mpsc::channel::<WorkerMessage>();
        bridge.bind_execution(&surface(), worker_tx).unwrap();
        let connection = initialize_connection(&bridge);

        let worker = std::thread::spawn(move || {
            let WorkerMessage::ToolCall(request) = worker_rx.recv().unwrap() else {
                panic!("expected a tool call");
            };
            assert_eq!(request.invocation.callable.as_str(), "phenix.echo");
            assert_eq!(request.invocation.arguments_json, r#"{"value":"from-acp"}"#);
            assert!(!request.invocation.cancellation.is_cancelled());
            request
                .response
                .send(Ok(ToolResult {
                    output: "echo:from-acp".to_owned(),
                    success: true,
                }))
                .unwrap();
        });

        let called = bridge
            .message_with_request_id(
                MessageMcpRequest::new(connection, "tools/call").params(params(
                    json!({"name": "phenix.echo", "arguments": {"value": "from-acp"}}),
                )),
                json!(17),
            )
            .unwrap();
        let called = response(called);
        assert_eq!(called["isError"], false);
        assert_eq!(called["content"][0]["type"], "text");
        assert_eq!(called["content"][0]["text"], "echo:from-acp");
        worker.join().unwrap();
    }

    #[test]
    fn cancellation_reaches_execution_and_rejects_overlapping_calls() {
        let bridge = ToolBridge::default();
        let (worker_tx, worker_rx) = mpsc::channel::<WorkerMessage>();
        bridge.bind_execution(&surface(), worker_tx).unwrap();
        let connection = initialize_connection(&bridge);

        let (started_tx, started_rx) = mpsc::channel();
        let (observed_tx, observed_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let WorkerMessage::ToolCall(request) = worker_rx.recv().unwrap() else {
                panic!("expected a tool call");
            };
            started_tx.send(()).unwrap();
            while !request.invocation.cancellation.is_cancelled() {
                std::thread::yield_now();
            }
            observed_tx
                .send(request.invocation.cancellation.is_cancelled())
                .unwrap();
        });

        let caller = {
            let bridge = bridge.clone();
            let connection = connection.clone();
            std::thread::spawn(move || {
                bridge.message_with_request_id(
                    MessageMcpRequest::new(connection, "tools/call")
                        .params(params(json!({"name": "phenix.echo", "arguments": {}}))),
                    json!(1),
                )
            })
        };

        started_rx.recv().unwrap();
        let overlapping = bridge
            .message_with_request_id(
                MessageMcpRequest::new(connection.clone(), "tools/call")
                    .params(params(json!({"name": "phenix.echo", "arguments": {}}))),
                json!(2),
            )
            .unwrap_err();
        assert!(overlapping
            .to_string()
            .contains("already has an in-flight tool call"));

        bridge
            .notification(
                MessageMcpNotification::new(connection, "notifications/cancelled")
                    .params(params(json!({"requestId": 1}))),
            )
            .unwrap();

        let called = caller.join().unwrap().unwrap();
        let called = response(called);
        assert_eq!(called["isError"], true);
        assert!(called["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("cancelled"));
        assert!(observed_rx.recv().unwrap());
        worker.join().unwrap();
    }
}
