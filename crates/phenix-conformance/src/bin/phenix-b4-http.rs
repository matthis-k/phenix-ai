//! Real pending native Model with independently hosted HTTP and journal providers.
use phenix_basic_contracts::{
    BasicError, ContextItem, ErrorCode, EventPage, JournalQuery, Limits, Message, ModelReply,
    ModelRequest, Skill, ToolCall, ToolInfo, ToolOutcome,
};
use phenix_contract::{Authority, Portable};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection, SlotBinding},
    Call, Outcome, Poll, Root,
};
use std::{
    collections::BTreeSet,
    fs,
    path::Path,
    time::{Duration, Instant},
};

fn selection(key: &str, provider: &str) -> Selection {
    let (interface, operation) = key.split_once('/').unwrap();
    Selection {
        interface: interface.into(),
        operation: operation.into(),
        provider: provider.into(),
    }
}
fn journal(root: &Root<'_>) -> EventPage {
    let outcome = root
        .invoke_selected(
            &selection("phenix.basic.journal@1/read", "phenix.basic.event-journal"),
            &JournalQuery {
                session: "s1".into(),
                after: 0,
                limit: 128,
            }
            .to_value(),
        )
        .unwrap();
    match outcome {
        Outcome::Success(value) => EventPage::from_value(value).unwrap(),
        Outcome::Domain(value) => panic!("journal failed {value:?}"),
    }
}
fn outcome(value: Outcome) -> Result<ModelReply, BasicError> {
    match value {
        Outcome::Success(value) => Ok(ModelReply::from_value(value).unwrap()),
        Outcome::Domain(value) => Err(BasicError::from_value(value).unwrap()),
    }
}
fn request(mode: &str) -> ModelRequest {
    ModelRequest {
        session: if mode == "denied" { "s2" } else { "s1" }.into(),
        run: "r1".into(),
        history: vec![
            Message::user("prior"),
            Message::assistant(
                String::new(),
                vec![
                    ToolCall {
                        id: "prior-1".into(),
                        tool: "uppercase".into(),
                        arguments: r#""old""#.into(),
                    },
                    ToolCall {
                        id: "prior-2".into(),
                        tool: "length".into(),
                        arguments: r#""old""#.into(),
                    },
                ],
            ),
            Message::tool("prior-1", ToolOutcome::Succeeded { text: "OLD".into() }),
            Message::tool(
                "prior-2",
                ToolOutcome::Failed {
                    output: String::new(),
                    reason: "prior tool failed".into(),
                },
            ),
            Message::user(mode),
        ],
        tools: ["uppercase", "length"]
            .into_iter()
            .map(|id| ToolInfo {
                id: id.into(),
                description: id.into(),
                arguments_schema: r#"{"type":"string"}"#.into(),
                idempotent: true,
            })
            .collect(),
        skills: vec![Skill {
            id: "rust".into(),
            description: "Rust".into(),
            instructions: "Use Rust".into(),
        }],
        context: vec![ContextItem {
            id: "repo".into(),
            text: "Phenix".into(),
        }],
        limits: Limits {
            model_tokens: Some(if mode == "limit" { 1 } else { 32 }),
            http_timeout_ms: Some(if mode == "timeout" {
                80
            } else if mode == "zero" {
                0
            } else {
                10_000
            }),
            ..Limits::default()
        },
    }
}

fn selected_install(endpoint: &str, session: &str) -> phenix_provider_contracts::HttpModelInstall {
    use phenix_provider_contracts::{
        bind_http_model, validate_model_selection, AuthenticationStatus, ConnectionDescriptor,
        HttpModelInstall, ModelOrigin, ModelPage, ModelSelection, ProtocolBinding,
        ProviderDescriptor, ProviderModel,
    };
    // Product fixture only. A production controller must query fresh plugin
    // registrations and authorization, not construct invented provider truth.
    let provider = ProviderDescriptor {
        id: "loopback-fixture".into(),
        label: "Loopback".into(),
        connections: vec![ConnectionDescriptor {
            id: "local".into(),
            label: "Local".into(),
            protocols: vec![ProtocolBinding {
                id: "openai-chat".into(),
                endpoint: Some(endpoint.into()),
            }],
            base_url: None,
            auth_methods: vec![],
            model_sources: vec![],
        }],
    };
    let authentication = AuthenticationStatus {
        provider_id: provider.id.clone(),
        connection_id: "local".into(),
        profiles: vec![],
    };
    let models = ModelPage {
        profile_id: None,
        models: vec![ProviderModel {
            id: "selected-fixture".into(),
            label: "Selected fixture".into(),
            connection_id: "local".into(),
            protocol_id: "openai-chat".into(),
            endpoint_id: "chat".into(),
            capabilities: vec![],
            origin: ModelOrigin::Discovered,
            selectable: true,
        }],
        next_cursor: None,
        revision: 1,
    };
    let selection = ModelSelection {
        provider_id: provider.id.clone(),
        connection_id: "local".into(),
        profile_id: None,
        model_id: "selected-fixture".into(),
        protocol_id: "openai-chat".into(),
        endpoint_id: "chat".into(),
        catalog_revision: 1,
    };
    let admitted =
        validate_model_selection(&provider, &authentication, &models, &selection, true).unwrap();
    let bound = bind_http_model(&provider, admitted).unwrap();
    HttpModelInstall::from_checked(&bound, session.into(), None, None).unwrap()
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(
        args.len(),
        8,
        "MODE HTTP JOURNAL SQLITE BASIC_DEFINITIONS PROVIDER_DEFINITIONS OUTPUT"
    );
    let mode = &args[1];
    let output = Path::new(&args[7]);
    fs::create_dir_all(output).unwrap();
    let resource = BTreeSet::from(["phenix.session.s1".to_owned()]);
    let mut specs = vec![
        ("root", "phenix.basic.model@1/complete"),
        ("root", "phenix.basic.journal@1/read"),
        ("phenix.model.http", "phenix.basic.journal@1/append"),
        ("phenix.basic.event-journal", "phenix.basic.storage@1/read"),
        (
            "phenix.basic.event-journal",
            "phenix.basic.storage@1/transact",
        ),
    ];
    if mode == "selected" {
        // Install is a separate native capability, scoped to this session.
        specs.push(("root", "phenix.provider.http-model-session@1/install"));
    }
    let kernel = Prepared::prepare(Composition {
        format: "phenix.composition.v3".into(),
        artifacts: args[2..5].to_vec(),
        definitions: vec![args[5].clone(), args[6].clone()],
        entry: selection("phenix.basic.model@1/complete", "phenix.model.http"),
        slots: vec![
            SlotBinding {
                caller: "phenix.model.http".into(),
                slot: "Journal".into(),
                index: 0,
                provider: "phenix.basic.event-journal".into(),
                export: "Journal".into(),
            },
            SlotBinding {
                caller: "phenix.basic.event-journal".into(),
                slot: "Storage".into(),
                index: 0,
                provider: "phenix.basic.storage-sqlite".into(),
                export: "Storage".into(),
            },
        ],
        grants: specs
            .iter()
            .map(|(caller, key)| {
                let (interface, operation) = key.split_once('/').unwrap();
                ScopedGrant {
                    caller: (*caller).into(),
                    interface: interface.into(),
                    operation: operation.into(),
                    resources: resource.clone(),
                }
            })
            .collect(),
        excluded: BTreeSet::new(),
        substitutions: vec![],
        authority: Authority {
            operations: specs.iter().map(|(_, key)| (*key).into()).collect(),
            resources: resource,
        },
        resource: Some("phenix.session.s1".into()),
    })
    .unwrap()
    .load()
    .unwrap();
    let root = kernel.admit_default_root().unwrap();
    if mode == "selected" {
        use phenix_provider_contracts::{ProviderError, ProviderErrorCode};
        let endpoint =
            std::env::var("PHENIX_SELECTED_HTTP_ENDPOINT").expect("selected loopback endpoint");
        let control = selection(
            "phenix.provider.http-model-session@1/install",
            "phenix.model.http",
        );
        // A caller with only the s1 session root cannot install s2's choice.
        let denied = selected_install(&endpoint, "s2");
        match root.invoke_selected(&control, &denied.to_value()).unwrap() {
            Outcome::Domain(value) => {
                assert_eq!(
                    ProviderError::from_value(value).unwrap().code,
                    ProviderErrorCode::Denied
                );
            }
            Outcome::Success(_) => panic!("cross-session native install was accepted"),
        }
        let chosen = selected_install(&endpoint, "s1");
        assert!(matches!(
            root.invoke_selected(&control, &chosen.to_value()).unwrap(),
            Outcome::Success(_)
        ));
    }
    let call = root.begin(&request(mode).to_value()).unwrap();
    let mut live = false;
    let mut cancelled = false;
    let mut before_cancel = 0;
    let deadline = Instant::now() + Duration::from_secs(20);
    let result = match call {
        Call::Ready(value) => outcome(value),
        Call::Pending(ticket) => loop {
            assert!(
                Instant::now() < deadline,
                "HTTP fixture deadline for {mode}"
            );
            let polled = root.poll(&ticket).unwrap();
            if (mode == "stream" || mode == "cancel") && !live {
                let page = journal(&root);
                if !page.events.is_empty() {
                    live = true;
                    assert!(
                        matches!(polled, Poll::Pending),
                        "progress arrived only at final completion"
                    );
                    if mode == "stream" {
                        fs::write(output.join("live-progress"), b"observed").unwrap();
                    } else {
                        before_cancel = page.next;
                        assert!(
                            !root.cancel(&ticket).unwrap(),
                            "worker was incorrectly reported quiescent"
                        );
                        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
                        fs::write(output.join("cancel-requested"), b"cancelled").unwrap();
                        cancelled = true;
                    }
                }
            }
            match polled {
                Poll::Ready(value) => {
                    root.release(&ticket).unwrap();
                    break outcome(value.unwrap());
                }
                Poll::Cancelled { quiescent: true } => {
                    assert!(cancelled);
                    assert_eq!(
                        journal(&root).next,
                        before_cancel,
                        "cancelled cleanup wrote progress"
                    );
                    root.release(&ticket).unwrap();
                    break Ok(ModelReply {
                        text: "cancelled".into(),
                        calls: vec![],
                        usage: Default::default(),
                    });
                }
                _ => std::thread::sleep(Duration::from_millis(5)),
            }
        },
    };
    match mode.as_str() {
        "stream" | "tls" | "selected" => {
            let reply = result.unwrap();
            assert_eq!(reply.text, "first-aß");
            assert_eq!(
                journal(&root)
                    .events
                    .iter()
                    .map(|event| event.text.as_str())
                    .collect::<String>(),
                reply.text
            );
            assert_eq!(
                (reply.usage.input_tokens, reply.usage.output_tokens),
                (27, 7)
            );
            if mode == "stream" {
                assert!(live);
            }
        }
        "multi" => {
            let reply = result.unwrap();
            assert_eq!(reply.calls.len(), 2);
            assert_eq!(reply.calls[0].id, "c1");
            assert_eq!(reply.calls[1].id, "c2");
            assert_eq!(reply.calls[0].arguments, r#""hello""#);
        }
        "cancel" => {
            assert!(live && cancelled);
            assert_eq!(result.unwrap().text, "cancelled");
        }
        "denied" => assert_eq!(result.unwrap_err().code, ErrorCode::Denied),
        "zero" | "bad-config" => assert_eq!(result.unwrap_err().code, ErrorCode::InvalidInput),
        "timeout" | "limit" => assert_eq!(result.unwrap_err().code, ErrorCode::TurnLimit),
        "status" | "redirect" | "tls-untrusted" | "provider-error" => {
            assert_eq!(result.unwrap_err().code, ErrorCode::ModelFailed)
        }
        "malformed" | "truncated" | "no-usage" => {
            assert_eq!(result.unwrap_err().code, ErrorCode::InvalidReply)
        }
        _ => panic!("unknown fixture mode"),
    }
    if mode == "selected" {
        // Consuming an admitted choice cannot silently fall back to the
        // unrelated legacy environment preset on a later Model turn.
        match root.begin(&request(mode).to_value()).unwrap() {
            Call::Ready(Outcome::Domain(value)) => {
                assert_eq!(
                    BasicError::from_value(value).unwrap().code,
                    ErrorCode::InvalidInput
                );
            }
            _ => panic!("consumed native choice unexpectedly started another model turn"),
        }
    }
    assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
    drop(root);
    kernel.shutdown().unwrap();
    fs::write(
        output.join(format!("{mode}.json")),
        serde_json::to_vec(&serde_json::json!({"case":mode,"pending_native":true,"passed":true}))
            .unwrap(),
    )
    .unwrap();
}
