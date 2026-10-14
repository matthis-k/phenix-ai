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
        AcpInput::Prompt(incoming) => {
            if incoming.session_id.to_string() != request.session {
                return Err(error(ErrorCode::Denied, "ACP session mismatch"));
            }
            let mut texts = Vec::with_capacity(incoming.prompt.len());
            for block in incoming.prompt {
                match block {
                    ContentBlock::Text(text) => texts.push(text.text),
                    _ => return Err(error(
                        ErrorCode::InvalidInput,
                        "unsupported ACP content block",
                    )),
                }
            }
            prompt(request.session, texts.join("\n"), Limits::default())
        }
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
            EventKind::TextDelta => Some(SessionUpdate::AgentMessageChunk(
                ContentChunk::new(ContentBlock::Text(TextContent::new(event.text)))
                    .message_id(MessageId::new(event.run)),
            )),
            EventKind::ToolStarted => Some(SessionUpdate::ToolCall(
                ToolCall::new(event.call_id, event.text).status(ToolCallStatus::Pending),
            )),
            EventKind::ToolFinished => {
                // Durable pre-payload entries represented successful work.
                let failed = if event.data.0.is_empty() {
                    false
                } else {
                    decode::<ToolCompletion>(&event.data)
                        .map_err(|_| {
                            error(ErrorCode::InvalidReply, "invalid tool completion metadata")
                        })?
                        .failed
                };
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

        assert!(decode(serde_json::json!({"cancel": {"sessionId": "s1"}}))
            .unwrap()
            .cancel);
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
