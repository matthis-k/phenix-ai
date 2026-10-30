//! Partial migration: real Basic loop behind a V2 action, not a new executor.
use super::*;
use phenix_basic_contracts::{ClientInput, RenderRequest, Submission};
use phenix_contract::graph::v2::*;
use phenix_kernel::graph::v2::{OperationSecurity, SourceOwnerPolicy, TrustedCompositionPolicy};

pub(super) fn run(mut composition: Composition, frontend: &str, output: &Path) {
    const OWNER: &str = "phenix.basic.agent";
    const SOURCE: &str = "phenix.basic.agent/prompt";
    const NODE: &str = "phenix.basic.agent/submit";
    const KEY: &str = "phenix.basic.input@1/submit";
    let label = PortSecurity {
        confidentiality: Classification::Confidential,
        integrity: IntegrityClass::Untrusted,
    };
    let execution = ExecutionPolicy {
        on_domain_error: DomainErrorPolicy::RouteErr,
    };
    let package = phenix_artifact::VerifiedPackage::verify_trusted_read_only(Path::new(
        &composition.artifacts[0],
    ))
    .unwrap();
    let contracts = phenix_artifact::VerifiedContractPackage::verify_trusted_read_only(Path::new(
        &composition.definitions[0],
    ))
    .unwrap();
    let schema = contracts
        .metadata()
        .definitions
        .iter()
        .find(|d| d.contract == "phenix.basic.input@1")
        .unwrap()
        .operations
        .iter()
        .find(|op| op.name == "submit")
        .unwrap();
    let graph = GraphContributionV2 {
        contributor: OWNER.into(),
        sources: vec![SourceDecl {
            id: SOURCE.into(),
            owner: OWNER.into(),
            kind: SourceKind::ExternalAction {
                action: "phenix.basic.submit".into(),
            },
            payload: schema.request.clone(),
            security: label,
        }],
        nodes: vec![
            NodeDecl {
                id: SOURCE.into(),
                kind: NodeKind::Source {
                    source: SOURCE.into(),
                },
                inputs: vec![],
                outputs: vec![OutputPort {
                    name: "out".into(),
                    schema: schema.request.clone(),
                    security: label,
                }],
                execution,
            },
            NodeDecl {
                id: NODE.into(),
                kind: NodeKind::Invoke {
                    selector: String::new(),
                    operation: KEY.into(),
                },
                inputs: vec![InputPort {
                    name: "in".into(),
                    schema: schema.request.clone(),
                    readiness: InputReadiness::One,
                    security: label,
                }],
                outputs: vec![
                    OutputPort {
                        name: "err".into(),
                        schema: schema.error.clone(),
                        security: label,
                    },
                    OutputPort {
                        name: "ok".into(),
                        schema: schema.response.clone(),
                        security: label,
                    },
                ],
                execution,
            },
        ],
        edges: vec![EdgeDecl {
            id: "phenix.basic.agent/start".into(),
            kind: EdgeKind::Flow {
                from: OutPortRef {
                    node: SOURCE.into(),
                    port: "out".into(),
                },
                to: InPortRef {
                    node: NODE.into(),
                    port: "in".into(),
                },
            },
        }],
        patches: vec![],
    };
    // Use the existing native acceptance technique: replace only the padded
    // offline graph section, then rehash and verify before loading ABI 3.
    let destination = output.join("v2-agent");
    fs::create_dir_all(destination.join("lib")).unwrap();
    fs::copy(
        Path::new(&composition.artifacts[0]).join("descriptor.bin"),
        destination.join("descriptor.bin"),
    )
    .unwrap();
    let mut bytes = fs::read(Path::new(&composition.artifacts[0]).join("lib/plugin.so")).unwrap();
    let original = package.graph().unwrap().encode().unwrap();
    let at = bytes
        .windows(original.len())
        .position(|b| b == original)
        .unwrap();
    let replacement = graph.normalized().unwrap().encode().unwrap();
    assert_eq!(&bytes[at - 4..at], &(original.len() as u32).to_be_bytes());
    let end = at + original.len().max(replacement.len());
    assert!(bytes[at + original.len()..end].iter().all(|b| *b == 0));
    bytes[at - 4..at].copy_from_slice(&(replacement.len() as u32).to_be_bytes());
    bytes[at..end].fill(0);
    bytes[at..at + replacement.len()].copy_from_slice(&replacement);
    assert_eq!(
        phenix_artifact::extract_graph_v2(&bytes).unwrap().unwrap(),
        graph.normalized().unwrap()
    );
    fs::write(destination.join("lib/plugin.so"), &bytes).unwrap();
    let mut record: serde_json::Value = serde_json::from_slice(
        &fs::read(Path::new(&composition.artifacts[0]).join("package.json")).unwrap(),
    )
    .unwrap();
    record["artifact_sha256"] = phenix_artifact::digest(&bytes).into();
    fs::write(
        destination.join("package.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    composition.artifacts[0] = destination.to_str().unwrap().into();
    // Keep canonical domain definitions, but do not mix their legacy action
    // declaration with the explicit V2 source.
    let definitions = output.join("v2-definitions");
    fs::create_dir_all(&definitions).unwrap();
    fs::copy(
        Path::new(&composition.definitions[0]).join("descriptor.bin"),
        definitions.join("descriptor.bin"),
    )
    .unwrap();
    let mut record: serde_json::Value = serde_json::from_slice(
        &fs::read(Path::new(&composition.definitions[0]).join("package.json")).unwrap(),
    )
    .unwrap();
    record["graph"] = serde_json::Value::Null;
    fs::write(
        definitions.join("package.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    composition.definitions = vec![definitions.to_str().unwrap().into()];
    composition.artifacts.push(frontend.into());
    let frontend =
        phenix_artifact::VerifiedPackage::verify_trusted_read_only(Path::new(frontend)).unwrap();
    let frontend = frontend.metadata().plugin.clone();
    for operation in ["decode", "render"] {
        composition
            .authority
            .operations
            .insert(format!("phenix.basic.frontend@1/{operation}"));
        composition.grants.push(ScopedGrant {
            caller: "root".into(),
            interface: "phenix.basic.frontend@1".into(),
            operation: operation.into(),
            resources: composition.authority.resources.clone(),
        });
    }
    composition.grants.push(ScopedGrant {
        caller: "root".into(),
        interface: "phenix.basic.journal@1".into(),
        operation: "read".into(),
        resources: composition.authority.resources.clone(),
    });
    // Isolate this increment from the legacy gate's database and state.
    let previous = std::env::var_os("PHENIX_BASIC_SQLITE_PATH");
    std::env::set_var("PHENIX_BASIC_SQLITE_PATH", output.join("v2-session.db"));
    let mut policy = TrustedCompositionPolicy::default();
    policy.source_owners.insert(
        OWNER.into(),
        SourceOwnerPolicy {
            owner_ceiling: Classification::Confidential,
            composition_ceiling: Classification::Confidential,
        },
    );
    policy.sources.insert(SOURCE.into(), label);
    policy.operations.insert(
        (OWNER.into(), KEY.into()),
        OperationSecurity {
            input: label,
            ok: label,
            err: label,
        },
    );
    let scope = ScopedRequest {
        authority: composition.authority.clone(),
        resource: composition.resource.clone(),
    };
    let kernel = Prepared::prepare_graph_v2(composition, &policy)
        .unwrap()
        .load()
        .unwrap();
    // No Session::open or authentication call precedes the first prompt.
    for (prompt, expected) in [("hello-v2", Some("mock:hello-v2")), ("model-error", None)] {
        let submission: Submission = invoke(
            &kernel,
            "phenix.basic.frontend@1/decode",
            &frontend,
            ClientInput {
                session: "s1".into(),
                line: prompt.into(),
            },
        )
        .unwrap()
        .unwrap();
        let (action, payload) = submission.as_dispatch().unwrap();
        let payload = phenix_basic_contracts::decode::<PromptSubmitted>(payload).unwrap();
        let mut run = kernel
            .begin_action_scoped(action, &payload.to_value(), scope.clone())
            .unwrap();
        let mut terminals = 0;
        for _ in 0..1000 {
            let status = run
                .pump(|update| {
                    if matches!(update.event, phenix_kernel::graph::GraphEvent::Finished(_)) {
                        terminals += 1;
                    }
                })
                .unwrap();
            if status == GraphStatus::Finished {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(terminals, 1);
        assert_eq!(
            run.completion(),
            Some(if expected.is_some() {
                GraphCompletion::Success
            } else {
                GraphCompletion::Domain
            })
        );
        assert_eq!(run.steps().len(), 1);
        match (&run.steps()[0].outcome, expected) {
            (Outcome::Success(value), Some(answer)) => {
                assert_eq!(RunResult::from_value(value.clone()).unwrap().answer, answer)
            }
            (Outcome::Domain(value), None) => assert_eq!(
                BasicError::from_value(value.clone()).unwrap().code,
                ErrorCode::ModelFailed
            ),
            other => panic!("unexpected V2 outcome: {other:?}"),
        }
        assert_eq!(
            run.pump(|_| panic!("duplicate terminal")).unwrap(),
            GraphStatus::Finished
        );
    }
    let events: EventPage = invoke(
        &kernel,
        "phenix.basic.output@1/read",
        OWNER,
        JournalQuery {
            session: "s1".into(),
            after: 0,
            limit: 100,
        },
    )
    .unwrap()
    .unwrap();
    let event_count = events.next;
    let lines: Vec<String> = invoke(
        &kernel,
        "phenix.basic.frontend@1/render",
        &frontend,
        RenderRequest {
            session: "s1".into(),
            events: events.events,
        },
    )
    .unwrap()
    .unwrap();
    assert!(lines.iter().any(|line| line == "answer: mock:hello-v2"));
    assert!(lines
        .iter()
        .any(|line| line.contains("scripted model failure")));
    let cancellation: Submission = invoke(
        &kernel,
        "phenix.basic.frontend@1/decode",
        &frontend,
        ClientInput {
            session: "s1".into(),
            line: "/cancel".into(),
        },
    )
    .unwrap()
    .unwrap();
    assert!(matches!(cancellation, Submission::Cancel));
    let mut cancelled = kernel
        .begin_action_scoped(
            "phenix.basic.submit",
            &PromptSubmitted {
                session: "s1".into(),
                text: "not-started".into(),
                limits: Limits::default(),
            }
            .to_value(),
            scope.clone(),
        )
        .unwrap();
    cancelled.cancel().unwrap();
    assert_eq!(cancelled.pump(|_| {}).unwrap(), GraphStatus::Finished);
    assert_eq!(cancelled.completion(), Some(GraphCompletion::Cancelled));
    assert!(cancelled.steps().is_empty());
    drop(cancelled);
    let unchanged: EventPage = invoke(
        &kernel,
        "phenix.basic.output@1/read",
        OWNER,
        JournalQuery {
            session: "s1".into(),
            after: event_count,
            limit: 100,
        },
    )
    .unwrap()
    .unwrap();
    assert!(
        unchanged.events.is_empty(),
        "cancelled-before-dispatch prompt produced output"
    );
    assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
    for cancel in [false, true] {
        use phenix_basic_contracts::EventKind;
        let before: EventPage = invoke(
            &kernel,
            "phenix.basic.output@1/read",
            OWNER,
            JournalQuery {
                session: "s1".into(),
                after: 0,
                limit: 100,
            },
        )
        .unwrap()
        .unwrap();
        let fds = fs::read_dir("/proc/self/fd").unwrap().count();
        std::env::set_var("PHENIX_SCRIPTED_MODEL_CONTROL", "hold");
        let mut run = kernel
            .begin_action_scoped(
                "phenix.basic.submit",
                &PromptSubmitted {
                    session: "s1".into(),
                    text: "controlled in-flight".into(),
                    limits: Limits::default(),
                }
                .to_value(),
                scope.clone(),
            )
            .unwrap();
        let mut terminals = 0;
        let mut observe = |update: &phenix_kernel::graph::GraphUpdate<'_>| {
            assert!(
                !matches!(
                    update.event,
                    phenix_kernel::graph::GraphEvent::NodeFinished(_)
                ),
                "protected result exposed to observer"
            );
            if matches!(update.event, phenix_kernel::graph::GraphEvent::Finished(_)) {
                assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
                assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds);
                terminals += 1;
            }
        };
        assert_eq!(run.pump(&mut observe).unwrap(), GraphStatus::Pending);
        assert!(run.steps().is_empty());
        assert!(run.completion().is_none());
        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 2);
        assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds + 1);
        // This read uses the live graph's authenticated root, not a second root.
        let Outcome::Success(value) = run
            .invoke_selected(
                &selection("phenix.basic.journal@1/read", "phenix.basic.event-journal"),
                &JournalQuery {
                    session: "s1".into(),
                    after: before.next,
                    limit: 100,
                }
                .to_value(),
            )
            .unwrap()
        else {
            panic!("pending journal read failed")
        };
        let admitted = EventPage::from_value(value).unwrap();
        assert_eq!(
            admitted
                .events
                .iter()
                .map(|event| event.kind.clone())
                .collect::<Vec<_>>(),
            vec![EventKind::PromptAccepted, EventKind::ModelStarted]
        );
        if cancel {
            run.cancel().unwrap();
            assert_eq!(run.pump(&mut observe).unwrap(), GraphStatus::Draining);
            assert!(run.completion().is_none());
            assert!(
                kernel.inspect()["pending"]["physical_leases"]
                    .as_u64()
                    .unwrap()
                    > 0
            );
            assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds + 1);
            assert!(
                kernel.shutdown().is_err(),
                "shutdown accepted a retained worker"
            );
            std::env::set_var("PHENIX_SCRIPTED_MODEL_CONTROL", "drain");
            for _ in 0..8 {
                if run.pump(&mut observe).unwrap() == GraphStatus::Finished {
                    break;
                }
            }
            assert_eq!(run.completion(), Some(GraphCompletion::Cancelled));
            assert_eq!(
                run.pump(|_| panic!("duplicate cancelled event")).unwrap(),
                GraphStatus::Finished
            );
        } else {
            std::env::set_var("PHENIX_SCRIPTED_MODEL_CONTROL", "fail");
            let failure = run.pump(&mut observe).unwrap_err();
            assert_eq!(failure.code, Code::PluginFailed);
            assert_eq!(failure.message, "controlled scripted Model failure");
            assert_eq!(run.completion(), Some(GraphCompletion::Failure(&failure)));
            assert_eq!(
                run.pump(|_| panic!("duplicate failed event")).unwrap_err(),
                failure
            );
        }
        assert_eq!(terminals, 1);
        assert!(
            run.steps().is_empty(),
            "cancelled/kernel failure became a domain result"
        );
        drop(run);
        std::env::remove_var("PHENIX_SCRIPTED_MODEL_CONTROL");
        let after: EventPage = invoke(
            &kernel,
            "phenix.basic.output@1/read",
            OWNER,
            JournalQuery {
                session: "s1".into(),
                after: admitted.next,
                limit: 100,
            },
        )
        .unwrap()
        .unwrap();
        if cancel {
            assert!(
                after.events.is_empty(),
                "revoked cleanup or late success wrote output"
            );
        } else {
            assert_eq!(after.events.len(), 1);
            assert_eq!(after.events[0].kind, EventKind::Failure);
            assert_eq!(after.events[0].text, "Model execution failed");
            let snapshot: phenix_basic_contracts::SessionSnapshot = invoke(
                &kernel,
                "phenix.basic.session@1/open",
                "phenix.basic.session",
                Scope {
                    session: "s1".into(),
                },
            )
            .unwrap()
            .unwrap();
            assert_eq!(snapshot.status, phenix_basic_contracts::RunStatus::Failed);
        }
        let rendered: Vec<String> = invoke(
            &kernel,
            "phenix.basic.frontend@1/render",
            &frontend,
            RenderRequest {
                session: "s1".into(),
                events: after.events,
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            rendered,
            if cancel {
                vec![]
            } else {
                vec!["error: Model execution failed".to_owned()]
            }
        );
        let unchanged: EventPage = invoke(
            &kernel,
            "phenix.basic.output@1/read",
            OWNER,
            JournalQuery {
                session: "s1".into(),
                after: after.next,
                limit: 100,
            },
        )
        .unwrap()
        .unwrap();
        assert!(unchanged.events.is_empty());
        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
        assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds);
    }
    kernel.shutdown().unwrap();
    match previous {
        Some(path) => std::env::set_var("PHENIX_BASIC_SQLITE_PATH", path),
        None => std::env::remove_var("PHENIX_BASIC_SQLITE_PATH"),
    }
}
