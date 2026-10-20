use crate::{request, stream::Stream};
use phenix_basic_contracts::{
    ContextItem, ErrorCode, Limits, Message, ModelRequest, Skill, ToolCall, ToolInfo,
};
use serde_json::{json, Value};

fn request() -> ModelRequest {
    ModelRequest {
        session: "s1".into(),
        run: "r1".into(),
        history: vec![Message::user("hello")],
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
        Message::assistant(
            String::new(),
            vec![ToolCall {
                id: "c1".into(),
                tool: "uppercase".into(),
                arguments: r#""aß""#.into(),
            }],
        ),
        Message::tool(
            "c1",
            phenix_basic_contracts::ToolOutcome::Failed {
                output: String::new(),
                reason: "tool failed".into(),
            },
        ),
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
fn standard_sse_line_endings_metadata_and_bom_preserve_model_reply() {
    let mut stream = Stream::default();
    let data = format!(
        "\u{feff}: keepalive\r\nevent: message\r\nid: message-1\r\ndata: {}\r\n\r\n{}data: [DONE]\r\n\r\n",
        json!({"choices":[{"index":0,"delta":{"content":"aß"},"finish_reason":"stop"}]}),
        frame(usage()),
    );
    let mut streamed = String::new();
    for part in data.as_bytes().chunks(1) {
        streamed.push_str(&stream.feed(part).unwrap());
    }
    let reply = stream.finish(&request()).unwrap();
    assert_eq!(streamed, "aß");
    assert_eq!(reply.text, streamed);
    assert_eq!(reply.usage.input_tokens, 27);
}

#[test]
fn invalid_utf8_even_when_fragmented_is_not_silently_replaced() {
    let mut stream = Stream::default();
    stream.feed(b"data: \xc3").unwrap();
    assert_eq!(stream.feed(b"(").unwrap_err().code, ErrorCode::InvalidReply,);

    let mut stream = Stream::default();
    stream.feed(b"data: \xe2").unwrap();
    assert_eq!(
        stream.finish(&request()).unwrap_err().code,
        ErrorCode::InvalidReply,
    );
}

#[test]
fn standard_sse_parser_respects_portable_frame_bounds() {
    let mut stream = Stream::default();
    let oversized = vec![b'x'; phenix_sdk::MAX_BYTES + 1];
    assert_eq!(
        stream.feed(&oversized).unwrap_err().code,
        ErrorCode::InvalidReply,
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
    request.history.push(Message::tool(
        "missing",
        phenix_basic_contracts::ToolOutcome::Succeeded { text: "x".into() },
    ));
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

#[test]
fn http_model_endpoint_is_parsed_by_url_crate_before_curl() {
    assert_eq!(
        request::endpoint_url("https://[::1]:9443/completions?stream=1").unwrap(),
        "https://[::1]:9443/completions?stream=1"
    );
    assert_eq!(
        request::endpoint_url("http://127.0.0.1:8800").unwrap(),
        "http://127.0.0.1:8800/"
    );
    assert_eq!(
        request::endpoint_url("https://example.org/@scope?mention=@alice").unwrap(),
        "https://example.org/@scope?mention=@alice"
    );
    // A URL parser can normalize empty userinfo away, but credentials
    // must never be sent to an endpoint with explicit userinfo.
    assert_eq!(
        request::endpoint_url("http://@localhost/completions")
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
    for bad in [
        "",
        "example.com/completions",
        "file:///etc/passwd",
        "ftp://localhost/file",
        "http://",
        "http://localhost:8800",
        "http://192.0.2.42/completions",
        "http://10.0.0.8/completions",
        "http://0.0.0.0:8080/completions",
        "http://user@localhost/completions",
        "http://%40@localhost/completions",
        "https://user:password@localhost/completions",
        "https://localhost/completions#fragment",
        "http://localhost:70000/completions",
        "https://localhost/\ncompletions",
        "http://localhost/?q=1\r\n",
    ] {
        assert_eq!(
            request::endpoint_url(bad).unwrap_err().code,
            ErrorCode::InvalidInput,
            "unexpected endpoint acceptance: {bad:?}"
        );
    }
}

#[test]
fn anthropic_wire_max_tokens_required_and_provider_bound_precede_transport() {
    use phenix_provider_protocols::{request::encode_protocol_with_max_tokens, Protocol};
    let request = request();
    let err = encode_protocol_with_max_tokens(
        &request,
        "claude-fixture",
        Protocol::AnthropicMessages,
        None,
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);

    let encoded: Value = serde_json::from_slice(
        &encode_protocol_with_max_tokens(
            &request,
            "claude-fixture",
            Protocol::AnthropicMessages,
            Some(1024),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(encoded["max_tokens"], 1024);

    let mut limited = request.clone();
    limited.limits.model_tokens = Some(128);
    let encoded: Value = serde_json::from_slice(
        &encode_protocol_with_max_tokens(
            &limited,
            "claude-fixture",
            Protocol::AnthropicMessages,
            Some(1024),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(encoded["max_tokens"], 128);

    limited.limits.model_tokens = Some(2048);
    let encoded: Value = serde_json::from_slice(
        &encode_protocol_with_max_tokens(
            &limited,
            "claude-fixture",
            Protocol::AnthropicMessages,
            Some(1024),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(encoded["max_tokens"], 1024);
    assert_eq!(
        encode_protocol_with_max_tokens(
            &limited,
            "claude-fixture",
            Protocol::AnthropicMessages,
            Some(0)
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidInput,
    );
    // A provider budget is not an implicit caller token limit on Chat/Responses.
    for protocol in [Protocol::OpenaiChat, Protocol::OpenaiResponses] {
        let body: Value = serde_json::from_slice(
            &encode_protocol_with_max_tokens(&request, "fixture", protocol, Some(1024)).unwrap(),
        )
        .unwrap();
        assert!(body.get("max_completion_tokens").is_none());
        assert!(body.get("max_output_tokens").is_none());
    }
}

#[test]
fn anthropic_groups_batched_tool_results_without_inventing_user_turns() {
    use phenix_provider_protocols::{request::encode_protocol_with_max_tokens, Protocol};
    let mut request = request();
    request.history.extend([
        Message::assistant(
            String::new(),
            vec![
                ToolCall {
                    id: "c1".into(),
                    tool: "uppercase".into(),
                    arguments: r#""hello""#.into(),
                },
                ToolCall {
                    id: "c2".into(),
                    tool: "uppercase".into(),
                    arguments: r#""world""#.into(),
                },
            ],
        ),
        Message::tool(
            "c1",
            phenix_basic_contracts::ToolOutcome::Succeeded {
                text: "HELLO".into(),
            },
        ),
        Message::tool(
            "c2",
            phenix_basic_contracts::ToolOutcome::Failed {
                output: String::new(),
                reason: "failed".into(),
            },
        ),
    ]);
    let body: Value = serde_json::from_slice(
        &encode_protocol_with_max_tokens(
            &request,
            "claude-fixture",
            Protocol::AnthropicMessages,
            Some(100),
        )
        .unwrap(),
    )
    .unwrap();
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3);
    let blocks = messages[2]["content"].as_array().unwrap();
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0]["tool_use_id"], "c1");
    assert_eq!(blocks[1]["tool_use_id"], "c2");
    assert_eq!(blocks[1]["is_error"], true);
}

#[test]
fn legacy_http_provider_max_tokens_parser_rejects_invalid_values() {
    let parse = crate::request::configured_max_tokens;
    assert_eq!(parse(Err(std::env::VarError::NotPresent)).unwrap(), None);
    assert_eq!(parse(Ok("32".into())).unwrap(), Some(32));
    for raw in ["", "0", "-1", "abc", " 32", "18446744073709551616"] {
        assert_eq!(
            parse(Ok(raw.into())).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }
    assert_eq!(
        parse(Err(std::env::VarError::NotUnicode(
            std::ffi::OsString::from("\u{fffd}")
        )))
        .unwrap_err()
        .code,
        ErrorCode::InvalidInput
    );
}
