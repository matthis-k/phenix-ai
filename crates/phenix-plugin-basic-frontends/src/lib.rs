//! Terminal and JSONL presentation adapters. Each feature builds a separate
//! plugin artifact; both use the same typed Basic contracts and codec.
#[cfg(any(
    all(feature = "terminal", feature = "jsonl"),
    all(feature = "terminal", feature = "acp"),
    all(feature = "jsonl", feature = "acp")
))]
compile_error!("select exactly one Basic frontend adapter");

#[cfg(feature = "acp")]
use phenix_basic_contracts::{decode, ToolCompletion};
use phenix_basic_contracts::{
    encode, error, BasicError, ClientInput, ErrorCode, EventKind, Limits, PromptSubmitted,
    RenderRequest, Submission,
};
use phenix_sdk::Blob;
use serde::{Deserialize, Serialize};

fn prompt(session: String, text: String, limits: Limits) -> Result<Submission, BasicError> {
    if text.is_empty() {
        return Err(error(ErrorCode::InvalidInput, "empty prompt"));
    }
    let payload = encode(&PromptSubmitted {
        session,
        text,
        limits,
    })
    .map_err(|_| error(ErrorCode::InvalidInput, "invalid prompt payload"))?;
    Ok(Submission {
        action: "phenix.basic.submit".into(),
        payload,
        cancel: false,
    })
}

fn cancellation() -> Submission {
    Submission {
        action: "phenix.basic.submit".into(),
        payload: Blob(vec![]),
        cancel: true,
    }
}

pub fn terminal_decode(request: ClientInput) -> Result<Submission, BasicError> {
    if request.line.trim() == "/cancel" {
        return Ok(cancellation());
    }
    prompt(request.session, request.line, Limits::default())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Input {
    Prompt {
        session: String,
        text: String,
        #[serde(default)]
        limits: Limits,
    },
    Cancel {
        session: String,
    },
}

pub fn jsonl_decode(request: ClientInput) -> Result<Submission, BasicError> {
    let incoming: Input = serde_json::from_str(&request.line)
        .map_err(|_| error(ErrorCode::InvalidInput, "malformed JSONL input"))?;
    match incoming {
        Input::Prompt {
            session,
            text,
            limits,
        } => {
            if session != request.session {
                return Err(error(ErrorCode::Denied, "JSONL session mismatch"));
            }
            prompt(session, text, limits)
        }
        Input::Cancel { session } => {
            if session != request.session {
                return Err(error(ErrorCode::Denied, "JSONL session mismatch"));
            }
            Ok(cancellation())
        }
    }
}

pub fn terminal_render(request: RenderRequest) -> Result<Vec<String>, BasicError> {
    let mut lines = Vec::new();
    for item in request.events {
        if item.session != request.session {
            return Err(error(ErrorCode::Denied, "cross-session output"));
        }
        let text = match item.kind {
            EventKind::TextDelta => Some(item.text),
            EventKind::Answer => Some(format!("answer: {}", item.text)),
            EventKind::Failure => Some(format!("error: {}", item.text)),
            EventKind::Cancelled => Some("cancelled".into()),
            EventKind::ToolStarted => Some(format!("tool: {}", item.text)),
            EventKind::ToolFinished => Some(format!("tool result: {}", item.text)),
            _ => None,
        };
        if let Some(text) = text {
            lines.push(text);
        }
    }
    Ok(lines)
}

pub fn jsonl_render(request: RenderRequest) -> Result<Vec<String>, BasicError> {
    let mut lines = Vec::with_capacity(request.events.len());
    for event in request.events {
        if event.session != request.session {
            return Err(error(ErrorCode::Denied, "cross-session output"));
        }
        lines.push(
            serde_json::to_string(&event)
                .map_err(|_| error(ErrorCode::InvalidReply, "event encoding failed"))?,
        );
    }
    Ok(lines)
}

#[cfg(feature = "terminal")]
#[phenix_sdk::plugin(
    id = "phenix.basic.frontend-terminal",
    version = "1.0.0",
    provide(phenix_basic_contracts::Frontend, slot = "Frontend")
)]
mod terminal {
    use phenix_basic_contracts::{
        BasicError, ClientInput, Frontend, RenderRequest, Scope, Submission,
    };
    use phenix_sdk::{CallError, CallResult, Host};

    #[derive(Default)]
    pub struct State;

    #[provide(Frontend::decode)]
    fn decode(
        _: &mut State,
        host: &Host<'_>,
        request: ClientInput,
    ) -> CallResult<Submission, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        super::terminal_decode(request).map_err(CallError::Domain)
    }

    #[provide(Frontend::render)]
    fn render(
        _: &mut State,
        host: &Host<'_>,
        request: RenderRequest,
    ) -> CallResult<Vec<String>, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        super::terminal_render(request).map_err(CallError::Domain)
    }

    #[provide(Frontend::cancel)]
    fn cancel(_: &mut State, host: &Host<'_>, request: Scope) -> CallResult<(), BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        Err(CallError::Domain(phenix_basic_contracts::error(
            phenix_basic_contracts::ErrorCode::InvalidInput,
            "host controller must cancel the active graph root",
        )))
    }
}

#[cfg(feature = "jsonl")]
#[phenix_sdk::plugin(
    id = "phenix.basic.frontend-jsonl",
    version = "1.0.0",
    provide(phenix_basic_contracts::Frontend, slot = "Frontend")
)]
mod jsonl {
    use phenix_basic_contracts::{
        BasicError, ClientInput, Frontend, RenderRequest, Scope, Submission,
    };
    use phenix_sdk::{CallError, CallResult, Host};

    #[derive(Default)]
    pub struct State;

    #[provide(Frontend::decode)]
    fn decode(
        _: &mut State,
        host: &Host<'_>,
        request: ClientInput,
    ) -> CallResult<Submission, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        super::jsonl_decode(request).map_err(CallError::Domain)
    }

    #[provide(Frontend::render)]
    fn render(
        _: &mut State,
        host: &Host<'_>,
        request: RenderRequest,
    ) -> CallResult<Vec<String>, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        super::jsonl_render(request).map_err(CallError::Domain)
    }

    #[provide(Frontend::cancel)]
    fn cancel(_: &mut State, host: &Host<'_>, request: Scope) -> CallResult<(), BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        Err(CallError::Domain(phenix_basic_contracts::error(
            phenix_basic_contracts::ErrorCode::InvalidInput,
            "host controller must cancel the active graph root",
        )))
    }
}

/// ACP v1 prompt and cancel frames. Initialize and session creation belong
/// to the connection host; this provider only decodes authorized turn input.
#[cfg(feature = "acp")]
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum AcpInput {
    // Internal, tagged SDK-to-plugin payloads. No JSON-RPC frames.
    Prompt(agent_client_protocol_schema::v1::PromptRequest),
    Cancel(agent_client_protocol_schema::v1::CancelNotification),
    #[cfg(feature = "acp_v2")]
    PromptV2(agent_client_protocol_schema::v2::PromptRequest),
    #[cfg(feature = "acp_v2")]
    CancelV2(agent_client_protocol_schema::v2::CancelSessionNotification),
}

#[cfg(feature = "acp")]
fn acp_resource_reference(name: &str, uri: &str) -> String {
    // A URI is a reference for the Model or authorized Tool provider,
    // never an instruction for the Frontend to fetch local/remote content.
    format!("Resource link: {name} ({uri})")
}

#[cfg(feature = "acp")]
pub fn acp_decode(request: ClientInput) -> Result<Submission, BasicError> {
    use agent_client_protocol_schema::v1::ContentBlock;

    let packet: AcpInput = serde_json::from_str(&request.line)
        .map_err(|_| error(ErrorCode::InvalidInput, "invalid typed ACP input"))?;
    match packet {
        AcpInput::Cancel(cancel) => {
            if cancel.session_id.to_string() != request.session {
                return Err(error(ErrorCode::Denied, "ACP session mismatch"));
            }
            Ok(cancellation())
        }
        #[cfg(feature = "acp_v2")]
        AcpInput::CancelV2(cancel) => {
            if cancel.session_id.to_string() != request.session {
                return Err(error(ErrorCode::Denied, "ACP session mismatch"));
            }
            Ok(cancellation())
        }
        AcpInput::Prompt(incoming) => {
            if incoming.session_id.to_string() != request.session {
                return Err(error(ErrorCode::Denied, "ACP session mismatch"));
            }
            let mut texts = Vec::with_capacity(incoming.prompt.len());
            for block in incoming.prompt {
                match block {
                    ContentBlock::Text(text) => texts.push(text.text),
                    ContentBlock::ResourceLink(link) => {
                        // ACP v1 requires resource links. Basic carries the
                        // reference to the model as text; the frontend never
                        // dereferences a URI or grants filesystem authority.
                        texts.push(acp_resource_reference(&link.name, &link.uri));
                    }
                    _ => {
                        return Err(error(
                            ErrorCode::InvalidInput,
                            "unsupported ACP content block",
                        ))
                    }
                }
            }
            prompt(request.session, texts.join("\n"), Limits::default())
        }
        #[cfg(feature = "acp_v2")]
        AcpInput::PromptV2(incoming) => {
            use agent_client_protocol_schema::v2::ContentBlock as V2ContentBlock;
            if incoming.session_id.to_string() != request.session {
                return Err(error(ErrorCode::Denied, "ACP session mismatch"));
            }
            let mut texts = Vec::with_capacity(incoming.prompt.len());
            for block in incoming.prompt {
                match block {
                    V2ContentBlock::Text(text) => texts.push(text.text),
                    V2ContentBlock::ResourceLink(link) => {
                        texts.push(acp_resource_reference(&link.name, &link.uri));
                    }
                    // The official draft-v2 schema preserves extensions.
                    // This non-proxy Basic adapter may ignore unknown blocks,
                    // but must not silently enable optional image/audio I/O.
                    V2ContentBlock::Other(_) => {}
                    _ => {
                        return Err(error(
                            ErrorCode::InvalidInput,
                            "unsupported ACP v2 content block",
                        ))
                    }
                }
            }
            prompt(request.session, texts.join("\n"), Limits::default())
        }
    }
}

/// The journal stores typed tool completion metadata. Both ACP versions use
/// the same result, including legacy payload-free successful tool events.
#[cfg(feature = "acp")]
fn acp_tool_failed(data: &Blob) -> Result<bool, BasicError> {
    if data.0.is_empty() {
        Ok(false)
    } else {
        decode::<ToolCompletion>(data)
            .map(|result| result.failed)
            .map_err(|_| error(ErrorCode::InvalidReply, "invalid tool completion metadata"))
    }
}

/// Serialize only typed ACP v1 SessionNotification payloads across the native
/// Frontend ABI. The official SDK owns JSON-RPC method/envelope framing.
#[cfg(feature = "acp")]
pub fn acp_render(request: RenderRequest) -> Result<Vec<String>, BasicError> {
    use agent_client_protocol_schema::v1::{
        ContentBlock, ContentChunk, MessageId, SessionNotification, SessionUpdate, TextContent,
        ToolCall, ToolCallContent, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
    };

    let mut output = Vec::new();
    for event in request.events {
        if event.session != request.session {
            return Err(error(ErrorCode::Denied, "cross-session ACP output"));
        }
        let update = match event.kind {
            // Project the accepted user prompt from the authoritative journal.
            // Loading the session replays it exactly like assistant/tool events.
            EventKind::PromptAccepted => Some(SessionUpdate::UserMessageChunk(
                ContentChunk::new(ContentBlock::Text(TextContent::new(event.text)))
                    .message_id(MessageId::new(format!("{}:user", event.run))),
            )),
            EventKind::TextDelta => Some(SessionUpdate::AgentMessageChunk(
                ContentChunk::new(ContentBlock::Text(TextContent::new(event.text)))
                    .message_id(MessageId::new(event.run)),
            )),
            EventKind::ToolStarted => Some(SessionUpdate::ToolCall(
                ToolCall::new(event.call_id, event.text).status(ToolCallStatus::Pending),
            )),
            EventKind::ToolFinished => {
                let failed = acp_tool_failed(&event.data)?;
                let status = if failed {
                    ToolCallStatus::Failed
                } else {
                    ToolCallStatus::Completed
                };
                let fields = ToolCallUpdateFields::new().status(status).content(vec![
                    ToolCallContent::from(ContentBlock::Text(TextContent::new(event.text))),
                ]);
                Some(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                    event.call_id,
                    fields,
                )))
            }
            _ => None,
        };
        if let Some(update) = update {
            let notification = SessionNotification::new(request.session.clone(), update);
            output.push(
                serde_json::to_string(&notification)
                    .map_err(|_| error(ErrorCode::InvalidReply, "invalid ACP update payload"))?,
            );
        }
    }
    Ok(output)
}

/// Draft-v2 journal projection using the official schema. The v2 host sends
/// these typed notifications through the SDK, never as handmade JSON-RPC.
/// Session running/idle transitions belong to the v2 graph controller.
/// This helper is feature-gated until the v2 native provider is wired.
#[cfg(feature = "acp_v2")]
pub fn acp_render_v2(request: RenderRequest) -> Result<Vec<String>, BasicError> {
    use agent_client_protocol_schema::v2::{
        ContentChunk, MessageId, SessionId, SessionUpdate, ToolCallStatus, ToolCallUpdate,
        UpdateSessionNotification,
    };

    let mut output = Vec::new();
    for event in request.events {
        if event.session != request.session {
            return Err(error(ErrorCode::Denied, "cross-session ACP output"));
        }
        let update = match event.kind {
            EventKind::TextDelta => Some(SessionUpdate::AgentMessageChunk(ContentChunk::new(
                event.text.into(),
                MessageId::new(event.run),
            ))),
            EventKind::ToolStarted => Some(SessionUpdate::ToolCallUpdate(
                ToolCallUpdate::new(event.call_id)
                    .title(event.text)
                    .status(ToolCallStatus::Pending),
            )),
            EventKind::ToolFinished => {
                let status = if acp_tool_failed(&event.data)? {
                    ToolCallStatus::Failed
                } else {
                    ToolCallStatus::Completed
                };
                Some(SessionUpdate::ToolCallUpdate(
                    ToolCallUpdate::new(event.call_id).status(status),
                ))
            }
            _ => None,
        };
        if let Some(update) = update {
            let notification =
                UpdateSessionNotification::new(SessionId::new(request.session.clone()), update);
            output
                .push(serde_json::to_string(&notification).map_err(|_| {
                    error(ErrorCode::InvalidReply, "invalid ACP v2 update payload")
                })?);
        }
    }
    Ok(output)
}

#[cfg(feature = "acp")]
#[phenix_sdk::plugin(
    id = "phenix.basic.frontend-acp",
    version = "1.0.0",
    provide(phenix_basic_contracts::Frontend, slot = "Frontend")
)]
mod acp {
    use phenix_basic_contracts::{
        BasicError, ClientInput, Frontend, RenderRequest, Scope, Submission,
    };
    use phenix_sdk::{CallError, CallResult, Host};

    #[derive(Default)]
    pub struct State;

    #[provide(Frontend::decode)]
    fn decode(
        _: &mut State,
        host: &Host<'_>,
        request: ClientInput,
    ) -> CallResult<Submission, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        super::acp_decode(request).map_err(CallError::Domain)
    }

    #[provide(Frontend::render)]
    fn render(
        _: &mut State,
        host: &Host<'_>,
        request: RenderRequest,
    ) -> CallResult<Vec<String>, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        super::acp_render(request).map_err(CallError::Domain)
    }

    #[provide(Frontend::cancel)]
    fn cancel(_: &mut State, host: &Host<'_>, request: Scope) -> CallResult<(), BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        Err(CallError::Domain(phenix_basic_contracts::error(
            phenix_basic_contracts::ErrorCode::InvalidInput,
            "ACP host must cancel the active graph root",
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_basic_contracts::{Event, EventKind};

    #[cfg(feature = "acp")]
    #[test]
    fn official_acp_v1_schema_decodes_authorized_turns() {
        let decode = |packet: serde_json::Value| {
            acp_decode(ClientInput {
                session: "s1".into(),
                line: packet.to_string(),
            })
        };
        let accepted = decode(serde_json::json!({
            "prompt": {
                "sessionId": "s1",
                "prompt": [
                    {"type": "text", "text": "hello"},
                    {"type": "text", "text": "world"}
                ]
            }
        }))
        .unwrap();
        assert_eq!(accepted.action, "phenix.basic.submit");
        let payload: PromptSubmitted = phenix_basic_contracts::decode(&accepted.payload).unwrap();
        assert_eq!(payload.text, "hello\nworld");
        assert_eq!(payload.session, "s1");

        let resource = decode(serde_json::json!({
            "prompt": {
                "sessionId": "s1",
                "prompt": [
                    {"type": "text", "text": "read this reference"},
                    {
                        "type": "resource_link",
                        "name": "design notes",
                        "uri": "file:///tmp/design.md"
                    }
                ]
            }
        }))
        .unwrap();
        let resource_payload: PromptSubmitted =
            phenix_basic_contracts::decode(&resource.payload).unwrap();
        assert_eq!(
            resource_payload.text,
            "read this reference\nResource link: design notes (file:///tmp/design.md)"
        );

        assert!(
            decode(serde_json::json!({"cancel": {"sessionId": "s1"}}))
                .unwrap()
                .cancel
        );
        let refused = decode(serde_json::json!({
            "prompt": {"sessionId": "s2", "prompt": [{"type": "text", "text": "other"}]}
        }))
        .unwrap_err();
        assert_eq!(refused.code, ErrorCode::Denied);
        for packet in [
            serde_json::json!({"prompt": {
                "sessionId": "s1", "prompt": [{"type": "text", "text": 12}]
            }}),
            serde_json::json!({"prompt": {
                "sessionId": "s1", "prompt": [{"type": "image", "data": "x"}]
            }}),
            serde_json::json!({"prompt": {"sessionId": "s1"}}),
            serde_json::json!({"prompt": {
                "sessionId": "s1", "prompt": []
            }}),
            serde_json::json!({"unknown": {"sessionId": "s1"}}),
            // JSON-RPC envelopes must be handled by the SDK, never here.
            serde_json::json!({
                "jsonrpc": "2.0", "id": 8, "method": "session/prompt",
                "params": {"sessionId": "s1", "prompt": []}
            }),
        ] {
            assert_eq!(decode(packet).unwrap_err().code, ErrorCode::InvalidInput);
        }
    }

    #[cfg(feature = "acp_v2")]
    #[test]
    fn draft_v2_renders_typed_journal_updates_without_wire_envelopes() {
        use agent_client_protocol_schema::v2::UpdateSessionNotification;

        let event = |kind: EventKind, data: Blob, call_id: &str, text: &str| Event {
            sequence: 1,
            session: "s1".into(),
            run: "r1".into(),
            kind,
            call_id: call_id.into(),
            text: text.into(),
            data,
        };
        let request = RenderRequest {
            session: "s1".into(),
            events: vec![
                event(EventKind::TextDelta, Blob(vec![]), "", "part"),
                event(EventKind::ToolStarted, Blob(vec![]), "call-1", "read"),
                event(
                    EventKind::ToolFinished,
                    encode(&ToolCompletion { failed: true }).unwrap(),
                    "call-1",
                    "failed",
                ),
            ],
        };
        let updates = acp_render_v2(request).unwrap();
        assert_eq!(updates.len(), 3);
        let values: Vec<serde_json::Value> = updates
            .iter()
            .map(|line| {
                let notification: UpdateSessionNotification = serde_json::from_str(line).unwrap();
                assert_eq!(notification.session_id.to_string(), "s1");
                let value: serde_json::Value = serde_json::from_str(line).unwrap();
                assert!(value.get("jsonrpc").is_none());
                assert!(value.get("method").is_none());
                value
            })
            .collect();
        assert_eq!(values[0]["update"]["sessionUpdate"], "agent_message_chunk");
        assert_eq!(values[1]["update"]["sessionUpdate"], "tool_call_update");
        assert_eq!(values[2]["update"]["sessionUpdate"], "tool_call_update");
        assert_eq!(values[2]["update"]["status"], "failed");

        let cross_session = RenderRequest {
            session: "s1".into(),
            events: vec![event(EventKind::TextDelta, Blob(vec![]), "", "forged")],
        };
        let mut denied = cross_session;
        denied.events[0].session = "s2".into();
        assert_eq!(acp_render_v2(denied).unwrap_err().code, ErrorCode::Denied);
        assert_eq!(
            acp_render_v2(RenderRequest {
                session: "s1".into(),
                events: vec![event(
                    EventKind::ToolFinished,
                    Blob(vec![0xff]),
                    "call-2",
                    "corrupt",
                )],
            })
            .unwrap_err()
            .code,
            ErrorCode::InvalidReply,
        );
    }

    #[cfg(feature = "acp_v2")]
    #[test]
    fn draft_v2_uses_same_authorized_submission_without_new_wire_parser() {
        let decode = |payload: serde_json::Value| {
            acp_decode(ClientInput {
                session: "s1".into(),
                line: payload.to_string(),
            })
        };
        let submitted = decode(serde_json::json!({
            "prompt_v2": {
                "sessionId": "s1",
                "prompt": [
                    {"type": "text", "text": "inspect"},
                    {"type": "resource_link", "name": "notes", "uri": "file:///tmp/notes.md"}
                ]
            }
        }))
        .unwrap();
        assert_eq!(submitted.action, "phenix.basic.submit");
        let turn: PromptSubmitted = phenix_basic_contracts::decode(&submitted.payload).unwrap();
        assert_eq!(turn.session, "s1");
        assert_eq!(
            turn.text,
            "inspect\nResource link: notes (file:///tmp/notes.md)"
        );

        let refused = decode(serde_json::json!({
            "prompt_v2": {
                "sessionId": "s2",
                "prompt": [{"type": "text", "text": "forged"}]
            }
        }))
        .unwrap_err();
        assert_eq!(refused.code, ErrorCode::Denied);
        for payload in [
            serde_json::json!({"prompt_v2": {
                "sessionId": "s1",
                "prompt": [{"type": "image", "data": "x", "mimeType": "image/png"}]
            }}),
            serde_json::json!({"prompt_v2": {"sessionId": "s1", "prompt": []}}),
            // SDK wire parsing and JSON-RPC envelopes never belong to
            // this native Frontend contract, for either v1 or v2.
            serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "session/prompt",
                "params": {"sessionId": "s1", "prompt": []}
            }),
        ] {
            assert_eq!(decode(payload).unwrap_err().code, ErrorCode::InvalidInput);
        }

        // v2 cancellation is a typed, session-scoped SDK notification.
        // The native adapter never handles the JSON-RPC envelope.
        let cancelled = decode(serde_json::json!({
            "cancel_v2": {"sessionId": "s1"}
        }))
        .unwrap();
        assert!(cancelled.cancel);
        assert_eq!(cancelled.action, "phenix.basic.submit");
        assert_eq!(
            decode(serde_json::json!({
                "cancel_v2": {"sessionId": "s2"}
            }))
            .unwrap_err()
            .code,
            ErrorCode::Denied
        );
        for malformed in [
            serde_json::json!({"cancel_v2": {}}),
            serde_json::json!({"cancel_v2": {"sessionId": 3}}),
            serde_json::json!({"cancel_v2": {"sessionId": "s1", "prompt": []}, "prompt": {}}),
        ] {
            assert_eq!(decode(malformed).unwrap_err().code, ErrorCode::InvalidInput);
        }

        // Stable-v1 prompt and cancellation remain available in the dual-schema build.
        let v1_cancelled = decode(serde_json::json!({
            "cancel": {"sessionId": "s1"}
        }))
        .unwrap();
        assert!(v1_cancelled.cancel);
        assert_eq!(
            decode(serde_json::json!({
                "prompt": {
                    "sessionId": "s1",
                    "prompt": [{"type": "text", "text": "stable v1"}]
                }
            }))
            .unwrap()
            .action,
            "phenix.basic.submit"
        );
    }

    #[test]
    fn terminal_and_jsonl_produce_the_same_prompt_action() {
        let line = terminal_decode(ClientInput {
            session: "s1".into(),
            line: "hello".into(),
        })
        .unwrap();
        let json = jsonl_decode(ClientInput {
            session: "s1".into(),
            line: r#"{"type":"prompt","session":"s1","text":"hello"}"#.into(),
        })
        .unwrap();
        assert_eq!(line, json);
        assert_eq!(line.action, "phenix.basic.submit");
        assert_eq!(
            terminal_decode(ClientInput {
                session: "s1".into(),
                line: "/cancel".into(),
            })
            .unwrap(),
            cancellation()
        );
        assert_eq!(
            jsonl_decode(ClientInput {
                session: "s1".into(),
                line: r#"{"type":"cancel","session":"s1"}"#.into(),
            })
            .unwrap(),
            cancellation()
        );
        assert_eq!(
            jsonl_decode(ClientInput {
                session: "s1".into(),
                line: r#"{"type":"prompt","session":"s2","text":"forged"}"#.into()
            })
            .unwrap_err()
            .code,
            ErrorCode::Denied
        );
    }

    #[test]
    fn terminal_renders_durable_cancellation() {
        let lines = terminal_render(RenderRequest {
            session: "s1".into(),
            events: vec![Event {
                sequence: 1,
                session: "s1".into(),
                run: "r1".into(),
                kind: EventKind::Cancelled,
                call_id: String::new(),
                text: String::new(),
                data: Blob(vec![]),
            }],
        })
        .unwrap();
        assert_eq!(lines, vec!["cancelled"]);
    }

    #[cfg(feature = "acp")]
    #[test]
    fn acp_reports_typed_tool_outcomes_and_rejects_corrupt_metadata() {
        let render = |data: Blob| {
            acp_render(RenderRequest {
                session: "s1".into(),
                events: vec![Event {
                    sequence: 1,
                    session: "s1".into(),
                    run: "r1".into(),
                    kind: EventKind::ToolFinished,
                    call_id: "call-1".into(),
                    text: "tool output".into(),
                    data,
                }],
            })
        };
        for (failed, expected) in [(false, "completed"), (true, "failed")] {
            let data = encode(&ToolCompletion { failed }).unwrap();
            let frames = render(data).unwrap();
            assert_eq!(frames.len(), 1);
            let frame: serde_json::Value = serde_json::from_str(&frames[0]).unwrap();
            assert_eq!(
                frame
                    .pointer("/update/status")
                    .and_then(|status| status.as_str()),
                Some(expected),
            );
            assert_eq!(
                frame
                    .pointer("/update/toolCallId")
                    .and_then(|id| id.as_str()),
                Some("call-1"),
            );
        }
        // Preserve old durable journals with payload-free ToolFinished entries.
        let legacy = render(Blob(vec![])).unwrap();
        let legacy: serde_json::Value = serde_json::from_str(&legacy[0]).unwrap();
        assert_eq!(
            legacy
                .pointer("/update/status")
                .and_then(|status| status.as_str()),
            Some("completed"),
        );
        assert_eq!(
            render(Blob(vec![0xff])).unwrap_err().code,
            ErrorCode::InvalidReply,
        );
    }

    #[test]
    fn both_adapters_reject_cross_session_output() {
        let request = RenderRequest {
            session: "s1".into(),
            events: vec![Event {
                sequence: 1,
                session: "s2".into(),
                run: "r1".into(),
                kind: EventKind::Answer,
                call_id: String::new(),
                text: "answer".into(),
                data: Blob(vec![]),
            }],
        };
        assert_eq!(
            terminal_render(request.clone()).unwrap_err().code,
            ErrorCode::Denied
        );
        assert_eq!(jsonl_render(request).unwrap_err().code, ErrorCode::Denied);
    }
}
