use crate::{request, stream::Stream};
use phenix_basic_contracts::{
    error, BasicError, ContextItem, ErrorCode, Limits, Message, ModelRequest, Skill, ToolCall,
    ToolInfo,
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

fn bound_http_choice(authenticated: bool) -> phenix_provider_contracts::HttpModelBinding {
    bound_http_choice_with_model(authenticated, "model-a")
}

fn bound_http_choice_with_model(
    authenticated: bool,
    selected_model: &str,
) -> phenix_provider_contracts::HttpModelBinding {
    bound_http_choice_with_kind(
        authenticated,
        selected_model,
        phenix_provider_contracts::AuthenticationKind::ApiKey,
    )
}

fn bound_http_choice_with_kind(
    authenticated: bool,
    selected_model: &str,
    kind: phenix_provider_contracts::AuthenticationKind,
) -> phenix_provider_contracts::HttpModelBinding {
    bound_http_choice_with_kind_endpoint(authenticated, selected_model, kind, None)
}

fn bound_http_choice_with_kind_endpoint(
    authenticated: bool,
    selected_model: &str,
    kind: phenix_provider_contracts::AuthenticationKind,
    selected_endpoint: Option<&str>,
) -> phenix_provider_contracts::HttpModelBinding {
    use phenix_provider_contracts::{
        bind_http_model, validate_model_selection, AuthenticationMethod, AuthenticationStatus,
        ConnectionDescriptor, CredentialProfile, CredentialProfileStatus, ModelOrigin, ModelPage,
        ModelSelection, ProtocolBinding, ProviderDescriptor, ProviderModel,
    };
    let endpoint = selected_endpoint.unwrap_or(if authenticated {
        "https://api.openai.com/v1/chat/completions"
    } else {
        "http://127.0.0.1:8080/v1/chat/completions"
    });
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
                    kind: kind.clone(),
                    adapter: if kind == phenix_provider_contracts::AuthenticationKind::ApiKey {
                        phenix_provider_contracts::AdapterBinding::from_typed(
                            "key",
                            &phenix_provider_contracts::ApiKeyOptions {
                                environment_variable: None,
                                interactive: true,
                                presentation: phenix_provider_contracts::ApiKeyPresentation::Bearer,
                            },
                        )
                        .unwrap()
                    } else {
                        phenix_provider_contracts::AdapterBinding::from_typed("key", &()).unwrap()
                    },
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
            id: selected_model.into(),
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
        model_id: selected_model.into(),
        protocol_id: "openai-chat".into(),
        endpoint_id: "chat".into(),
        catalog_revision: 2,
    };
    let admitted = validate_model_selection(&provider, &auth, &page, &selection, true).unwrap();
    bind_http_model(&provider, admitted).unwrap()
}

// Use the same checked native handoff as the runtime controller. Fixture
// credentials are copied here because HttpModelInstall consumes its secret.
fn selected_config(
    binding: &phenix_provider_contracts::HttpModelBinding,
    credential: Option<(
        &phenix_provider_contracts::CredentialIdentity,
        &phenix_provider_contracts::CredentialSecret,
    )>,
    provider_max_tokens: Option<u64>,
) -> Result<request::Config, BasicError> {
    use phenix_provider_contracts::{CredentialSecret, HttpModelInstall, ProviderErrorCode};
    let credential = credential.map(|(identity, secret)| {
        (
            identity.clone(),
            CredentialSecret::new(secret.expose_for_authorized_provider().to_vec()).unwrap(),
        )
    });
    let install = HttpModelInstall::from_checked(
        binding,
        "selected-session".into(),
        credential,
        provider_max_tokens,
    )
    .map_err(|problem| {
        error(
            if problem.code == ProviderErrorCode::Denied {
                ErrorCode::Denied
            } else {
                ErrorCode::InvalidInput
            },
            problem.message,
        )
    })?;
    request::Config::from_install(install)
}

#[test]
fn selected_http_config_is_bound_to_catalog_and_exact_credential_presence() {
    use phenix_provider_contracts::CredentialSecret;
    let choice = bound_http_choice(true);
    let key = CredentialSecret::new(b"selected-account-token".to_vec()).unwrap();
    let config = selected_config(
        &choice,
        Some((choice.credential_identity().unwrap(), &key)),
        None,
    )
    .unwrap();
    assert_eq!(config.model, "model-a");
    assert_eq!(
        config.endpoint,
        "https://api.openai.com/v1/chat/completions"
    );
    assert_eq!(
        config.protocol,
        phenix_provider_protocols::Protocol::OpenaiChat
    );
    let held: &CredentialSecret = config.credential.as_ref().expect("checked token");
    assert_eq!(
        held.expose_for_authorized_provider(),
        b"selected-account-token"
    );
    assert!(!format!("{config:?}").contains("selected-account-token"));

    // A valid secret from a *different* named account must not be bound to
    // this model merely because the bytes are available to the controller.
    let foreign =
        phenix_provider_contracts::CredentialIdentity::new("fixture", "api", "other-account")
            .unwrap();
    assert_eq!(
        selected_config(&choice, Some((&foreign, &key)), None)
            .unwrap_err()
            .code,
        ErrorCode::Denied
    );

    assert_eq!(
        selected_config(&choice, None, None).unwrap_err().code,
        ErrorCode::Denied
    );
    let no_auth = bound_http_choice(false);
    let config = selected_config(&no_auth, None, None).unwrap();
    assert!(config.credential.is_none());
    assert_eq!(
        selected_config(
            &no_auth,
            Some((choice.credential_identity().unwrap(), &key)),
            None,
        )
        .unwrap_err()
        .code,
        ErrorCode::Denied
    );
}

#[test]
fn checked_transport_rejects_non_utf8_secret_without_environment_fallback() {
    use phenix_provider_contracts::CredentialSecret;
    let binding = bound_http_choice(true);
    let key = CredentialSecret::new(vec![0xff, 0xfe]).unwrap();
    assert_eq!(
        selected_config(
            &binding,
            Some((binding.credential_identity().unwrap(), &key)),
            Some(512),
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidInput
    );
}

#[test]
fn checked_model_configuration_rejects_zero_provider_budget_and_blank_model() {
    use phenix_provider_contracts::CredentialSecret;
    let choice = bound_http_choice(true);
    let key = CredentialSecret::new(b"selected-account-token".to_vec()).unwrap();
    assert_eq!(
        selected_config(
            &choice,
            Some((choice.credential_identity().unwrap(), &key)),
            Some(0),
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidInput
    );

    // A provider plugin must not get to execute a catalog entry whose
    // identifier cannot name any concrete model, even if it claims selectable.
    let blank = bound_http_choice_with_model(false, "  ");
    assert_eq!(
        selected_config(&blank, None, None).unwrap_err().code,
        ErrorCode::InvalidInput
    );
}

#[test]
fn checked_http_adapter_accepts_verified_bearer_methods_but_rejects_unknown_auth() {
    use phenix_provider_contracts::{AuthenticationKind, CredentialSecret};
    for kind in [
        AuthenticationKind::OAuthAuthorizationCode,
        AuthenticationKind::OAuthDeviceCode,
    ] {
        let choice = bound_http_choice_with_kind(true, "model-a", kind);
        let token = CredentialSecret::new(b"authorized-oauth-bearer".to_vec()).unwrap();
        let config = selected_config(
            &choice,
            Some((choice.credential_identity().unwrap(), &token)),
            None,
        )
        .unwrap();
        assert_eq!(
            config
                .credential
                .as_ref()
                .unwrap()
                .expose_for_authorized_provider(),
            b"authorized-oauth-bearer"
        );
        assert!(!format!("{config:?}").contains("authorized-oauth-bearer"));
    }
    let plugin = bound_http_choice_with_kind(true, "model-a", AuthenticationKind::Plugin);
    let token = CredentialSecret::new(b"unrecognized-plugin-secret".to_vec()).unwrap();
    assert_eq!(
        selected_config(
            &plugin,
            Some((plugin.credential_identity().unwrap(), &token)),
            None,
        )
        .unwrap_err()
        .code,
        ErrorCode::Denied
    );
    let local = bound_http_choice(false);
    assert!(selected_config(&local, None, None).is_ok());
}

#[test]
fn native_http_oauth_install_rejects_foreign_account_before_transport() {
    use phenix_provider_contracts::{
        AuthenticationKind, CredentialIdentity, CredentialSecret, HttpModelInstall,
        ProviderErrorCode,
    };
    for kind in [
        AuthenticationKind::OAuthAuthorizationCode,
        AuthenticationKind::OAuthDeviceCode,
    ] {
        let binding = bound_http_choice_with_kind(true, "selected-model", kind);
        let expected = binding.credential_identity().unwrap().clone();
        let install = HttpModelInstall::from_checked(
            &binding,
            "session-1".into(),
            Some((
                expected.clone(),
                CredentialSecret::new(b"authorized-bearer-token".to_vec()).unwrap(),
            )),
            None,
        )
        .unwrap();
        let config = request::Config::from_install(install).unwrap();
        assert_eq!(
            config
                .credential
                .as_ref()
                .unwrap()
                .expose_for_authorized_provider(),
            b"authorized-bearer-token"
        );
        assert!(!format!("{config:?}").contains("authorized-bearer-token"));

        let foreign = CredentialIdentity::new("fixture", "api", "another-account").unwrap();
        assert_ne!(foreign, expected);
        let error = HttpModelInstall::from_checked(
            &binding,
            "session-1".into(),
            Some((
                foreign,
                CredentialSecret::new(b"foreign-bearer-token".to_vec()).unwrap(),
            )),
            None,
        )
        .unwrap_err();
        assert_eq!(error.code, ProviderErrorCode::Denied);
    }
}

#[test]
fn preset_http_credentials_use_clearing_secret_storage_and_reject_bad_inputs() {
    use phenix_provider_contracts::CredentialSecret;
    let key: CredentialSecret = request::configured_credential(Ok("fixture-secret".into()))
        .unwrap()
        .expect("configured credential");
    assert_eq!(key.expose_for_authorized_provider(), b"fixture-secret");
    assert!(
        request::configured_credential(Err(std::env::VarError::NotPresent))
            .unwrap()
            .is_none()
    );
    for invalid in ["", "line\nfeed", "bad\0nul"] {
        assert_eq!(
            request::configured_credential(Ok(invalid.into()))
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput
        );
    }
}

#[test]
fn checked_http_binding_executes_live_local_sse_without_env_model_or_key_fallback() {
    use crate::transport;
    use phenix_provider_contracts::{AuthenticationKind, CredentialSecret};
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{atomic::AtomicBool, Arc, Mutex},
        time::Duration,
    };

    // Exercise the actual curl/SSE path for both OAuth grant types, an API
    // key and credential-free inference. All authenticated methods must send
    // exactly the selected bearer and never read the environment fallback.
    for auth_method in [
        None,
        Some(AuthenticationKind::ApiKey),
        Some(AuthenticationKind::OAuthAuthorizationCode),
        Some(AuthenticationKind::OAuthDeviceCode),
    ] {
        let authenticated = auth_method.is_some();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!(
            "http://127.0.0.1:{}/v1/chat/completions",
            listener.local_addr().unwrap().port()
        );
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(8)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(8)))
                .unwrap();
            let mut captured = Vec::new();
            let mut buffer = [0u8; 4096];
            let (headers_end, body_size) = loop {
                let bytes = stream.read(&mut buffer).unwrap();
                assert!(bytes > 0, "missing selected HTTP request");
                captured.extend_from_slice(&buffer[..bytes]);
                if let Some(index) = captured.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    let headers = std::str::from_utf8(&captured[..index]).unwrap();
                    let expected = headers
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .expect("HTTP request Content-Length");
                    if captured.len() >= index + 4 + expected {
                        break (index, expected);
                    }
                }
            };
            let headers = std::str::from_utf8(&captured[..headers_end]).unwrap();
            assert!(headers.starts_with("POST /v1/chat/completions HTTP/1.1"));
            assert_eq!(
                headers.contains("Authorization: Bearer selected-fixture-key"),
                authenticated,
                "only the checked selected account may supply an API key"
            );
            let body = &captured[headers_end + 4..headers_end + 4 + body_size];
            let payload: serde_json::Value = serde_json::from_slice(body).unwrap();
            assert_eq!(payload["model"], "selected-fixture-model");
            assert!(payload["messages"].is_array());

            let content = concat!(
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"selected-live-reply\"},",
                "\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}\n\n",
                "data: [DONE]\n\n"
            );
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                content.len(),
                content
            )
            .unwrap();
            stream.flush().unwrap();
        });

        let binding = bound_http_choice_with_kind_endpoint(
            authenticated,
            "selected-fixture-model",
            auth_method.unwrap_or(AuthenticationKind::ApiKey),
            Some(&endpoint),
        );
        let key = CredentialSecret::new(b"selected-fixture-key".to_vec()).unwrap();
        let credential = authenticated.then(|| (binding.credential_identity().unwrap(), &key));
        let config = selected_config(&binding, credential, None).unwrap();
        let turn = request();
        let body = phenix_provider_protocols::request::encode_protocol_with_max_tokens(
            &turn,
            &config.model,
            config.protocol,
            config.provider_max_tokens,
        )
        .unwrap();
        let progress = Arc::new(Mutex::new(String::new()));
        let reply = transport::run_with_notify(
            config,
            body,
            turn,
            Arc::clone(&progress),
            Arc::new(AtomicBool::new(false)),
            || {},
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(reply.text, "selected-live-reply");
        assert_eq!(&*progress.lock().unwrap(), "selected-live-reply");
    }
}
