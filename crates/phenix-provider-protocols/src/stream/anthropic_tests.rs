use super::*;
use phenix_basic_contracts::{Limits, Message, ModelRequest, Role, ToolInfo};
use serde_json::json;

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
        tools: vec![
            ToolInfo {
                id: "uppercase".into(),
                description: "Uppercase".into(),
                arguments_schema: r#"{"type":"string"}"#.into(),
                idempotent: true,
            },
            ToolInfo {
                id: "length".into(),
                description: "Length".into(),
                arguments_schema: r#"{"type":"string"}"#.into(),
                idempotent: true,
            },
        ],
        skills: vec![],
        context: vec![],
        limits: Limits::default(),
    }
}

fn decode(events: &[Value]) -> Result<Stream, BasicError> {
    let mut stream = Stream::new(crate::Protocol::AnthropicMessages);
    for event in events {
        for byte in format!(
            "event: {}\ndata: {event}\n\n",
            event["type"].as_str().unwrap()
        )
        .bytes()
        {
            stream.feed(&[byte])?;
        }
    }
    Ok(stream)
}

fn text_events() -> Vec<Value> {
    vec![
        json!({"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":25,"output_tokens":1}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":7}}),
        json!({"type":"message_stop"}),
    ]
}

fn tool_events() -> Vec<Value> {
    vec![
        json!({"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":25,"output_tokens":1}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"phase1-"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"uppercase","input":{}}}),
        json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"value\":"}}),
        json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"hello\"}"}}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":15}}),
        json!({"type":"message_stop"}),
    ]
}

#[test]
fn anthropic_text_stream_decodes_to_a_completed_reply() {
    let stream = decode(&text_events()).unwrap();
    assert_eq!(stream.text, "Hello");
    assert_eq!(stream.finish.as_deref(), Some("stop"));
    assert!(stream.done);
    let reply = stream.finish(&request()).unwrap();
    assert_eq!(reply.text, "Hello");
    assert!(reply.calls.is_empty());
    assert_eq!(
        (reply.usage.input_tokens, reply.usage.output_tokens),
        (25, 7)
    );
}

#[test]
fn anthropic_tool_calls_stream_to_typed_reply() {
    let stream = decode(&tool_events()).unwrap();
    assert_eq!(stream.text, "phase1-");
    assert_eq!(stream.finish.as_deref(), Some("tool_calls"));
    let reply = stream.finish(&request()).unwrap();
    assert_eq!(reply.text, "phase1-");
    assert_eq!(reply.calls.len(), 1);
    assert_eq!(reply.calls[0].id, "toolu_1");
    assert_eq!(reply.calls[0].tool, "uppercase");
    assert_eq!(reply.calls[0].arguments, r#""hello""#);
}

#[test]
fn anthropic_rejects_ordering_lifecycle_and_usage_corruption() {
    for index in [0, 1, 2, 3, 4, 5] {
        let mut events = text_events();
        match index {
            0 => events[index]["message"]["usage"]["input_tokens"] = json!(-1),
            1 => events[index]["index"] = json!(1),
            2 => events[index]["delta"]["type"] = json!("input_json_delta"),
            3 => events[index]["index"] = json!(9),
            4 => events[index]["usage"]["output_tokens"] = json!(null),
            5 => events[index]["type"] = json!("message_stop_again"),
            _ => unreachable!(),
        }
        assert!(decode(&events).is_err(), "corruption at {index}");
    }
    for removed in [0, 1, 3, 4] {
        let mut events = text_events();
        events.remove(removed);
        assert!(
            decode(&events).is_err(),
            "missing lifecycle event {removed}"
        );
    }
    // Removing the terminal message_stop leaves an incomplete stream that
    // only finish() can reject.
    let mut events = text_events();
    events.pop();
    let stream = decode(&events).unwrap();
    assert_eq!(
        stream.finish(&request()).unwrap_err().code,
        ErrorCode::InvalidReply
    );
    let mut events = text_events();
    events.push(events.last().unwrap().clone());
    assert!(decode(&events).is_err());
}

#[test]
fn anthropic_errors_are_redacted_and_max_tokens_is_not_success() {
    let mut stream = Stream::new(crate::Protocol::AnthropicMessages);
    let error = stream
        .anthropic_frame(r#"{"type":"error","error":{"type":"overloaded","message":"secret"}}"#)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ModelFailed);
    assert!(!error.message.contains("secret"));

    let mut events = text_events();
    events[4] = json!({"type":"message_delta","delta":{"stop_reason":"max_tokens","stop_sequence":null},"usage":{"output_tokens":32}});
    let mut stream = decode(&events[..5]).unwrap();
    assert_eq!(
        stream.anthropic_message_stop().unwrap_err().code,
        ErrorCode::TurnLimit
    );
}

#[test]
fn anthropic_unsupported_content_blocks_fail_closed() {
    let mut events = text_events();
    events[1] = json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}});
    match decode(&events) {
        Err(error) => assert_eq!(error.code, ErrorCode::ModelFailed),
        Ok(_) => panic!("unsupported content block was accepted"),
    }
}

#[test]
fn anthropic_request_encodes_system_tools_and_full_history() {
    use crate::request::encode_protocol;
    use phenix_basic_contracts::{ContextItem, Skill, ToolCall};
    let mut request = request();
    request.skills = vec![Skill {
        id: "rust".into(),
        description: "Rust".into(),
        instructions: "Use Rust".into(),
    }];
    request.context = vec![ContextItem {
        id: "repo".into(),
        text: "Phenix".into(),
    }];
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
    // Anthropic requires a positive output budget on every wire request.
    request.limits.model_tokens = Some(128);
    let encoded: Value = serde_json::from_slice(
        &encode_protocol(&request, "fixture", crate::Protocol::AnthropicMessages).unwrap(),
    )
    .unwrap();
    assert!(encoded["system"].as_str().unwrap().contains("Use Rust"));
    assert!(encoded["system"].as_str().unwrap().contains("Phenix"));
    assert_eq!(
        encoded["messages"][0],
        json!({"role":"user","content":"hello"})
    );
    assert_eq!(encoded["messages"][1]["content"][0]["type"], "tool_use");
    assert_eq!(
        encoded["messages"][1]["content"][0]["input"],
        json!({"value":"aß"})
    );
    assert_eq!(encoded["messages"][2]["content"][0]["tool_use_id"], "c1");
    assert_eq!(encoded["messages"][2]["content"][0]["is_error"], true);
    assert!(encoded["messages"][2]["content"][0]["content"]
        .as_str()
        .unwrap()
        .contains("tool failed"));
    assert_eq!(
        encoded["tools"][0]["input_schema"]["properties"]["value"]["type"],
        "string"
    );
    assert_eq!(encoded["stream"], true);
}
