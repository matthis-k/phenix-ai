use super::WorkerMessage;
use agent_client_protocol::schema::v1::{
    ConnectMcpRequest, ConnectMcpResponse, DisconnectMcpRequest, DisconnectMcpResponse,
    McpConnectionId, McpServer, McpServerAcp, MessageMcpNotification, MessageMcpRequest,
    MessageMcpResponse,
};
use phenix_domain::{CallableDescriptor, PhenixSchema};
use phenix_model_adapter::{
    ModelAdapterError, PreparedToolSurface, ToolInvocation, ToolPresentation, ToolResult,
};
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResult, ContentBlock, DiscoverResult,
    Implementation, ListToolsResult, ProtocolVersion, RequestMetaObject, ServerCapabilities, Tool,
};
use serde_json::{Map, Value, json, value::RawValue};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, mpsc};

const SERVER_ID: &str = "phenix-tools";
const SERVER_NAME: &str = "Phenix tools";

#[derive(Clone, Default)]
pub(super) struct ToolBridge {
    state: Arc<Mutex<ToolBridgeState>>,
}

#[derive(Default)]
struct ToolBridgeState {
    callables: BTreeMap<String, CallableDescriptor>,
    worker: Option<mpsc::Sender<WorkerMessage>>,
    connections: BTreeMap<String, Option<ProtocolVersion>>,
    next_connection: u64,
}

impl ToolBridge {
    pub(super) fn server(&self) -> McpServer {
        McpServer::Acp(McpServerAcp::new(SERVER_NAME, SERVER_ID))
    }

    pub(super) fn provision(&self, tools: &PreparedToolSurface) -> Result<(), ModelAdapterError> {
        if !tools.is_empty() && tools.presentation() != Some(ToolPresentation::AcpExtension) {
            return Err(ModelAdapterError::Unsupported(
                "ACP tool bridge requires the negotiated ACP extension presentation".to_owned(),
            ));
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| ModelAdapterError::Protocol("ACP tool bridge lock poisoned".to_owned()))?;
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
    ) -> Result<(), ModelAdapterError> {
        self.provision(tools)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| ModelAdapterError::Protocol("ACP tool bridge lock poisoned".to_owned()))?;
        state.worker = Some(worker);
        Ok(())
    }

    pub(super) fn unbind_execution(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.worker = None;
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
        let mut state = self.state.lock().map_err(|_| {
            agent_client_protocol::Error::internal_error().data("ACP tool bridge lock poisoned")
        })?;
        state.next_connection += 1;
        let connection_id = format!("phenix-tools-{}", state.next_connection);
        state.connections.insert(connection_id.clone(), None);
        Ok(ConnectMcpResponse::new(connection_id))
    }

    pub(super) fn disconnect(
        &self,
        request: DisconnectMcpRequest,
    ) -> Result<DisconnectMcpResponse, agent_client_protocol::Error> {
        let mut state = self.state.lock().map_err(|_| {
            agent_client_protocol::Error::internal_error().data("ACP tool bridge lock poisoned")
        })?;
        state.connections.remove(request.connection_id.0.as_ref());
        Ok(DisconnectMcpResponse::new())
    }

    pub(super) fn message(
        &self,
        request: MessageMcpRequest,
    ) -> Result<MessageMcpResponse, agent_client_protocol::Error> {
        self.require_connection(&request.connection_id)?;
        let result = match request.method.as_str() {
            "server/discover" => discover_result()?,
            "initialize" => self.initialize(&request.connection_id, request.params.as_ref())?,
            "ping" => {
                self.request_protocol_version(&request.connection_id, request.params.as_ref())?;
                json!({})
            }
            "tools/list" => {
                let version =
                    self.request_protocol_version(&request.connection_id, request.params.as_ref())?;
                self.list_tools(&version)?
            }
            "tools/call" => {
                let version =
                    self.request_protocol_version(&request.connection_id, request.params.as_ref())?;
                self.call_tool(request.params.as_ref(), &version)?
            }
            method => {
                return Err(agent_client_protocol::Error::method_not_found()
                    .data(format!("unsupported Phenix MCP method {method}")));
            }
        };
        Ok(MessageMcpResponse::new(raw_value(result)?))
    }

    pub(super) fn notification(
        &self,
        notification: MessageMcpNotification,
    ) -> Result<(), agent_client_protocol::Error> {
        self.require_connection(&notification.connection_id)?;
        match notification.method.as_str() {
            "notifications/initialized" | "notifications/cancelled" => Ok(()),
            method => Err(agent_client_protocol::Error::method_not_found()
                .data(format!("unsupported Phenix MCP notification {method}"))),
        }
    }

    fn initialize(
        &self,
        connection_id: &McpConnectionId,
        params: Option<&Map<String, Value>>,
    ) -> Result<Value, agent_client_protocol::Error> {
        let requested = params
            .and_then(|params| params.get("protocolVersion"))
            .cloned()
            .map(serde_json::from_value::<ProtocolVersion>)
            .transpose()
            .map_err(|error| {
                agent_client_protocol::Error::invalid_params()
                    .data(format!("invalid MCP protocolVersion: {error}"))
            })?
            .unwrap_or(ProtocolVersion::V_2025_06_18);
        let selected = if requested.as_str() < ProtocolVersion::V_2026_07_28.as_str()
            && supports_protocol(&requested)
        {
            requested
        } else {
            ProtocolVersion::V_2025_11_25
        };
        self.set_connection_protocol(connection_id, selected.clone())?;

        Ok(json!({
            "protocolVersion": selected,
            "features": server_capabilities(),
            "serverInfo": server_implementation(),
        }))
    }

    fn require_connection(
        &self,
        connection_id: &McpConnectionId,
    ) -> Result<(), agent_client_protocol::Error> {
        let state = self.state.lock().map_err(|_| {
            agent_client_protocol::Error::internal_error().data("ACP tool bridge lock poisoned")
        })?;
        if state.connections.contains_key(connection_id.0.as_ref()) {
            Ok(())
        } else {
            Err(agent_client_protocol::Error::invalid_params()
                .data(format!("unknown Phenix MCP connection {connection_id}")))
        }
    }

    fn set_connection_protocol(
        &self,
        connection_id: &McpConnectionId,
        version: ProtocolVersion,
    ) -> Result<(), agent_client_protocol::Error> {
        let mut state = self.state.lock().map_err(|_| {
            agent_client_protocol::Error::internal_error().data("ACP tool bridge lock poisoned")
        })?;
        let selected = state
            .connections
            .get_mut(connection_id.0.as_ref())
            .ok_or_else(|| {
                agent_client_protocol::Error::invalid_params()
                    .data(format!("unknown Phenix MCP connection {connection_id}"))
            })?;
        *selected = Some(version);
        Ok(())
    }

    fn connection_protocol(
        &self,
        connection_id: &McpConnectionId,
    ) -> Result<Option<ProtocolVersion>, agent_client_protocol::Error> {
        let state = self.state.lock().map_err(|_| {
            agent_client_protocol::Error::internal_error().data("ACP tool bridge lock poisoned")
        })?;
        state
            .connections
            .get(connection_id.0.as_ref())
            .cloned()
            .ok_or_else(|| {
                agent_client_protocol::Error::invalid_params()
                    .data(format!("unknown Phenix MCP connection {connection_id}"))
            })
    }

    fn request_protocol_version(
        &self,
        connection_id: &McpConnectionId,
        params: Option<&Map<String, Value>>,
    ) -> Result<ProtocolVersion, agent_client_protocol::Error> {
        let legacy = self.connection_protocol(connection_id)?;
        let Some(meta) = params.and_then(|params| params.get("_meta")) else {
            return Ok(legacy.unwrap_or(ProtocolVersion::V_2025_06_18));
        };
        let meta = serde_json::from_value::<RequestMetaObject>(meta.clone()).map_err(|error| {
            agent_client_protocol::Error::invalid_params()
                .data(format!("invalid MCP request _meta: {error}"))
        })?;
        let Some(version) = meta.protocol_version() else {
            return Ok(legacy.unwrap_or(ProtocolVersion::V_2025_06_18));
        };
        if !supports_protocol(&version) {
            return Err(agent_client_protocol::Error::invalid_params()
                .data(format!("unsupported MCP protocol version {version}")));
        }
        let missing = meta.missing_required_keys(&version);
        if !missing.is_empty() {
            return Err(agent_client_protocol::Error::invalid_params().data(format!(
                "MCP {version} request is missing required _meta keys: {}",
                missing.join(", ")
            )));
        }
        Ok(version)
    }

    fn list_tools(&self, version: &ProtocolVersion) -> Result<Value, agent_client_protocol::Error> {
        let state = self.state.lock().map_err(|_| {
            agent_client_protocol::Error::internal_error().data("ACP tool bridge lock poisoned")
        })?;
        let tools = state
            .callables
            .values()
            .map(|callable| {
                let input_schema = json_schema_object(&callable.input_schema).map_err(|error| {
                    agent_client_protocol::Error::internal_error().data(error.to_string())
                })?;
                Ok(Tool::new(
                    callable.id.as_str().to_owned(),
                    callable.description.clone(),
                    input_schema,
                ))
            })
            .collect::<Result<Vec<_>, agent_client_protocol::Error>>()?;
        let mut result = ListToolsResult::with_all_items(tools);
        if is_current_protocol(version) {
            result = result.with_ttl_ms(0).with_cache_scope(CacheScope::Private);
        } else {
            result.result_type = None;
        }
        serde_json::to_value(result).map_err(agent_client_protocol::Error::into_internal_error)
    }

    fn call_tool(
        &self,
        params: Option<&Map<String, Value>>,
        version: &ProtocolVersion,
    ) -> Result<Value, agent_client_protocol::Error> {
        let request = serde_json::from_value::<CallToolRequestParams>(Value::Object(
            params.cloned().unwrap_or_default(),
        ))
        .map_err(|error| {
            agent_client_protocol::Error::invalid_params()
                .data(format!("invalid MCP tools/call params: {error}"))
        })?;
        if request.input_responses.is_some() || request.request_state.is_some() {
            return Err(agent_client_protocol::Error::invalid_params()
                .data("Phenix MCP tools do not support MRTR retries"));
        }
        let arguments = request.arguments.unwrap_or_default();

        let (callable, input_schema, worker) = {
            let state = self.state.lock().map_err(|_| {
                agent_client_protocol::Error::internal_error().data("ACP tool bridge lock poisoned")
            })?;
            let callable = state.callables.get(request.name.as_ref()).ok_or_else(|| {
                agent_client_protocol::Error::invalid_params().data(format!(
                    "tool is not provisioned for this execution: {}",
                    request.name
                ))
            })?;
            let worker = state.worker.clone().ok_or_else(|| {
                agent_client_protocol::Error::internal_error()
                    .data("ACP tool call arrived outside an active execution")
            })?;
            (
                callable.id.clone(),
                callable.input_schema.clone(),
                worker,
            )
        };
        let arguments = model_tool_arguments(&input_schema, Value::Object(arguments)).map_err(
            |error| agent_client_protocol::Error::invalid_params().data(error.to_string()),
        )?;

        let (response_tx, response_rx) = mpsc::sync_channel(1);
        worker
            .send(WorkerMessage::ToolCall(BridgeToolRequest {
                invocation: ToolInvocation {
                    callable,
                    arguments_json: serde_json::to_string(&arguments)
                        .map_err(agent_client_protocol::Error::into_internal_error)?,
                },
                response: response_tx,
            }))
            .map_err(|error| {
                agent_client_protocol::Error::internal_error()
                    .data(format!("runtime tool host is unavailable: {error}"))
            })?;
        let result = response_rx.recv().map_err(|error| {
            agent_client_protocol::Error::internal_error()
                .data(format!("runtime tool result channel closed: {error}"))
        })?;
        serialize_tool_result(result, version)
    }
}

#[derive(Debug)]
pub(super) struct BridgeToolRequest {
    pub(super) invocation: ToolInvocation,
    pub(super) response: mpsc::SyncSender<Result<ToolResult, ModelAdapterError>>,
}

fn server_capabilities() -> ServerCapabilities {
    let mut capabilities = ServerCapabilities::default();
    capabilities.tools = Some(Default::default());
    capabilities
}

fn server_implementation() -> Implementation {
    Implementation::new("phenix-runtime", env!("CARGO_PKG_VERSION"))
}

fn discover_result() -> Result<Value, agent_client_protocol::Error> {
    let result = DiscoverResult::new(
        ProtocolVersion::KNOWN_VERSIONS.to_vec(),
        server_capabilities(),
    )
    .with_server_info(server_implementation());
    serde_json::to_value(result).map_err(agent_client_protocol::Error::into_internal_error)
}

fn supports_protocol(version: &ProtocolVersion) -> bool {
    ProtocolVersion::KNOWN_VERSIONS
        .iter()
        .any(|candidate| candidate == version)
}

fn is_current_protocol(version: &ProtocolVersion) -> bool {
    version.as_str() >= ProtocolVersion::V_2026_07_28.as_str()
}

fn json_schema_object(schema: &PhenixSchema) -> Result<Map<String, Value>, ModelAdapterError> {
    let value = model_tool_json_schema(schema)?;
    let Value::Object(object) = value else {
        return Err(ModelAdapterError::Protocol(
            "model tool JSON Schema must be an object".to_owned(),
        ));
    };
    Ok(object)
}

fn model_tool_json_schema(schema: &PhenixSchema) -> Result<Value, ModelAdapterError> {
    match schema {
        PhenixSchema::Unit => Ok(json!({
            "type": "object",
            "properties": {},
            "required": [],
            "additionalProperties": false,
        })),
        PhenixSchema::Map(_) | PhenixSchema::Table(_) => json_schema(schema),
        _ => Ok(json!({
            "type": "object",
            "properties": {
                "value": json_schema(schema)?,
            },
            "required": ["value"],
            "additionalProperties": false,
        })),
    }
}

fn model_tool_arguments(schema: &PhenixSchema, value: Value) -> Result<Value, ModelAdapterError> {
    let object = value.as_object().ok_or_else(|| {
        ModelAdapterError::Protocol("model tool arguments must be a JSON object".to_owned())
    })?;
    match schema {
        PhenixSchema::Unit => {
            if object.is_empty() {
                Ok(Value::Null)
            } else {
                Err(ModelAdapterError::Protocol(
                    "unit model tool arguments must be an empty object".to_owned(),
                ))
            }
        }
        PhenixSchema::Map(_) | PhenixSchema::Table(_) => Ok(value),
        _ => object.get("value").cloned().ok_or_else(|| {
            ModelAdapterError::Protocol(
                "model tool arguments are missing the required value field".to_owned(),
            )
        }),
    }
}

fn json_schema(schema: &PhenixSchema) -> Result<Value, ModelAdapterError> {
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
                .collect::<Result<Map<String, Value>, ModelAdapterError>>()?;
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
            return Err(ModelAdapterError::Unsupported(
                "Phenix callable schema cannot be represented as JSON Schema".to_owned(),
            ));
        }
    };
    Ok(schema)
}

fn serialize_tool_result(
    result: Result<ToolResult, ModelAdapterError>,
    version: &ProtocolVersion,
) -> Result<Value, agent_client_protocol::Error> {
    let mut result = match result {
        Ok(result) if result.success => {
            CallToolResult::success(vec![ContentBlock::text(result.output)])
        }
        Ok(result) => CallToolResult::error(vec![ContentBlock::text(result.output)]),
        Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
    };
    if !is_current_protocol(version) {
        result.result_type = None;
    }
    serde_json::to_value(result).map_err(agent_client_protocol::Error::into_internal_error)
}

fn raw_value(value: Value) -> Result<Arc<RawValue>, agent_client_protocol::Error> {
    RawValue::from_string(value.to_string())
        .map(Arc::from)
        .map_err(agent_client_protocol::Error::into_internal_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_domain::{
        CallableFeatureSet, CallableId, CallableKind, CallablePolicy, PhenixSchema,
    };
    use phenix_model_adapter::{ModelAdapterFeatures, ToolProvision};
    use std::collections::BTreeSet;

    fn callable_with_schema(input_schema: PhenixSchema) -> CallableDescriptor {
        CallableDescriptor {
            id: CallableId::parse("phenix.echo").unwrap(),
            kind: CallableKind::Agent,
            description: "Echo a value".to_owned(),
            input_schema,
            output_schema: PhenixSchema::String,
            features: CallableFeatureSet::default(),
            policy: CallablePolicy::default(),
        }
    }

    fn callable() -> CallableDescriptor {
        callable_with_schema(PhenixSchema::Table(BTreeMap::from([(
            "value".parse().unwrap(),
            PhenixSchema::String,
        )])))
    }

    fn surface_with_schema(input_schema: PhenixSchema) -> PreparedToolSurface {
        ToolProvision {
            callables: vec![callable_with_schema(input_schema)],
        }
        .prepare(&ModelAdapterFeatures {
            tool_presentations: BTreeSet::from([ToolPresentation::AcpExtension]),
            images: false,
            persistent_sessions: false,
        })
        .unwrap()
    }

    fn surface() -> PreparedToolSurface {
        ToolProvision {
            callables: vec![callable()],
        }
        .prepare(&ModelAdapterFeatures {
            tool_presentations: BTreeSet::from([ToolPresentation::AcpExtension]),
            images: false,
            persistent_sessions: false,
        })
        .unwrap()
    }

    #[test]
    fn server_declaration_uses_native_acp_transport() {
        assert!(matches!(ToolBridge::default().server(), McpServer::Acp(_)));
    }

    #[test]
    fn discovery_advertises_current_and_legacy_mcp_versions() {
        let discovered = discover_result().unwrap();
        assert_eq!(discovered["resultType"], "complete");
        assert!(
            discovered["supportedVersions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|version| version == "2026-07-28")
        );
        assert_eq!(discovered["capabilities"]["tools"], json!({}));
        assert_eq!(discovered["cacheScope"], "private");
        assert_eq!(discovered["ttlMs"], 0);
    }

    #[test]
    fn list_tools_adapts_structural_schema_at_the_mcp_boundary() {
        let bridge = ToolBridge::default();
        bridge.provision(&surface()).unwrap();
        let listed = bridge.list_tools(&ProtocolVersion::V_2026_07_28).unwrap();
        assert_eq!(listed["resultType"], "complete");
        assert_eq!(listed["ttlMs"], 0);
        assert_eq!(listed["cacheScope"], "private");
        assert_eq!(listed["tools"][0]["name"], "phenix.echo");
        assert_eq!(listed["tools"][0]["inputSchema"]["type"], "object");
        assert_eq!(
            listed["tools"][0]["inputSchema"]["properties"]["value"]["type"],
            "string"
        );
    }

    #[test]
    fn unit_tool_list_uses_empty_object_schema_and_projects_empty_arguments_to_null() {
        let bridge = ToolBridge::default();
        bridge
            .provision(&surface_with_schema(PhenixSchema::Unit))
            .unwrap();
        let listed = bridge.list_tools(&ProtocolVersion::V_2026_07_28).unwrap();
        let schema = &listed["tools"][0]["inputSchema"];
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"], json!({}));
        assert_eq!(schema["required"], json!([]));
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(
            model_tool_arguments(&PhenixSchema::Unit, json!({})).unwrap(),
            Value::Null
        );
    }

    #[test]
    fn scalar_tool_list_uses_value_envelope_and_unwraps_before_dispatch() {
        let bridge = ToolBridge::default();
        bridge
            .provision(&surface_with_schema(PhenixSchema::U64))
            .unwrap();
        let listed = bridge.list_tools(&ProtocolVersion::V_2026_07_28).unwrap();
        let schema = &listed["tools"][0]["inputSchema"];
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["value"]["type"], "integer");
        assert_eq!(
            model_tool_arguments(&PhenixSchema::U64, json!({"value": 7})).unwrap(),
            json!(7)
        );
    }

    #[test]
    fn every_supported_model_tool_schema_has_an_object_root() {
        let schemas = vec![
            PhenixSchema::Any,
            PhenixSchema::Never,
            PhenixSchema::Unit,
            PhenixSchema::Bool,
            PhenixSchema::I64,
            PhenixSchema::U64,
            PhenixSchema::F64,
            PhenixSchema::String,
            PhenixSchema::Bytes,
            PhenixSchema::Option(Box::new(PhenixSchema::String)),
            PhenixSchema::Array {
                item: Box::new(PhenixSchema::String),
                len: 1,
            },
            PhenixSchema::List(Box::new(PhenixSchema::String)),
            PhenixSchema::Map(Box::new(PhenixSchema::String)),
            PhenixSchema::Table(BTreeMap::new()),
        ];
        for schema in schemas {
            let json = model_tool_json_schema(&schema).unwrap();
            assert_eq!(json["type"], "object", "non-object root for {schema:?}");
        }
    }

    #[test]
    fn nested_unit_keeps_null_semantics() {
        let schema = model_tool_json_schema(&PhenixSchema::Table(BTreeMap::from([(
            "done".parse().unwrap(),
            PhenixSchema::Unit,
        )])))
        .unwrap();
        assert_eq!(schema["properties"]["done"]["type"], "null");
    }

    #[test]
    fn unit_tool_call_projects_empty_object_back_to_canonical_unit_json() {
        let bridge = ToolBridge::default();
        let surface = surface_with_schema(PhenixSchema::Unit);
        let (worker, receiver) = mpsc::channel();
        bridge.bind_execution(&surface, worker).unwrap();

        let worker = std::thread::spawn(move || {
            let WorkerMessage::ToolCall(request) = receiver.recv().unwrap() else {
                panic!("expected ACP tool call");
            };
            assert_eq!(request.invocation.callable.as_str(), "phenix.echo");
            assert_eq!(
                request.invocation.arguments_json,
                "null",
                "unit input must reach the runtime as canonical JSON null"
            );
            request
                .response
                .send(Ok(ToolResult {
                    output: "ok".to_owned(),
                    success: true,
                }))
                .unwrap();
        });

        let params = json!({
            "name": "phenix.echo",
            "arguments": {}
        });
        let params = params.as_object().unwrap();
        bridge
            .call_tool(Some(params), &ProtocolVersion::V_2026_07_28)
            .unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn legacy_tool_list_keeps_legacy_wire_shape() {
        let bridge = ToolBridge::default();
        bridge.provision(&surface()).unwrap();
        let listed = bridge.list_tools(&ProtocolVersion::V_2025_06_18).unwrap();
        assert!(listed.get("resultType").is_none());
        assert!(listed.get("ttlMs").is_none());
        assert!(listed.get("cacheScope").is_none());
    }

    #[test]
    fn tool_results_add_result_type_only_for_current_protocol() {
        let legacy = serialize_tool_result(
            Ok(ToolResult {
                output: "ok".into(),
                success: true,
            }),
            &ProtocolVersion::V_2025_06_18,
        )
        .unwrap();
        assert!(legacy.get("resultType").is_none());

        let current = serialize_tool_result(
            Ok(ToolResult {
                output: "ok".into(),
                success: true,
            }),
            &ProtocolVersion::V_2026_07_28,
        )
        .unwrap();
        assert_eq!(current["resultType"], "complete");
    }
}
