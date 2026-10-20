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
        Message {
            role: Role::Assistant,
            text: String::new(),
            calls: vec![
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
            call_id: String::new(),
            error: None,
        },
        Message {
            role: Role::Tool,
            text: "HELLO".into(),
            calls: vec![],
            call_id: "c1".into(),
            error: None,
        },
        Message {
            role: Role::Tool,
            text: String::new(),
            calls: vec![],
            call_id: "c2".into(),
            error: Some("failed".into()),
        },
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

fn bound_http_choice(authenticated: bool) -> phenix_provider_contracts::HttpModelBinding {
    use phenix_provider_contracts::{
        bind_http_model, validate_model_selection, AuthenticationKind, AuthenticationMethod,
        AuthenticationStatus, ConnectionDescriptor, CredentialProfile, CredentialProfileStatus,
        ModelOrigin, ModelPage, ModelSelection, ProtocolBinding, ProviderDescriptor, ProviderModel,
    };
    let endpoint = if authenticated {
        "https://api.openai.com/v1/chat/completions"
    } else {
        "http://127.0.0.1:8080/v1/chat/completions"
    };
    let provider = ProviderDescriptor {
        id: "fixture".into(),
        label: "Fixture".into(),
        connections: vec![ConnectionDescriptor {
            id: "api".into(),
            label: "API".into(),
            protocols: vec![ProtocolBinding {
                id: "openai-chat".into(),
                endpoint: Some(endpoint.into()),
            }],
            base_url: None,
            auth_methods: if authenticated {
                vec![AuthenticationMethod {
                    id: "key".into(),
                    label: "API key".into(),
                    kind: AuthenticationKind::ApiKey,
                    adapter: phenix_provider_contracts::AdapterBinding::from_typed("key", &())
                        .unwrap(),
                }]
            } else {
                Vec::new()
            },
            model_sources: Vec::new(),
        }],
    };
    let auth = AuthenticationStatus {
        provider_id: "fixture".into(),
        connection_id: "api".into(),
        profiles: if authenticated {
            vec![CredentialProfile {
                id: "primary".into(),
                label: "Primary".into(),
                method_id: "key".into(),
                status: CredentialProfileStatus::Authenticated,
            }]
        } else {
            Vec::new()
        },
    };
    let profile_id = authenticated.then(|| "primary".into());
    let page = ModelPage {
        profile_id: profile_id.clone(),
        models: vec![ProviderModel {
            id: "model-a".into(),
            label: "Model A".into(),
            connection_id: "api".into(),
            protocol_id: "openai-chat".into(),
            endpoint_id: "chat".into(),
            capabilities: vec!["tools".into()],
            origin: ModelOrigin::Discovered,
            selectable: true,
        }],
        next_cursor: None,
        revision: 2,
    };
    let selection = ModelSelection {
        provider_id: "fixture".into(),
        connection_id: "api".into(),
        profile_id,
        model_id: "model-a".into(),
        protocol_id: "openai-chat".into(),
        endpoint_id: "chat".into(),
        catalog_revision: 2,
    };
    let admitted = validate_model_selection(&provider, &auth, &page, &selection, true).unwrap();
    bind_http_model(&provider, admitted).unwrap()
}

#[test]
fn selected_http_config_is_bound_to_catalog_and_exact_credential_presence() {
    use phenix_provider_contracts::CredentialSecret;
    let choice = bound_http_choice(true);
    let key = CredentialSecret::new(b"selected-account-token".to_vec()).unwrap();
    let config = request::Config::from_checked_binding(
        &choice,
        Some((choice.credential_identity().unwrap(), &key)),
        None,
    ).unwrap();
    assert_eq!(config.model, "model-a");
    assert_eq!(
        config.endpoint,
        "https://api.openai.com/v1/chat/completions"
    );
    assert_eq!(
        config.protocol,
        phenix_provider_protocols::Protocol::OpenaiChat
    );
    assert_eq!(config.credential.as_deref(), Some("selected-account-token"));

    // A valid secret from a *different* named account must not be bound to
    // this model merely because the bytes are available to the controller.
    let foreign = phenix_provider_contracts::CredentialIdentity::new(
        "fixture", "api", "other-account",
    )
    .unwrap();
    assert_eq!(
        request::Config::from_checked_binding(&choice, Some((&foreign, &key)), None)
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );

    assert_eq!(
        request::Config::from_checked_binding(&choice, None, None)
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
    let no_auth = bound_http_choice(false);
    let config = request::Config::from_checked_binding(&no_auth, None, None).unwrap();
    assert_eq!(config.credential, None);
    assert_eq!(
        request::Config::from_checked_binding(
            &no_auth,
            Some((choice.credential_identity().unwrap(), &key)),
            None,
        )
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
}

#[test]
fn checked_transport_rejects_non_utf8_secret_without_environment_fallback() {
    use phenix_provider_contracts::CredentialSecret;
    let binding = bound_http_choice(true);
    let key = CredentialSecret::new(vec![0xff, 0xfe]).unwrap();
    assert_eq!(
        request::Config::from_checked_binding(
            &binding,
            Some((binding.credential_identity().unwrap(), &key)),
            Some(512),
        )
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
}
