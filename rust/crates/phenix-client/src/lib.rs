#![forbid(unsafe_code)]

use phenix_domain::{
    AuthenticationInput, AuthenticationMethodId, BackendCatalog, BackendId, CallableDescriptor,
    CallableId, ConfigRevisionId, ExecutionEvent, ExecutionId, ExecutionSummary, ExecutionTarget,
    RoutingProfileDescriptor, SessionId, SessionSummary, SkillDescriptor,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

/// Canonical frontend/service request accepted by the supported Harness process.
///
/// Authority and provider binding are intentionally absent. They are product
/// policy and cannot be supplied by an untrusted frontend request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceRequest {
    #[serde(default)]
    pub id: Value,
    pub service: String,
    #[serde(default)]
    pub input: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ServiceOutput {
    Json { output: Value },
    Bytes { output_bytes: Vec<u8> },
}

/// Canonical response produced by the supported Harness service wire.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ServiceResponse {
    Ok {
        id: Value,
        #[serde(flatten)]
        output: ServiceOutput,
    },
    Error {
        id: Value,
        error: String,
    },
}

impl ServiceResponse {
    #[must_use]
    pub fn json(id: Value, output: Value) -> Self {
        Self::Ok {
            id,
            output: ServiceOutput::Json { output },
        }
    }

    #[must_use]
    pub fn bytes(id: Value, output: Vec<u8>) -> Self {
        Self::Ok {
            id,
            output: ServiceOutput::Bytes {
                output_bytes: output,
            },
        }
    }

    #[must_use]
    pub fn error(id: Value, error: impl Into<String>) -> Self {
        Self::Error {
            id,
            error: error.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidFrontendServiceProviderId;

impl Display for InvalidFrontendServiceProviderId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("frontend service provider identifier must not be empty")
    }
}

impl std::error::Error for InvalidFrontendServiceProviderId {}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct FrontendServiceProviderId(String);

impl FrontendServiceProviderId {
    pub fn parse(value: impl Into<String>) -> Result<Self, InvalidFrontendServiceProviderId> {
        value.into().try_into()
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for FrontendServiceProviderId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Display for FrontendServiceProviderId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for FrontendServiceProviderId {
    type Err = InvalidFrontendServiceProviderId;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.to_owned().try_into()
    }
}

impl TryFrom<String> for FrontendServiceProviderId {
    type Error = InvalidFrontendServiceProviderId;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.trim().is_empty() {
            return Err(InvalidFrontendServiceProviderId);
        }
        Ok(Self(value))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FrontendServiceProviderDescriptor {
    pub id: FrontendServiceProviderId,
    #[serde(default)]
    pub capabilities: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrontendServiceRequest {
    pub provider: FrontendServiceProviderId,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrontendServiceNotification {
    pub provider: FrontendServiceProviderId,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrontendServiceError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum FrontendServiceResponsePayload {
    Ok { result: Value },
    Error { error: FrontendServiceError },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrontendServiceResponse {
    pub id: u64,
    #[serde(flatten)]
    pub response: FrontendServiceResponsePayload,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FrontendConnectionCommand {
    SetFrontendServiceProviders {
        id: u64,
        providers: Vec<FrontendServiceProviderDescriptor>,
    },
}

impl FrontendConnectionCommand {
    #[must_use]
    pub fn id(&self) -> u64 {
        match self {
            Self::SetFrontendServiceProviders { id, .. } => *id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FrontendConnectionEvent {
    FrontendServiceNotification {
        notification: FrontendServiceNotification,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSnapshot {
    pub sessions: Vec<SessionSummary>,
    pub executions: Vec<ExecutionSummary>,
    pub last_event_sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    Initialize {
        after_sequence: Option<u64>,
    },
    GetSnapshot,
    GetCallableCatalog,
    GetRoutingCatalog,
    GetSkillCatalog,
    ExportSessionDebug {
        session_id: SessionId,
    },
    RequestWorkspaceCheckpoint {
        execution_id: ExecutionId,
    },
    CreateSession {
        parent_session: Option<SessionId>,
        name: Option<String>,
        target: ExecutionTarget,
    },
    ForkSession {
        session_id: SessionId,
        name: Option<String>,
    },
    RenameSession {
        session_id: SessionId,
        name: String,
    },
    SetSessionTarget {
        session_id: SessionId,
        target: ExecutionTarget,
    },
    RebaseSession {
        session_id: SessionId,
        config_revision: ConfigRevisionId,
    },
    CloseSession {
        session_id: SessionId,
    },
    Submit {
        session_id: SessionId,
        text: String,
    },
    StartCallable {
        session_id: SessionId,
        callable: CallableId,
        input: Value,
    },
    CancelExecution {
        execution_id: ExecutionId,
    },
    RefreshBackendCatalog {
        backend_id: BackendId,
    },
    SelectAuthentication {
        backend_id: BackendId,
        method_id: AuthenticationMethodId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input: Option<AuthenticationInput>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ClientMessage {
    pub id: u64,
    pub command: Command,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ClientEnvelope {
    Command(ClientMessage),
    ConnectionCommand(FrontendConnectionCommand),
    ConnectionEvent(FrontendConnectionEvent),
    FrontendServiceResponse(FrontendServiceResponse),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Reply {
    Initialized {
        snapshot: RuntimeSnapshot,
        events: Vec<ExecutionEvent>,
        backends: Vec<BackendCatalog>,
    },
    Snapshot {
        snapshot: RuntimeSnapshot,
        backends: Vec<BackendCatalog>,
    },
    CallableCatalog {
        callables: Vec<CallableDescriptor>,
    },
    RoutingCatalog {
        profiles: Vec<RoutingProfileDescriptor>,
    },
    SkillCatalog {
        skills: Vec<SkillDescriptor>,
    },
    Session {
        session: SessionSummary,
    },
    Execution {
        execution: ExecutionSummary,
    },
    BackendCatalog {
        catalog: BackendCatalog,
    },
    SessionDebug {
        bundle: Box<phenix_domain::SessionDebugBundle>,
    },
    Accepted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    UnknownId,
    PolicyDenied,
    UnsupportedCapability,
    RoutingFailure,
    AuthenticationRequired,
    BackendTransport,
    BackendProtocol,
    ExecutionProviderFailure,
    ToolFailure,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: ErrorCode,
    pub message: String,
    pub session_id: Option<SessionId>,
    pub execution_id: Option<ExecutionId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ResponsePayload {
    Ok { result: Reply },
    Error { error: ProtocolError },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Response {
        id: u64,
        #[serde(flatten)]
        response: ResponsePayload,
    },
    Event {
        event: ExecutionEvent,
    },
    FrontendServiceRequest {
        id: u64,
        request: FrontendServiceRequest,
    },
    FrontendServiceNotification {
        notification: FrontendServiceNotification,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_domain::{ProviderId, RoutingProfileId};

    #[test]
    fn harness_service_wire_is_protocol_owned_and_policy_neutral() {
        let request: ServiceRequest = serde_json::from_value(serde_json::json!({
            "id": 7,
            "service": "phenix.sessions@1",
            "input": {"operation": "list"}
        }))
        .unwrap();
        assert_eq!(request.id, serde_json::json!(7));
        assert_eq!(request.service, "phenix.sessions@1");
        assert_eq!(request.input["operation"], "list");

        for forbidden in ["authority", "binding"] {
            let mut value = serde_json::json!({
                "id": 7,
                "service": "phenix.sessions@1",
                "input": null
            });
            value[forbidden] = serde_json::json!("ambient");
            assert!(serde_json::from_value::<ServiceRequest>(value).is_err());
        }
    }

    #[test]
    fn harness_service_response_preserves_json_and_byte_wire_shapes() {
        assert_eq!(
            serde_json::to_value(ServiceResponse::json(
                serde_json::json!(1),
                serde_json::json!({"result": "ok"})
            ))
            .unwrap(),
            serde_json::json!({"status": "ok", "id": 1, "output": {"result": "ok"}})
        );
        assert_eq!(
            serde_json::to_value(ServiceResponse::bytes(serde_json::json!(2), vec![1, 2, 3]))
                .unwrap(),
            serde_json::json!({"status": "ok", "id": 2, "output_bytes": [1, 2, 3]})
        );
        assert_eq!(
            serde_json::to_value(ServiceResponse::error(serde_json::json!(3), "denied")).unwrap(),
            serde_json::json!({"status": "error", "id": 3, "error": "denied"})
        );
    }

    #[test]
    fn protocol_has_explicit_request_ids() {
        let message = ClientMessage {
            id: 7,
            command: Command::GetSnapshot,
        };
        assert_eq!(message.id, 7);
    }

    #[test]
    fn response_wire_shape_is_protocol_owned() {
        let message = ServerMessage::Response {
            id: 7,
            response: ResponsePayload::Ok {
                result: Reply::Accepted,
            },
        };
        let value = serde_json::to_value(message).expect("serialize response");
        assert_eq!(value["type"], "response");
        assert_eq!(value["status"], "ok");
        assert_eq!(value["id"], 7);
        assert_eq!(value["result"]["type"], "accepted");
        assert!(value.get("Ok").is_none());
    }

    #[test]
    fn callable_start_wire_shape_is_typed_and_backend_neutral() {
        let message = ClientMessage {
            id: 9,
            command: Command::StartCallable {
                session_id: SessionId::parse("session-1").expect("valid session id"),
                callable: CallableId::parse("orchestration.implement").expect("valid callable id"),
                input: serde_json::json!({"objective": "implement change"}),
            },
        };
        let value = serde_json::to_value(message).expect("serialize callable start");
        assert_eq!(value["command"]["type"], "start_callable");
        assert_eq!(value["command"]["session_id"], "session-1");
        assert_eq!(value["command"]["callable"], "orchestration.implement");
        assert_eq!(
            value["command"]["input"],
            serde_json::json!({"objective": "implement change"})
        );
        assert!(value["command"].get("backend").is_none());
        assert!(value["command"].get("provider").is_none());
    }

    #[test]
    fn callable_catalog_wire_shape_is_conductor_owned() {
        let message = ClientMessage {
            id: 10,
            command: Command::GetCallableCatalog,
        };
        let value = serde_json::to_value(message).expect("serialize callable catalog request");
        assert_eq!(value["command"]["type"], "get_callable_catalog");
        assert_eq!(value["command"].as_object().unwrap().len(), 1);
    }

    #[test]
    fn skill_catalog_wire_shape_is_conductor_owned() {
        let request = ClientMessage {
            id: 12,
            command: Command::GetSkillCatalog,
        };
        let value = serde_json::to_value(request).expect("serialize skill catalog request");
        assert_eq!(value["command"]["type"], "get_skill_catalog");
        assert_eq!(value["command"].as_object().unwrap().len(), 1);
    }

    #[test]
    fn routing_catalog_wire_shape_is_conductor_owned() {
        let request = ClientMessage {
            id: 11,
            command: Command::GetRoutingCatalog,
        };
        let value = serde_json::to_value(request).expect("serialize routing catalog request");
        assert_eq!(value["command"]["type"], "get_routing_catalog");
        assert_eq!(value["command"].as_object().unwrap().len(), 1);

        let reply = ServerMessage::Response {
            id: 11,
            response: ResponsePayload::Ok {
                result: Reply::RoutingCatalog {
                    profiles: vec![RoutingProfileDescriptor {
                        id: RoutingProfileId::parse("router.mixed").unwrap(),
                        providers: vec![
                            ProviderId::parse("openai-codex").unwrap(),
                            ProviderId::parse("opencode-go").unwrap(),
                        ],
                    }],
                },
            },
        };
        let value = serde_json::to_value(reply).expect("serialize routing catalog reply");
        assert_eq!(value["result"]["type"], "routing_catalog");
        assert_eq!(value["result"]["profiles"][0]["id"], "router.mixed");
        assert_eq!(
            value["result"]["profiles"][0]["providers"],
            serde_json::json!(["openai-codex", "opencode-go"])
        );
    }

    #[test]
    fn api_key_auth_input_is_typed_and_debug_redacted() {
        let message = ClientMessage {
            id: 11,
            command: Command::SelectAuthentication {
                backend_id: BackendId::parse("phenix").unwrap(),
                method_id: AuthenticationMethodId::parse("openai-api").unwrap(),
                input: Some(AuthenticationInput::ApiKey {
                    secret: "super-secret".to_owned(),
                }),
            },
        };
        let value = serde_json::to_value(&message).expect("serialize auth request");
        assert_eq!(value["command"]["input"]["type"], "api_key");
        assert_eq!(value["command"]["input"]["secret"], "super-secret");
        assert!(!format!("{message:?}").contains("super-secret"));
    }

    #[test]
    fn close_session_is_an_explicit_terminal_operation() {
        let message = ClientMessage {
            id: 11,
            command: Command::CloseSession {
                session_id: SessionId::parse("session-1").unwrap(),
            },
        };
        let value = serde_json::to_value(message).expect("serialize session close");
        assert_eq!(value["command"]["type"], "close_session");
        assert_eq!(value["command"]["session_id"], "session-1");
    }

    #[test]
    fn session_rebase_is_an_explicit_revision_operation() {
        let message = ClientMessage {
            id: 13,
            command: Command::RebaseSession {
                session_id: SessionId::parse("session-1").unwrap(),
                config_revision: ConfigRevisionId::parse("config-3").unwrap(),
            },
        };
        let value = serde_json::to_value(message).unwrap();
        assert_eq!(value["command"]["type"], "rebase_session");
        assert_eq!(value["command"]["session_id"], "session-1");
        assert_eq!(value["command"]["config_revision"], "config-3");
    }

    #[test]
    fn session_debug_export_is_an_explicit_operation() {
        let message = ClientMessage {
            id: 12,
            command: Command::ExportSessionDebug {
                session_id: SessionId::parse("session-1").unwrap(),
            },
        };
        let value = serde_json::to_value(message).unwrap();
        assert_eq!(value["command"]["type"], "export_session_debug");
        assert_eq!(value["command"]["session_id"], "session-1");
    }

    #[test]
    fn workspace_checkpoint_request_is_explicit() {
        let value = serde_json::to_value(ClientMessage {
            id: 13,
            command: Command::RequestWorkspaceCheckpoint {
                execution_id: ExecutionId::parse("execution-1").unwrap(),
            },
        })
        .unwrap();
        assert_eq!(value["command"]["type"], "request_workspace_checkpoint");
        assert_eq!(value["command"]["execution_id"], "execution-1");
    }

    #[test]
    fn execution_provider_failure_has_stable_wire_code() {
        let value = serde_json::to_value(ErrorCode::ExecutionProviderFailure)
            .expect("serialize error code");
        assert_eq!(value, "execution_provider_failure");
    }

    #[test]
    fn frontend_provider_ids_reject_blank_wire_values() {
        assert!(serde_json::from_str::<FrontendServiceProviderId>("\"\"").is_err());
        assert!(serde_json::from_str::<FrontendServiceProviderId>("\"  \"").is_err());
    }

    fn frontend_provider() -> FrontendServiceProviderDescriptor {
        FrontendServiceProviderDescriptor {
            id: FrontendServiceProviderId::parse("web").unwrap(),
            capabilities: BTreeSet::from(["search".to_owned(), "fetch".to_owned()]),
        }
    }

    #[test]
    fn frontend_provider_registration_is_connection_protocol_state() {
        let message = ClientEnvelope::ConnectionCommand(
            FrontendConnectionCommand::SetFrontendServiceProviders {
                id: 20,
                providers: vec![frontend_provider()],
            },
        );
        let value = serde_json::to_value(message).unwrap();
        assert_eq!(value["type"], "set_frontend_service_providers");
        assert_eq!(value["id"], 20);
        assert_eq!(value["providers"][0]["id"], "web");
        assert_eq!(
            value["providers"][0]["capabilities"],
            serde_json::json!(["fetch", "search"])
        );
        assert!(value.get("command").is_none());
    }

    #[test]
    fn reverse_request_and_response_have_correlated_ids() {
        let request = ServerMessage::FrontendServiceRequest {
            id: 41,
            request: FrontendServiceRequest {
                provider: FrontendServiceProviderId::parse("web").unwrap(),
                method: "search".to_owned(),
                params: serde_json::json!({"query": "nixos"}),
            },
        };
        let value = serde_json::to_value(request).unwrap();
        assert_eq!(value["type"], "frontend_service_request");
        assert_eq!(value["id"], 41);
        assert_eq!(value["request"]["provider"], "web");
        assert_eq!(value["request"]["method"], "search");

        let response = ClientEnvelope::FrontendServiceResponse(FrontendServiceResponse {
            id: 41,
            response: FrontendServiceResponsePayload::Ok {
                result: serde_json::json!({"items": []}),
            },
        });
        let value = serde_json::to_value(response).unwrap();
        assert_eq!(value["id"], 41);
        assert_eq!(value["status"], "ok");
        assert_eq!(value["result"], serde_json::json!({"items": []}));
        assert!(value.get("command").is_none());
    }

    #[test]
    fn frontend_notifications_are_one_way_in_both_directions() {
        let notification = FrontendServiceNotification {
            provider: FrontendServiceProviderId::parse("web").unwrap(),
            method: "invalidate".to_owned(),
            params: serde_json::json!({"key": "cache"}),
        };
        let outbound = ServerMessage::FrontendServiceNotification {
            notification: notification.clone(),
        };
        let value = serde_json::to_value(outbound).unwrap();
        assert_eq!(value["type"], "frontend_service_notification");
        assert_eq!(value["notification"]["provider"], "web");
        assert!(value.get("id").is_none());

        let inbound =
            ClientEnvelope::ConnectionEvent(FrontendConnectionEvent::FrontendServiceNotification {
                notification,
            });
        let value = serde_json::to_value(inbound).unwrap();
        assert_eq!(value["type"], "frontend_service_notification");
        assert_eq!(value["notification"]["provider"], "web");
        assert!(value.get("id").is_none());
    }

    #[test]
    fn client_envelope_keeps_existing_command_wire_shape() {
        let envelope = ClientEnvelope::Command(ClientMessage {
            id: 7,
            command: Command::GetSnapshot,
        });
        let value = serde_json::to_value(envelope).unwrap();
        assert_eq!(value["id"], 7);
        assert_eq!(value["command"]["type"], "get_snapshot");
        assert!(value.get("type").is_none());
    }
}
