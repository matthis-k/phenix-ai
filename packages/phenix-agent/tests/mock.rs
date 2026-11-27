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
    use phenix_agent::{
        CONTRACTS_PLUGIN, TOOL_CONTRACT, ToolInvocation, ToolOutcome, tool_contract,
    };
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
            let definition = ComponentDef::map(|invocation: ToolInvocation| {
                invocation.complete(ToolOutcome::Failed {
                    output: "partial output".into(),
                    reason: "denied".into(),
                })
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

fn environment_host(
    model: MockModel,
    remote: impl phenix_plugin::PluginInstance + 'static,
    mut options: phenix_kernel::BuildOptions,
) -> phenix_kernel::host::KernelHost {
    use phenix_agent::{
        AgentContracts, AgentLoop, CONTRACTS_PLUGIN, LOOP_PLUGIN,
        environment::{EnvironmentRoutes, environment_grants},
    };
    use phenix_graph::compose::Footprint;
    use phenix_kernel::{CompositionPolicy, Key, host::KernelHost};
    use phenix_plugin::PluginCatalog;

    const REMOTE: &str = "remote-provider";
    const ENVIRONMENT: &str = "environment-extension";
    let mut catalog = PluginCatalog::new();
    let contracts = catalog
        .register(CONTRACTS_PLUGIN, |_: ()| Ok(AgentContracts))
        .unwrap();
    let loop_key = catalog.register(LOOP_PLUGIN, Ok).unwrap();
    let model_key = catalog.register(mock::MODEL_PLUGIN, Ok).unwrap();
    let local_key = catalog.register(mock::TOOL_PLUGIN, Ok).unwrap();
    let remote_key = catalog.register(REMOTE, Ok).unwrap();
    let environment_key = catalog.register(ENVIRONMENT, Ok).unwrap();
    let mut host = KernelHost::new(catalog.instance());
    let stages = vec![
        host.plugins().prepare_load(&contracts, ()).unwrap(),
        host.plugins().prepare_load(&loop_key, AgentLoop).unwrap(),
        host.plugins()
            .prepare_load(
                &environment_key,
                EnvironmentRoutes {
                    environments: ["remote".into()].into(),
                },
            )
            .unwrap(),
        host.plugins().prepare_load(&model_key, model).unwrap(),
        host.plugins()
            .prepare_load(&local_key, MockTool::default())
            .unwrap(),
        host.plugins().prepare_load(&remote_key, remote).unwrap(),
    ];
    options.imports.allow.as_mut().unwrap().insert((
        Key::new(ENVIRONMENT, "env-remote"),
        Key::new(REMOTE, "tool"),
    ));
    options.composition.insert(
        Key::new(REMOTE, "main"),
        CompositionPolicy {
            grants: [Footprint::Component(TEST_REMOTE_PATH.into())].into(),
            ..Default::default()
        },
    );
    options.composition.insert(
        Key::new(ENVIRONMENT, "main"),
        CompositionPolicy {
            grants: environment_grants(&[("remote".into(), TEST_REMOTE_PATH.into())].into())
                .into_iter()
                .collect(),
            ..Default::default()
        },
    );
    let candidate = host
        .prepare(stages, options, "advanced", |_| Ok(()))
        .unwrap();
    host.publish(candidate).unwrap();
    host
}

const TEST_REMOTE_PATH: &str = "remote-tool";

#[test]
fn independent_environment_extension_rewires_the_basic_loop() {
    let host = environment_host(
        MockModel {
            environment_by_prompt: [
                ("remote".into(), "remote".into()),
                ("missing".into(), "unconfigured".into()),
            ]
            .into(),
            ..MockModel::default()
        },
        MockTool {
            path: TEST_REMOTE_PATH.into(),
            prefix: "remote".into(),
        },
        mock::options(),
    );
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
fn one_turn_can_chain_tools_across_environments() {
    let remote = ToolCall {
        prompt: "second".into(),
        environment: "remote".into(),
    };
    let local = ToolCall {
        prompt: "third".into(),
        environment: "local".into(),
    };
    let host = environment_host(
        MockModel {
            follow_up_by_prompt: [
                ("first".into(), remote.clone()),
                ("second".into(), local.clone()),
            ]
            .into(),
            ..MockModel::default()
        },
        MockTool {
            path: TEST_REMOTE_PATH.into(),
            prefix: "remote".into(),
        },
        mock::options(),
    );
    let generation = host.active().unwrap();
    for _ in 0..2 {
        assert_eq!(
            turn(&generation, "first"),
            [
                AgentEvent::ToolRequested(ToolCall {
                    prompt: "first".into(),
                    environment: "local".into(),
                }),
                AgentEvent::ToolRequested(remote.clone()),
                AgentEvent::ToolRequested(local.clone()),
                AgentEvent::Delta("mock: ".into()),
                AgentEvent::Delta("lookup(third)".into()),
                AgentEvent::Completed,
            ]
        );
    }
}

#[test]
fn suspended_remote_request_does_not_block_independent_local_request() {
    use phenix_agent::{CONTRACTS_PLUGIN, TOOL_CONTRACT, ToolInvocation, tool_contract};
    use phenix_graph::{
        ComponentDef, Failure as GraphFailure, Process, ProcessContext, Step,
        compose::{Edit, NetworkPatch},
        runtime::RequestQueuePolicy,
    };
    use phenix_kernel::{Contribution, Key};
    use phenix_plugin::{Failure, PluginInstance, Registrar};
    use std::{num::NonZeroUsize, sync::Arc};

    struct Suspended;
    impl Process for Suspended {
        fn step(&mut self, context: &mut ProcessContext) -> Result<Step, GraphFailure> {
            if let Some(value) = context.take("input") {
                assert!(value.borrow::<ToolInvocation>().is_some());
                Ok(Step::WaitWake)
            } else if context.is_eos("input") {
                Ok(Step::Finished)
            } else {
                Ok(Step::Complete)
            }
        }
    }

    struct Remote;
    impl PluginInstance for Remote {
        fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
            let contract = tool_contract();
            let definition =
                ComponentDef::primitive(contract.required().clone(), Arc::new(|| Suspended))
                    .claim(&contract)
                    .unwrap();
            Contribution::new(
                "main",
                NetworkPatch::new([Edit::Add {
                    path: TEST_REMOTE_PATH.into(),
                    definition: Arc::new(definition),
                }]),
            )
            .offer(
                "tool",
                TEST_REMOTE_PATH,
                Key::new(CONTRACTS_PLUGIN, TOOL_CONTRACT),
            )
            .register(registrar)
        }
    }

    let mut options = mock::options();
    options.execution.max_requests = NonZeroUsize::new(2).unwrap();
    options.execution.request_queue_policy = RequestQueuePolicy::BoundedLanes;
    let host = environment_host(
        MockModel {
            environment_by_prompt: [("slow".into(), "remote".into())].into(),
            ..MockModel::default()
        },
        Remote,
        options,
    );
    let generation = host.active().unwrap();
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
            let slow = session.begin_bound_request(None).unwrap();
            session
                .inject_bound_request(slow, &ingress, "slow".to_owned())
                .unwrap();
            session.close_request(slow).unwrap();
            let mut slow_events = Vec::new();
            for _ in 0..64 {
                session.run_until_idle(1);
                assert!(session.failure().is_none());
                while let Some(packet) = session.take_bound_request_output(slow, &capture).unwrap()
                {
                    slow_events.push(packet.value().borrow::<AgentEvent>().unwrap().clone());
                }
                if session.request_work(slow).unwrap().waiting_wake == 1 {
                    break;
                }
            }
            assert_eq!(session.request_work(slow).unwrap().waiting_wake, 1);
            assert_eq!(
                slow_events,
                [AgentEvent::ToolRequested(ToolCall {
                    prompt: "slow".into(),
                    environment: "remote".into(),
                })]
            );

            let fast = session.begin_bound_request(None).unwrap();
            session
                .inject_bound_request(fast, &ingress, "fast".to_owned())
                .unwrap();
            session.close_request(fast).unwrap();
            let mut fast_events = Vec::new();
            for _ in 0..128 {
                session.run_until_idle(1);
                assert!(session.failure().is_none());
                assert!(session.request_failure(fast).unwrap().is_none());
                while let Some(packet) = session.take_bound_request_output(fast, &capture).unwrap()
                {
                    session
                        .validate_bound_capture(fast, &capture, &packet)
                        .unwrap();
                    fast_events.push(packet.value().borrow::<AgentEvent>().unwrap().clone());
                }
                if matches!(
                    session.request_status(fast),
                    Ok(RequestStatus::Settled { .. })
                ) {
                    break;
                }
            }
            assert_eq!(
                fast_events,
                [
                    AgentEvent::ToolRequested(ToolCall {
                        prompt: "fast".into(),
                        environment: "local".into(),
                    }),
                    AgentEvent::Delta("mock: ".into()),
                    AgentEvent::Delta("lookup(fast)".into()),
                    AgentEvent::Completed,
                ]
            );
            assert!(matches!(
                session.request_status(fast),
                Ok(RequestStatus::Settled { .. })
            ));
            assert_eq!(
                session.request_status(slow).unwrap(),
                RequestStatus::Pending
            );
            assert_eq!(session.request_work(slow).unwrap().waiting_wake, 1);
            session.release_request(fast).unwrap();
            session.cancel_request(slow).unwrap();
            assert_eq!(
                session.request_status(slow).unwrap(),
                RequestStatus::Cancelled
            );
            session.release_request(slow).unwrap();
            assert!(session.failure().is_none());
        })
        .unwrap();
    assert_eq!(
        turn(&generation, "after").last(),
        Some(&AgentEvent::Completed)
    );
}

#[test]
fn frontend_cannot_submit_internal_tool_feedback() {
    use phenix_agent::{ToolInvocation, ToolOutcome};

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
            let forged = ModelInput::ToolResult(
                ToolInvocation::new(ToolCall {
                    prompt: "forged".into(),
                    environment: "local".into(),
                })
                .complete(ToolOutcome::Succeeded {
                    output: "untrusted".into(),
                }),
            );
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

#[test]
fn tool_contract_rejects_providers_that_emit_model_prompts() {
    use phenix_agent::{ToolInvocation, tool_contract};
    use phenix_graph::ComponentDef;

    let forged_prompt = ComponentDef::map(|invocation: ToolInvocation| {
        ModelInput::Prompt(invocation.call().prompt.clone())
    });
    assert!(forged_prompt.claim(&tool_contract()).is_err());
}

#[test]
fn fresh_identical_invocation_cannot_replace_dispatched_invocation() {
    assert_invalid_tool_feedback(false);
}

#[test]
fn previous_result_cannot_be_replayed_for_identical_call() {
    assert_invalid_tool_feedback(true);
}

fn assert_invalid_tool_feedback(replay: bool) {
    use phenix_agent::{
        CONTRACTS_PLUGIN, TOOL_CONTRACT, ToolInvocation, ToolOutcome, ToolResult, tool_contract,
    };
    use phenix_graph::{
        ComponentDef,
        compose::{Edit, NetworkPatch},
    };
    use phenix_kernel::{Contribution, Key};
    use phenix_plugin::{Failure, PluginInstance, Registrar};
    use std::sync::Arc;

    struct WrongCallTool(bool);
    impl PluginInstance for WrongCallTool {
        fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
            let replay = self.0;
            let previous = std::sync::Mutex::new(None::<ToolResult>);
            let definition = ComponentDef::map(move |invocation: ToolInvocation| {
                let outcome = ToolOutcome::Succeeded {
                    output: "untrusted".into(),
                };
                if replay {
                    previous
                        .lock()
                        .unwrap()
                        .get_or_insert_with(|| invocation.complete(outcome))
                        .clone()
                } else {
                    ToolInvocation::new(invocation.call().clone()).complete(outcome)
                }
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
        mock::host_with_options(MockModel::default(), WrongCallTool(replay), mock::options())
            .unwrap();
    let generation = host.active().unwrap();
    if replay {
        assert_eq!(
            turn(&generation, "real").last(),
            Some(&AgentEvent::Completed)
        );
    }
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
                .inject_bound_request(request, &ingress, "real".to_owned())
                .unwrap();
            session.close_request(request).unwrap();
            let mut events = Vec::new();
            for _ in 0..64 {
                session.run_until_idle(1);
                if let Some(failure) = session.request_failure(request).unwrap() {
                    assert_eq!(
                        failure.summary(),
                        "tool feedback does not match dispatched invocation"
                    );
                    assert_eq!(
                        session.request_status(request).unwrap(),
                        RequestStatus::Failed
                    );
                    assert!(session.failure().is_none());
                    assert_eq!(
                        events,
                        [AgentEvent::ToolRequested(ToolCall {
                            prompt: "real".into(),
                            environment: "local".into(),
                        })]
                    );
                    session.release_request(request).unwrap();
                    return;
                }
                while let Some(packet) = session
                    .take_bound_request_output(request, &capture)
                    .unwrap()
                {
                    events.push(packet.value().borrow::<AgentEvent>().unwrap().clone());
                }
            }
            panic!("mismatched feedback did not fail");
        })
        .unwrap();
}

#[test]
fn invocation_identity_is_independent_of_call_arguments() {
    use phenix_agent::ToolInvocation;

    let call = ToolCall {
        prompt: "same".into(),
        environment: "local".into(),
    };
    let first = ToolInvocation::new(call.clone());
    let second = ToolInvocation::new(call);
    assert_eq!(first.call(), second.call());
    assert_eq!(first, first.clone());
    assert_ne!(first, second);
}

#[test]
fn duplicate_completion_without_next_call_fails_instead_of_stranding_request() {
    use phenix_agent::{
        CONTRACTS_PLUGIN, TOOL_CONTRACT, ToolInvocation, ToolOutcome, ToolResult, tool_contract,
    };
    use phenix_graph::{
        ComponentDef, Failure as GraphFailure, Process, ProcessContext, ProcessEmission, Step,
        compose::{Edit, NetworkPatch},
    };
    use phenix_kernel::{Contribution, Key};
    use phenix_plugin::{Failure, PluginInstance, Registrar};
    use std::sync::Arc;

    #[derive(Default)]
    struct DuplicateProcess {
        duplicate: Option<ToolResult>,
        emitted: bool,
    }

    impl Process for DuplicateProcess {
        fn step(&mut self, context: &mut ProcessContext) -> Result<Step, GraphFailure> {
            if let Some(result) = self.duplicate.take() {
                return Ok(Step::Emit(ProcessEmission::new(
                    "output",
                    Value::local(result),
                )));
            }
            if self.emitted {
                self.emitted = false;
                return Ok(Step::Complete);
            }
            let Some(value) = context.take("input") else {
                return Ok(if context.is_eos("input") {
                    Step::Finished
                } else {
                    Step::Complete
                });
            };
            let invocation = value.borrow::<ToolInvocation>().unwrap().clone();
            let duplicate = invocation.call().prompt == "real";
            let result = invocation.complete(ToolOutcome::Succeeded {
                output: "once".into(),
            });
            if duplicate {
                self.duplicate = Some(result.clone());
            }
            self.emitted = true;
            Ok(Step::Emit(ProcessEmission::new(
                "output",
                Value::local(result),
            )))
        }
    }

    struct DuplicateTool;

    impl PluginInstance for DuplicateTool {
        fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
            let contract = tool_contract();
            let definition = ComponentDef::primitive(
                contract.required().clone(),
                Arc::new(DuplicateProcess::default),
            )
            .claim(&contract)
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
        mock::host_with_options(MockModel::default(), DuplicateTool, mock::options()).unwrap();
    let generation = host.active().unwrap();
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
                .inject_bound_request(request, &ingress, "real".to_owned())
                .unwrap();
            session.close_request(request).unwrap();
            for _ in 0..64 {
                session.run_until_idle(1);
                if let Some(failure) = session.request_failure(request).unwrap() {
                    assert_eq!(failure.summary(), "unsolicited tool feedback");
                    assert_eq!(
                        session.request_status(request).unwrap(),
                        RequestStatus::Failed
                    );
                    assert!(session.failure().is_none());
                    session.release_request(request).unwrap();
                    return;
                }
                while session
                    .take_bound_request_output(request, &capture)
                    .unwrap()
                    .is_some()
                {}
            }
            panic!("duplicate completion was not rejected");
        })
        .unwrap();
    assert_eq!(
        turn(&generation, "next"),
        [
            AgentEvent::ToolRequested(ToolCall {
                prompt: "next".into(),
                environment: "local".into(),
            }),
            AgentEvent::Delta("mock: ".into()),
            AgentEvent::Delta("once".into()),
            AgentEvent::Completed,
        ]
    );
}

#[test]
fn cancel_suspended_or_missing_tool_result_and_reuse_same_generation() {
    use phenix_agent::{
        CONTRACTS_PLUGIN, TOOL_CONTRACT, ToolInvocation, ToolOutcome, tool_contract,
    };
    use phenix_graph::{
        ComponentDef, Failure as GraphFailure, Process, ProcessContext, ProcessEmission, Step,
        compose::{Edit, NetworkPatch},
    };
    use phenix_kernel::{Contribution, Key};
    use phenix_plugin::{Failure, PluginInstance, Registrar};
    use std::sync::Arc;

    #[derive(Default)]
    struct SometimesIncomplete {
        emitted: bool,
    }

    impl Process for SometimesIncomplete {
        fn step(&mut self, context: &mut ProcessContext) -> Result<Step, GraphFailure> {
            if self.emitted {
                self.emitted = false;
                return Ok(Step::Complete);
            }
            let Some(value) = context.take("input") else {
                return Ok(if context.is_eos("input") {
                    Step::Finished
                } else {
                    Step::Complete
                });
            };
            let invocation = value.borrow::<ToolInvocation>().unwrap().clone();
            match invocation.call().prompt.as_str() {
                "suspend" => Ok(Step::WaitWake),
                "missing" => Ok(Step::Complete),
                _ => {
                    self.emitted = true;
                    Ok(Step::Emit(ProcessEmission::new(
                        "output",
                        Value::local(invocation.complete(ToolOutcome::Succeeded {
                            output: "recovered".into(),
                        })),
                    )))
                }
            }
        }
    }

    struct IncompleteTool;

    impl PluginInstance for IncompleteTool {
        fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
            let contract = tool_contract();
            let definition = ComponentDef::primitive(
                contract.required().clone(),
                Arc::new(SometimesIncomplete::default),
            )
            .claim(&contract)
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
        mock::host_with_options(MockModel::default(), IncompleteTool, mock::options()).unwrap();
    let generation = host.active().unwrap();
    let ingress = generation
        .compiled()
        .bind_ingress(PROMPT, InputKey::<String>::new(PROMPT))
        .unwrap();
    let capture = generation
        .compiled()
        .bind_capture(EVENTS, OutputKey::<AgentEvent>::new(EVENTS))
        .unwrap();
    for prompt in ["suspend", "missing"] {
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
                        events.push(packet.value().borrow::<AgentEvent>().unwrap().clone());
                    }
                }
                assert_eq!(
                    events,
                    [AgentEvent::ToolRequested(ToolCall {
                        prompt: prompt.into(),
                        environment: "local".into(),
                    })]
                );
                assert_eq!(
                    session.request_status(request).unwrap(),
                    RequestStatus::Pending
                );
                let work = session.request_work(request).unwrap();
                assert!(work.queued > 0);
                assert_eq!(work.waiting_wake, usize::from(prompt == "suspend"));
                assert!(session.cancel_request(request).unwrap().is_empty());
                assert_eq!(
                    session.request_status(request).unwrap(),
                    RequestStatus::Cancelled
                );
                assert!(session.request_disposal(request).unwrap().queued_packets > 0);
                assert!(session.failure().is_none());
                session.release_request(request).unwrap();
                assert_eq!(session.run_until_idle(4).steps, 0);
            })
            .unwrap();
        assert_eq!(
            turn(&generation, "healthy"),
            [
                AgentEvent::ToolRequested(ToolCall {
                    prompt: "healthy".into(),
                    environment: "local".into(),
                }),
                AgentEvent::Delta("mock: ".into()),
                AgentEvent::Delta("recovered".into()),
                AgentEvent::Completed,
            ]
        );
    }
}
