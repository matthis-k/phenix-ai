//! Real pending native Model with independently hosted HTTP and journal providers.
use phenix_basic_contracts::{
    BasicError, ContextItem, ErrorCode, EventPage, JournalQuery, Limits, Message, ModelReply,
    ModelRequest, Skill, ToolCall, ToolInfo, ToolOutcome,
};
use phenix_contract::{Authority, Portable, Value};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection, SlotBinding},
    Call, Generations, Outcome, Poll, Retirement, Root,
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
fn selected_overlap(mut composition: Composition, mode: &str, output: &Path) {
    let mut generations = Generations::default();
    let mut ids = Vec::new();
    let mut roots = Vec::new();
    let mut tickets = Vec::new();
    let mut tokens = Vec::new();
    let mut destroyed = Vec::new();
    let control = selection(
        "phenix.provider.http-model-session@2/install",
        "phenix.model.http",
    );
    let release = selection(
        "phenix.provider.http-model-session@2/release",
        "phenix.model.http",
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    for name in ["a", "b"] {
        let session = format!("s-{name}");
        let resource = format!("phenix.session.{session}");
        composition.resource = Some(resource.clone());
        composition.authority.resources = BTreeSet::from([resource.clone()]);
        for grant in &mut composition.grants {
            grant.resources = composition.authority.resources.clone();
        }
        for operation in ["install", "release"] {
            let key = format!("phenix.provider.http-model-session@2/{operation}");
            composition.authority.operations.insert(key);
            if !composition.grants.iter().any(|g| g.operation == operation) {
                composition.grants.push(ScopedGrant {
                    caller: "root".into(),
                    interface: "phenix.provider.http-model-session@2".into(),
                    operation: operation.into(),
                    resources: composition.authority.resources.clone(),
                });
            }
        }
        let id = generations
            .install_candidate(Prepared::prepare(composition.clone()).unwrap(), name)
            .unwrap();
        generations.promote(&id).unwrap();
        destroyed.push(generations.test_kernel_destroyed_probe(&id).unwrap());
        let root = generations.admit_default_root().unwrap();
        // Exercise the public native wire schema without adding a conformance dependency.
        let install = Value::map([
            ("session", Value::String(session.clone())),
            (
                "endpoint",
                Value::String(format!(
                    "{}/{name}",
                    std::env::var("PHENIX_MODEL_HTTP_ENDPOINT").unwrap()
                )),
            ),
            ("model_id", Value::String(format!("selected-{name}"))),
            (
                "protocol_id",
                Value::String(
                    std::env::var("PHENIX_MODEL_HTTP_PROTOCOL")
                        .unwrap_or_else(|_| "openai-chat".into()),
                ),
            ),
            ("provider_max_tokens", Value::Null),
            (
                "authorization",
                Value::map([(
                    "ApiKey",
                    Value::map([
                        (
                            "identity",
                            Value::map([
                                ("provider_id", Value::String(format!("provider-{name}"))),
                                ("connection_id", Value::String("local".into())),
                                ("profile_id", Value::String(name.into())),
                            ]),
                        ),
                        (
                            "secret",
                            Value::Bytes(format!("selected-secret-{name}").into_bytes()),
                        ),
                        (
                            "presentation",
                            if std::env::var("PHENIX_MODEL_HTTP_PROTOCOL").as_deref()
                                == Ok("anthropic-messages")
                            {
                                Value::map([(
                                    "Header",
                                    Value::map([
                                        ("name", Value::String("x-api-key".into())),
                                        ("prefix", Value::String(String::new())),
                                    ]),
                                )])
                            } else {
                                Value::map([("Bearer", Value::Null)])
                            },
                        ),
                    ]),
                )]),
            ),
        ]);
        let token = match root.invoke_selected(&control, &install).unwrap() {
            Outcome::Success(value) => u64::from_value(value).unwrap(),
            Outcome::Domain(value) => panic!("selected install failed: {value:?}"),
        };
        assert!(matches!(
            root.invoke_selected(
                &release,
                &Value::map([
                    ("session", Value::String("foreign-session".into())),
                    ("installation", Value::U64(token)),
                ])
            )
            .unwrap(),
            Outcome::Domain(_)
        ));
        let mut req = request(&format!("{mode}-{name}"));
        req.session = session;
        let ticket = match root.begin(&req.to_value()).unwrap() {
            Call::Pending(ticket) => ticket,
            _ => panic!("selected worker did not start"),
        };
        ids.push(id);
        roots.push(root);
        tickets.push(ticket);
        tokens.push(token);
    }
    assert_ne!(ids[0], ids[1]);
    // Installation counters are instance-local, not globally unique ownership IDs.
    assert_eq!(tokens, vec![1, 1]);
    // Server receipt, not elapsed time, establishes that both transfers overlap.
    while !output.join("both-selected-received").exists() {
        assert!(
            Instant::now() < deadline,
            "selected overlap receipt deadline"
        );
        std::thread::yield_now();
    }
    for id in &ids {
        assert_eq!(
            generations.inspect_candidate(id).unwrap()["pins"]["physical_leases"],
            1
        );
        assert!(matches!(
            generations.retire(id).unwrap(),
            Retirement::Blocked { .. }
        ));
    }
    eprintln!("test-http-fixture: both-held");
    assert!(destroyed.iter().all(|probe| !probe()));
    let revoke = |index: usize| {
        assert!(matches!(
            roots[index]
                .invoke_selected(
                    &release,
                    &Value::map([
                        (
                            "session",
                            Value::String(format!("s-{}", if index == 0 { "a" } else { "b" }))
                        ),
                        ("installation", Value::U64(tokens[index])),
                    ])
                )
                .unwrap(),
            Outcome::Success(_)
        ));
    };
    if mode == "selected-cancel" {
        roots[0].cancel(&tickets[0]).unwrap();
    }
    fs::write(output.join("release-selected-a"), b"release").unwrap();
    for index in 0..2 {
        loop {
            assert!(Instant::now() < deadline, "selected drain deadline");
            match roots[index].poll(&tickets[index]).unwrap() {
                Poll::Ready(value) => {
                    assert_eq!(outcome(value.unwrap()).unwrap().text, "first-aß");
                    break;
                }
                Poll::Cancelled { quiescent: true } => {
                    assert_eq!((index, mode), (0, "selected-cancel"));
                    break;
                }
                _ => std::thread::yield_now(),
            }
        }
        roots[index].release(&tickets[index]).unwrap();
        assert_eq!(
            generations.inspect_candidate(&ids[index]).unwrap()["pins"]["physical_leases"],
            0
        );
        if index == 0 {
            assert!(matches!(roots[1].poll(&tickets[1]).unwrap(), Poll::Pending));
            assert_eq!(
                generations.inspect_candidate(&ids[1]).unwrap()["pins"]["physical_leases"],
                1
            );
            revoke(0);
            eprintln!("test-http-fixture: a-drained-b-held");
            fs::write(output.join("release-selected-b"), b"release").unwrap();
        }
    }
    revoke(1);
    eprintln!("test-http-fixture: both-released");
    // Retirement checks below concern root/default ownership, not worker leases.
    for id in &ids {
        let inspection = generations.inspect_candidate(id).unwrap();
        assert_eq!(inspection["pins"]["physical_leases"], 0);
        assert_eq!(inspection["runtime"]["pending"]["physical_leases"], 0);
        assert_eq!(inspection["pins"]["roots"], 1);
    }
    match generations.retire(&ids[0]).unwrap() {
        Retirement::Blocked { reasons, .. } => assert_eq!(reasons, ["pinned root admission"]),
        _ => panic!("live root failed to pin generation"),
    }
    drop(roots);
    for id in &ids {
        let inspection = generations.inspect_candidate(id).unwrap();
        assert_eq!(inspection["pins"]["physical_leases"], 0);
        assert_eq!(inspection["pins"]["roots"], 0);
    }
    eprintln!("test-http-fixture: roots-dropped");
    assert!(destroyed.iter().all(|probe| !probe()));
    assert!(matches!(
        generations.retire(&ids[0]).unwrap(),
        Retirement::Retired
    ));
    eprintln!("test-http-fixture: a-retired");
    assert!(destroyed[0]());
    assert!(!destroyed[1]());
    eprintln!(
        "test-http-fixture: kernel-destroyed s-a {}",
        ids[0].as_str()
    );
    match generations.retire(&ids[1]).unwrap() {
        Retirement::Blocked { reasons, .. } => assert_eq!(reasons, ["selected default"]),
        _ => panic!("default generation retired"),
    }
    drop(generations);
    eprintln!("test-http-fixture: generations-dropped");
    assert!(destroyed.iter().all(|probe| probe()));
    eprintln!(
        "test-http-fixture: kernel-destroyed s-b {}",
        ids[1].as_str()
    );
    fs::write(
        output.join(format!("{mode}.json")),
        serde_json::to_vec(&serde_json::json!({
            "selected_generation_overlap": true,
            "all_physical_leases_released_before_retirement": true,
            "all_roots_released_before_successful_retirement": true,
            "final_kernel_reference_destructor_observed": true,
            "destroyed_kernel_generations": {"s-a": ids[0].as_str(), "s-b": ids[1].as_str()},
            "passed": true,
        }))
        .unwrap(),
    )
    .unwrap();
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
    let specs = [
        ("root", "phenix.basic.model@1/complete"),
        ("root", "phenix.basic.journal@1/read"),
        ("phenix.model.http", "phenix.basic.journal@1/append"),
        ("phenix.basic.event-journal", "phenix.basic.storage@1/read"),
        (
            "phenix.basic.event-journal",
            "phenix.basic.storage@1/transact",
        ),
    ];
    let composition = Composition {
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
    };
    if mode.starts_with("selected-") {
        selected_overlap(composition, mode, output);
        return;
    }
    let kernel = Prepared::prepare(composition.clone())
        .unwrap()
        .load()
        .unwrap();
    let root = kernel.admit_default_root().unwrap();
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
                        // The server holds this worker until cancel-requested.
                        // A separate loaded instance must finish independently.
                        let independent = Prepared::prepare(composition.clone())
                            .unwrap()
                            .load()
                            .unwrap();
                        let other = independent.admit_default_root().unwrap();
                        let other_ticket = match other.begin(&request("multi").to_value()).unwrap()
                        {
                            Call::Pending(ticket) => ticket,
                            Call::Ready(_) => panic!("independent HTTP did not start a worker"),
                        };
                        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
                        assert_eq!(independent.inspect()["pending"]["physical_leases"], 1);
                        assert!(matches!(
                            root.begin(&request(mode).to_value()),
                            Err(failure) if failure.code == phenix_contract::Code::ReentrancyRejected
                        ));
                        loop {
                            assert!(Instant::now() < deadline, "independent HTTP deadline");
                            match other.poll(&other_ticket).unwrap() {
                                Poll::Ready(value) => {
                                    assert_eq!(outcome(value.unwrap()).unwrap().calls.len(), 2);
                                    other.release(&other_ticket).unwrap();
                                    break;
                                }
                                Poll::Pending => std::thread::sleep(Duration::from_millis(5)),
                                _ => panic!("independent worker unexpectedly cancelled"),
                            }
                        }
                        assert_eq!(independent.inspect()["pending"]["physical_leases"], 0);
                        drop(other);
                        independent.shutdown().unwrap();
                        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
                        before_cancel = page.next;
                        assert!(
                            !root.cancel(&ticket).unwrap(),
                            "worker was incorrectly reported quiescent"
                        );
                        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
                        assert_eq!(
                            kernel.shutdown().unwrap_err().code,
                            phenix_contract::Code::ScopeRejected
                        );
                        assert!(matches!(
                            root.begin(&request(mode).to_value()),
                            Err(failure) if failure.code == phenix_contract::Code::ReentrancyRejected
                        ));
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
        "stream" | "tls" => {
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
