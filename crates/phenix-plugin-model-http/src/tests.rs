use crate::{request, stream::Stream};
use phenix_basic_contracts::{
    ContextItem, ErrorCode, Limits, Message, ModelRequest, Role, Skill, ToolCall, ToolInfo,
};
use serde_json::{json, Value};

fn request() -> ModelRequest {
    ModelRequest {
        session: "s1".into(),
        run: "r1".into(),
        history: vec![Message {
            role: Role::User,
            text: "hello".into(),
            calls: vec![],
            call_id: String::new(),
            error: None,
        }],
        tools: vec![ToolInfo {
            id: "uppercase".into(),
            description: "Uppercase".into(),
            arguments_schema: r#"{"type":"string"}"#.into(),
            idempotent: true,
        }],
        skills: vec![Skill {
            id: "rust".into(),
            description: "Rust".into(),
            instructions: "Use Rust".into(),
        }],
        context: vec![ContextItem {
            id: "repo".into(),
            text: "Phenix".into(),
        }],
        limits: Limits::default(),
    }
}
fn frame(value: Value) -> String {
    format!("data: {value}\r\n\r\n")
}
fn usage() -> Value {
    json!({"choices":[], "usage":{"prompt_tokens":27,"completion_tokens":7}})
}
#[test]
fn complete_history_and_scalar_argument_envelope_are_preserved() {
    let mut request = request();
    request.limits.model_tokens = Some(99);
    request.history.extend([
        Message {
            role: Role::Assistant,
            text: String::new(),
            calls: vec![ToolCall {
                id: "c1".into(),
                tool: "uppercase".into(),
                arguments: r#""aß""#.into(),
            }],
            call_id: String::new(),
            error: None,
        },
        Message {
            role: Role::Tool,
            text: String::new(),
            calls: vec![],
            call_id: "c1".into(),
            error: Some("tool failed".into()),
        },
    ]);
    let encoded: Value =
        serde_json::from_slice(&request::encode(&request, "fixture").unwrap()).unwrap();
    assert_eq!(encoded["max_completion_tokens"], 99);
    assert_eq!(encoded["messages"][0]["role"], "developer");
    assert!(encoded["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("Use Rust"));
    assert!(encoded["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("Phenix"));
    let args: Value = serde_json::from_str(
        encoded["messages"][2]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(args, json!({"value":"aß"}));
    assert!(encoded["messages"][3]["content"]
        .as_str()
        .unwrap()
        .contains("tool failed"));
    assert_eq!(
        encoded["tools"][0]["function"]["parameters"]["properties"]["value"]["type"],
        "string"
    );
    assert_eq!(encoded["stream_options"]["include_usage"], true);
}
#[test]
fn fragmented_utf8_sse_and_tool_arguments_decode_to_the_same_typed_reply() {
    let mut stream = Stream::default();
    let data = frame(
        json!({"choices":[{"index":0,"delta":{"content":"aß"},"finish_reason":null}]}),
    ) + &frame(
        json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"c1","type":"function", "function":{"name":"uppercase","arguments":"{\"value\":"}}]},"finish_reason":null}]}),
    ) + &frame(
        json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"hello\"}"}}]},"finish_reason":"tool_calls"}]}),
    ) + &frame(usage())
        + "data: [DONE]\r\n\r\n";
    let mut progress = String::new();
    for byte in data.bytes() {
        progress.push_str(&stream.feed(&[byte]).unwrap());
    }
    let reply = stream.finish(&request()).unwrap();
    assert_eq!(progress, "aß");
    assert_eq!(reply.text, "aß");
    assert_eq!(reply.calls[0].arguments, r#""hello""#);
    assert_eq!(reply.calls[0].id, "c1");
    assert_eq!(
        (reply.usage.input_tokens, reply.usage.output_tokens),
        (27, 7)
    );
}
#[test]
fn incomplete_and_malformed_streams_cannot_be_successful_replies() {
    let mut stream = Stream::default();
    assert_eq!(
        stream.feed(b"data: not-json\n\n").unwrap_err().code,
        ErrorCode::InvalidReply
    );
    let mut stream = Stream::default();
    stream
        .feed(frame(json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]})).as_bytes())
        .unwrap();
    assert_eq!(
        stream.finish(&request()).unwrap_err().code,
        ErrorCode::InvalidReply
    );
    let mut stream = Stream::default();
    stream
        .feed(
            (frame(json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}))
                + "data: [DONE]\n\n")
                .as_bytes(),
        )
        .unwrap();
    assert_eq!(
        stream.finish(&request()).unwrap_err().code,
        ErrorCode::InvalidReply
    );
}
#[test]
fn unmatched_tool_history_and_zero_limits_reject_before_transport() {
    let mut request = request();
    request.history.push(Message {
        role: Role::Tool,
        text: "x".into(),
        calls: vec![],
        call_id: "missing".into(),
        error: None,
    });
    assert_eq!(
        request::encode(&request, "fixture").unwrap_err().code,
        ErrorCode::InvalidInput
    );
    request.history.pop();
    request.limits.http_timeout_ms = Some(0);
    assert_eq!(
        request::encode(&request, "fixture").unwrap_err().code,
        ErrorCode::InvalidInput
    );
}
