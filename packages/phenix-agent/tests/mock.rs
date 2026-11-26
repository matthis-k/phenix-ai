use phenix_agent::{
    AgentEvent, EVENTS, ModelInput, PROMPT, ToolCall,
    mock::{self, MockModel, MockTool},
};
use phenix_graph::{InputKey, OutputKey, Value, runtime::RequestStatus};
use phenix_kernel::host::Generation;

fn turn(generation: &Generation, prompt: &str) -> Vec<AgentEvent> {
    let ingress = generation
        .compiled()
        .bind_ingress(PROMPT, InputKey::<String>::new(PROMPT))
        .unwrap();
    let capture = generation
        .compiled()
        .bind_capture(EVENTS, OutputKey::<AgentEvent>::new(EVENTS))
        .unwrap();
    generation
        .with_session(|session| {
            let request = session.begin_bound_request(None).unwrap();
            session
                .inject_bound_request(request, &ingress, prompt.to_owned())
                .unwrap();
            session.close_request(request).unwrap();
            let mut events = Vec::new();
            for _ in 0..64 {
                session.run_until_idle(1);
                assert!(session.failure().is_none());
                while let Some(packet) = session
                    .take_bound_request_output(request, &capture)
                    .unwrap()
                {
                    session
                        .validate_bound_capture(request, &capture, &packet)
                        .unwrap();
                    events.push(packet.value().borrow::<AgentEvent>().unwrap().clone());
                }
                if matches!(
                    session.request_status(request),
                    Ok(RequestStatus::Settled { .. })
                ) {
                    session.release_request(request).unwrap();
                    assert_eq!(session.run_until_idle(4).steps, 0);
                    return events;
                }
            }
            panic!("mock turn did not settle");
        })
        .unwrap()
}

#[test]
fn ordinary_plugins_execute_feedback_and_stream_ordered_events() {
    let host = mock::host(MockModel::default(), MockTool::default()).unwrap();
    let generation = host.active().unwrap();
    for prompt in ["first", "second"] {
        assert_eq!(
            turn(&generation, prompt),
            [
                AgentEvent::ToolRequested(ToolCall {
                    prompt: prompt.into(),
                    environment: "local".into(),
                }),
                AgentEvent::Delta("mock: ".into()),
                AgentEvent::Delta(format!("lookup({prompt})")),
                AgentEvent::Completed,
            ]
        );
    }
}

#[test]
fn same_loop_supports_direct_answer_without_tool_feedback() {
    let host = mock::host(
        MockModel {
            label: "direct".into(),
            use_tool: false,
            ..MockModel::default()
        },
        MockTool::default(),
    )
    .unwrap();
    assert_eq!(
        turn(&host.active().unwrap(), "hello"),
        [
            AgentEvent::Delta("direct: ".into()),
            AgentEvent::Delta("hello".into()),
            AgentEvent::Completed,
        ]
    );
}

#[test]
fn independent_tool_failure_returns_typed_feedback_without_failing_the_graph() {
    use phenix_agent::{CONTRACTS_PLUGIN, TOOL_CONTRACT, ToolOutcome, tool_contract};
    use phenix_graph::{
        ComponentDef,
        compose::{Edit, NetworkPatch},
    };
    use phenix_kernel::{Contribution, Key};
    use phenix_plugin::{Failure, PluginInstance, Registrar};
    use std::sync::Arc;

    struct ExternalTool;
    impl PluginInstance for ExternalTool {
        fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
            let definition = ComponentDef::map(|call: ToolCall| ModelInput::ToolResult {
                call,
                outcome: ToolOutcome::Failed {
                    output: "partial output".into(),
                    reason: "denied".into(),
                },
            })
            .claim(&tool_contract())
            .unwrap();
            Contribution::new(
                "main",
                NetworkPatch::new([Edit::Add {
                    path: mock::TOOL_PATH.into(),
                    definition: Arc::new(definition),
                }]),
            )
            .offer(
                "tool",
                mock::TOOL_PATH,
                Key::new(CONTRACTS_PLUGIN, TOOL_CONTRACT),
            )
            .register(registrar)
        }
    }

    let host =
        mock::host_with_options(MockModel::default(), ExternalTool, mock::options()).unwrap();
    let generation = host.active().unwrap();
    for prompt in ["first", "second"] {
        assert_eq!(
            turn(&generation, prompt),
            [
                AgentEvent::ToolRequested(ToolCall {
                    prompt: prompt.into(),
                    environment: "local".into(),
                }),
                AgentEvent::Delta("mock: ".into()),
                AgentEvent::Delta("tool failed: denied: partial output".into()),
                AgentEvent::Completed,
            ]
        );
    }
}

#[test]
fn configured_environments_route_each_call_without_privileged_providers() {
    use phenix_agent::{AgentContracts, AgentLoop, CONTRACTS_PLUGIN, LOOP_PLUGIN, loop_grants};
    use phenix_graph::compose::Footprint;
    use phenix_kernel::{CompositionPolicy, Key, host::KernelHost};
    use phenix_plugin::PluginCatalog;

    const REMOTE: &str = "remote-provider";
    const REMOTE_PATH: &str = "remote-tool";
    let mut catalog = PluginCatalog::new();
    let contracts = catalog
        .register(CONTRACTS_PLUGIN, |_: ()| Ok(AgentContracts))
        .unwrap();
    let loop_key = catalog.register(LOOP_PLUGIN, Ok).unwrap();
    let model_key = catalog.register(mock::MODEL_PLUGIN, Ok).unwrap();
    let local_key = catalog.register(mock::TOOL_PLUGIN, Ok).unwrap();
    let remote_key = catalog.register(REMOTE, Ok).unwrap();
    let mut host = KernelHost::new(catalog.instance());
    let stages = vec![
        host.plugins().prepare_load(&contracts, ()).unwrap(),
        host.plugins()
            .prepare_load(
                &loop_key,
                AgentLoop {
                    environments: ["local".into(), "remote".into()].into(),
                },
            )
            .unwrap(),
        host.plugins()
            .prepare_load(
                &model_key,
                MockModel {
                    environment_by_prompt: [
                        ("remote".into(), "remote".into()),
                        ("missing".into(), "unconfigured".into()),
                    ]
                    .into(),
                    ..MockModel::default()
                },
            )
            .unwrap(),
        host.plugins()
            .prepare_load(&local_key, MockTool::default())
            .unwrap(),
        host.plugins()
            .prepare_load(
                &remote_key,
                MockTool {
                    path: REMOTE_PATH.into(),
                    prefix: "remote".into(),
                },
            )
            .unwrap(),
    ];
    let mut options = mock::options();
    options.imports.allow.as_mut().unwrap().insert((
        Key::new(LOOP_PLUGIN, "env-remote"),
        Key::new(REMOTE, "tool"),
    ));
    options.composition.insert(
        Key::new(REMOTE, "main"),
        CompositionPolicy {
            grants: [Footprint::Component(REMOTE_PATH.into())].into(),
            ..Default::default()
        },
    );
    options.composition.insert(
        Key::new(LOOP_PLUGIN, "main"),
        CompositionPolicy {
            grants: loop_grants(
                mock::MODEL_PATH,
                &[
                    ("local".into(), mock::TOOL_PATH.into()),
                    ("remote".into(), REMOTE_PATH.into()),
                ]
                .into(),
            )
            .into_iter()
            .collect(),
            ..Default::default()
        },
    );
    let candidate = host
        .prepare(stages, options, "advanced", |_| Ok(()))
        .unwrap();
    host.publish(candidate).unwrap();
    let generation = host.active().unwrap();
    for (prompt, environment, response) in [
        ("local", "local", "lookup(local)"),
        ("remote", "remote", "remote(remote)"),
        (
            "missing",
            "unconfigured",
            "tool failed: environment unavailable: ",
        ),
        ("local", "local", "lookup(local)"),
    ] {
        assert_eq!(
            turn(&generation, prompt),
            [
                AgentEvent::ToolRequested(ToolCall {
                    prompt: prompt.into(),
                    environment: environment.into(),
                }),
                AgentEvent::Delta("mock: ".into()),
                AgentEvent::Delta(response.into()),
                AgentEvent::Completed,
            ]
        );
    }
}

#[test]
fn frontend_cannot_submit_internal_tool_feedback() {
    use phenix_agent::ToolOutcome;

    let host = mock::host(MockModel::default(), MockTool::default()).unwrap();
    let generation = host.active().unwrap();
    assert!(
        generation
            .compiled()
            .bind_ingress(PROMPT, InputKey::<ModelInput>::new(PROMPT))
            .is_err()
    );
    generation
        .with_session(|session| {
            let request = session.begin_request(None).unwrap();
            let forged = ModelInput::ToolResult {
                call: ToolCall {
                    prompt: "forged".into(),
                    environment: "local".into(),
                },
                outcome: ToolOutcome::Succeeded {
                    output: "untrusted".into(),
                },
            };
            assert!(
                session
                    .inject_request(request, PROMPT, Value::local(forged))
                    .is_err()
            );
            session.close_request(request).unwrap();
            session.run_until_idle(16);
            assert!(session.failure().is_none());
            session.release_request(request).unwrap();
        })
        .unwrap();
    assert_eq!(
        turn(&generation, "real").last(),
        Some(&AgentEvent::Completed)
    );
}

#[test]
fn missing_host_graph_grant_prevents_publication() {
    let mut options = mock::options();
    options
        .composition
        .remove(&phenix_kernel::Key::new("agent-loop", "main"));
    assert!(mock::host_with_options(MockModel::default(), MockTool::default(), options).is_err());
}

#[test]
fn independent_observer_attaches_without_basic_preset_or_tool() {
    use phenix_agent::{AgentContracts, CONTRACTS_PLUGIN, MODEL_CONTRACT};
    use phenix_graph::{
        ComponentDef,
        compose::{ConnectionKey, Edit, Footprint, NetworkPatch},
        network::{ConnectionDef, PortPath},
    };
    use phenix_kernel::{
        BuildOptions, CandidateArtifacts, CompositionPolicy, Contribution, Key, host::KernelHost,
    };
    use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
    use std::{
        num::NonZeroUsize,
        sync::{Arc, Mutex},
    };

    const ALTERNATE: &str = "alternate-loop";
    const DIRECT_PROMPT: &str = "direct-prompt";
    const OBSERVER: &str = "frontend-observer";
    struct DirectLoop;
    impl PluginInstance for DirectLoop {
        fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
            Contribution::new("main", NetworkPatch::new([]))
                .import("model", Key::new(CONTRACTS_PLUGIN, MODEL_CONTRACT))
                .prepare(|context| {
                    let model = context.bind_import("model")?;
                    let input = model.input::<ModelInput>("input")?;
                    let events = model.output::<AgentEvent>("events")?;
                    Ok(CandidateArtifacts {
                        patch: Some(NetworkPatch::new([
                            Edit::Add {
                                path: DIRECT_PROMPT.into(),
                                definition: Arc::new(ComponentDef::map(ModelInput::Prompt)),
                            },
                            Edit::Connect(ConnectionDef {
                                from: PortPath {
                                    process: DIRECT_PROMPT.into(),
                                    port: "output".into(),
                                },
                                to: PortPath {
                                    process: model.path().into(),
                                    port: input.name().into(),
                                },
                                capacity: NonZeroUsize::new(1).unwrap(),
                            }),
                            Edit::ExposeInput {
                                name: PROMPT.into(),
                                targets: vec![PortPath {
                                    process: DIRECT_PROMPT.into(),
                                    port: "input".into(),
                                }],
                                capacity: NonZeroUsize::new(1).unwrap(),
                            },
                            context.expose_bound_output(EVENTS, &events)?,
                        ])),
                        ..Default::default()
                    })
                })
                .register(registrar)
        }
    }

    struct Observer(Arc<Mutex<Vec<AgentEvent>>>);
    impl PluginInstance for Observer {
        fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
            let received = self.0.clone();
            let definition = ComponentDef::sink(move |event: &AgentEvent| {
                received.lock().unwrap().push(event.clone());
            });
            let definition = Arc::new(definition);
            Contribution::new("main", NetworkPatch::new([]))
                .import("model", Key::new(CONTRACTS_PLUGIN, MODEL_CONTRACT))
                .prepare(move |context| {
                    let model = context.bind_import("model")?;
                    let events = model.output::<AgentEvent>("events")?;
                    Ok(CandidateArtifacts {
                        patch: Some(NetworkPatch::new([
                            Edit::Add {
                                path: OBSERVER.into(),
                                definition: definition.clone(),
                            },
                            context.connect_bound_output(
                                &events,
                                PortPath {
                                    process: OBSERVER.into(),
                                    port: "input".into(),
                                },
                                NonZeroUsize::new(1).unwrap(),
                            )?,
                        ])),
                        ..Default::default()
                    })
                })
                .register(registrar)
        }
    }

    let mut catalog = PluginCatalog::new();
    let contracts = catalog
        .register(CONTRACTS_PLUGIN, |_: ()| Ok(AgentContracts))
        .unwrap();
    let loop_key = catalog.register(ALTERNATE, |_: ()| Ok(DirectLoop)).unwrap();
    let model_key = catalog
        .register(mock::MODEL_PLUGIN, |model: MockModel| Ok(model))
        .unwrap();
    let observer_key = catalog
        .register(OBSERVER, |events| Ok(Observer(events)))
        .unwrap();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let mut host = KernelHost::new(catalog.instance());
    let stages = vec![
        host.plugins().prepare_load(&contracts, ()).unwrap(),
        host.plugins().prepare_load(&loop_key, ()).unwrap(),
        host.plugins()
            .prepare_load(
                &model_key,
                MockModel {
                    use_tool: false,
                    ..MockModel::default()
                },
            )
            .unwrap(),
        host.plugins()
            .prepare_load(&observer_key, observed.clone())
            .unwrap(),
    ];
    let mut options = BuildOptions::default();
    options.execution.max_requests = NonZeroUsize::new(1).unwrap();
    options.execution.capture_capacity = NonZeroUsize::new(1).unwrap();
    options.composition.insert(
        Key::new(mock::MODEL_PLUGIN, "main"),
        CompositionPolicy {
            grants: [Footprint::Component(mock::MODEL_PATH.into())].into(),
            ..Default::default()
        },
    );
    options.composition.insert(
        Key::new(ALTERNATE, "main"),
        CompositionPolicy {
            grants: [
                Footprint::Component(DIRECT_PROMPT.into()),
                Footprint::Connection(ConnectionKey::new(
                    PortPath {
                        process: DIRECT_PROMPT.into(),
                        port: "output".into(),
                    },
                    PortPath {
                        process: mock::MODEL_PATH.into(),
                        port: "input".into(),
                    },
                )),
                Footprint::InputExport(PROMPT.into()),
                Footprint::OutputExport(EVENTS.into()),
            ]
            .into(),
            ..Default::default()
        },
    );
    options.composition.insert(
        Key::new(OBSERVER, "main"),
        CompositionPolicy {
            grants: [
                Footprint::Component(OBSERVER.into()),
                Footprint::Connection(ConnectionKey::new(
                    PortPath {
                        process: mock::MODEL_PATH.into(),
                        port: "events".into(),
                    },
                    PortPath {
                        process: OBSERVER.into(),
                        port: "input".into(),
                    },
                )),
            ]
            .into(),
            ..Default::default()
        },
    );
    options.imports.allow = Some(
        [
            (
                Key::new(ALTERNATE, "model"),
                Key::new(mock::MODEL_PLUGIN, "model"),
            ),
            (
                Key::new(OBSERVER, "model"),
                Key::new(mock::MODEL_PLUGIN, "model"),
            ),
        ]
        .into(),
    );
    options
        .execution
        .discard_outputs
        .insert((mock::MODEL_PATH.into(), "tool".into()));
    let candidate = host
        .prepare(stages, options, "alternate", |_| Ok(()))
        .unwrap();
    host.publish(candidate).unwrap();
    let generation = host.active().unwrap();
    for prompt in ["first", "second"] {
        let events = turn(&generation, prompt);
        assert_eq!(
            events,
            [
                AgentEvent::Delta("mock: ".into()),
                AgentEvent::Delta(prompt.into()),
                AgentEvent::Completed,
            ]
        );
        assert_eq!(std::mem::take(&mut *observed.lock().unwrap()), events);
    }
}
