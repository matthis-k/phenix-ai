//! Terminal and JSONL presentation adapters. Each feature builds a separate
//! plugin artifact; both use the same typed Basic contracts and codec.
#[cfg(any(
    all(feature = "terminal", feature = "jsonl"),
    all(feature = "terminal", feature = "acp"),
    all(feature = "jsonl", feature = "acp")
))]
compile_error!("select exactly one Basic frontend adapter");

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
pub fn acp_decode(request: ClientInput) -> Result<Submission, BasicError> {
    let frame: serde_json::Value = serde_json::from_str(&request.line)
        .map_err(|_| error(ErrorCode::InvalidInput, "malformed ACP JSON-RPC frame"))?;
    if frame.get("jsonrpc").and_then(|value| value.as_str()) != Some("2.0") {
        return Err(error(ErrorCode::InvalidInput, "invalid JSON-RPC version"));
    }
    let method = frame
        .get("method")
        .and_then(|value| value.as_str())
        .ok_or_else(|| error(ErrorCode::InvalidInput, "missing ACP method"))?;
    let session = frame
        .pointer("/params/sessionId")
        .and_then(|value| value.as_str())
        .ok_or_else(|| error(ErrorCode::InvalidInput, "missing ACP sessionId"))?;
    if session != request.session {
        return Err(error(ErrorCode::Denied, "ACP session mismatch"));
    }
    match method {
        "session/cancel" => {
            if frame.get("id").is_some() {
                return Err(error(
                    ErrorCode::InvalidInput,
                    "ACP cancel must be a notification",
                ));
            }
            Ok(cancellation())
        }
        "session/prompt" => {
            if frame.get("id").is_none() {
                return Err(error(
                    ErrorCode::InvalidInput,
                    "ACP prompt requires request ID",
                ));
            }
            let blocks = frame
                .pointer("/params/prompt")
                .and_then(|value| value.as_array())
                .ok_or_else(|| error(ErrorCode::InvalidInput, "missing ACP prompt blocks"))?;
            let mut texts = Vec::with_capacity(blocks.len());
            for block in blocks {
                if block.get("type").and_then(|value| value.as_str()) != Some("text") {
                    return Err(error(
                        ErrorCode::InvalidInput,
                        "unsupported ACP content block",
                    ));
                }
                let text = block
                    .get("text")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| error(ErrorCode::InvalidInput, "invalid ACP text block"))?;
                texts.push(text);
            }
            prompt(request.session, texts.join("\n"), Limits::default())
        }
        _ => Err(error(
            ErrorCode::InvalidInput,
            "unsupported ACP turn method",
        )),
    }
}

/// Each output line is a protocol notification, never a terminal transcript.
pub fn acp_render(request: RenderRequest) -> Result<Vec<String>, BasicError> {
    let mut output = Vec::new();
    for event in request.events {
        if event.session != request.session {
            return Err(error(ErrorCode::Denied, "cross-session ACP output"));
        }
        let update = match event.kind {
            EventKind::TextDelta => Some(serde_json::json!({
                "sessionUpdate": "agent_message_chunk",
                "messageId": event.run,
                "content": {"type": "text", "text": event.text}
            })),
            EventKind::ToolStarted => Some(serde_json::json!({
                "sessionUpdate": "tool_call",
                "toolCallId": event.call_id,
                "title": event.text,
                "kind": "other",
                "status": "pending"
            })),
            EventKind::ToolFinished => Some(serde_json::json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": event.call_id,
                "status": "completed",
                "content": [{"type": "content", "content": {"type": "text", "text": event.text}}]
            })),
            _ => None,
        };
        if let Some(update) = update {
            output.push(
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "session/update",
                    "params": {"sessionId": request.session, "update": update}
                })
                .to_string(),
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
