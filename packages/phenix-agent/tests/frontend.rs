//! An independent frontend attaches to ordinary agent contracts.

use phenix_agent::{
    AgentContracts, AgentEvent, AgentLoop, CONTRACTS_PLUGIN, EVENTS, LOOP_PLUGIN, MODEL_CONTRACT,
    PROMPT, PROMPT_CONTRACT,
    environment::{EnvironmentRoutes, environment_grants},
    mock::{self, MockModel, MockTool},
};
use phenix_graph::{
    ComponentDef, InputKey, OutputKey,
    compose::{ConnectionKey, Edit, Footprint, NetworkPatch},
    network::{ConnectionDef, PortPath},
    runtime::RequestStatus,
};
use phenix_kernel::{CandidateArtifacts, CompositionPolicy, Contribution, Key, host::KernelHost};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::{num::NonZeroUsize, sync::Arc};

const FRONTEND: &str = "mock-frontend";
const DECODE: &str = "frontend-decode";
const ENCODE: &str = "frontend-encode";
const INPUT: &str = "frontend-request";
const OUTPUT: &str = "frontend-update";
const ENVIRONMENT: &str = "environment-extension";
const REMOTE: &str = "remote-provider";
const REMOTE_PATH: &str = "remote-tool";

#[derive(Clone, Debug, PartialEq, Eq)]
struct MockPrompt(String);

#[derive(Clone, Debug, PartialEq, Eq)]
struct MockUpdate(AgentEvent);

fn port(process: &str, port: &str) -> PortPath {
    PortPath {
        process: process.into(),
        port: port.into(),
    }
}

struct Frontend;

impl PluginInstance for Frontend {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        Contribution::new("main", NetworkPatch::new([]))
            .import("prompt", Key::new(CONTRACTS_PLUGIN, PROMPT_CONTRACT))
            .import("model", Key::new(CONTRACTS_PLUGIN, MODEL_CONTRACT))
            .prepare(|context| {
                let prompt = context.bind_import("prompt")?;
                let input = prompt.input::<String>("input")?;
                let model = context.bind_import("model")?;
                let events = model.output::<AgentEvent>("events")?;
                let capacity = NonZeroUsize::new(1).unwrap();
                Ok(CandidateArtifacts {
                    patch: Some(NetworkPatch::new([
                        Edit::Add {
                            path: DECODE.into(),
                            definition: Arc::new(ComponentDef::map(|request: MockPrompt| {
                                request.0
                            })),
                        },
                        Edit::Add {
                            path: ENCODE.into(),
                            definition: Arc::new(ComponentDef::map(MockUpdate)),
                        },
                        Edit::Connect(ConnectionDef {
                            from: port(DECODE, "output"),
                            to: port(prompt.path(), input.name()),
                            capacity,
                        }),
                        context.connect_bound_output(&events, port(ENCODE, "input"), capacity)?,
                        Edit::ExposeInput {
                            name: INPUT.into(),
                            targets: vec![port(DECODE, "input")],
                            capacity,
                        },
                        Edit::ExposeOutput {
                            name: OUTPUT.into(),
                            from: port(ENCODE, "output"),
                        },
                    ])),
                    ..Default::default()
                })
            })
            .register(registrar)
    }
}

#[test]
fn independent_frontend_and_native_ingress_share_one_loop() {
    exercise_frontend(false);
}

#[test]
fn frontend_and_environment_extensions_compose_without_knowing_each_other() {
    exercise_frontend(true);
}

fn exercise_frontend(extended: bool) {
    let mut catalog = PluginCatalog::new();
    let contracts = catalog
        .register(CONTRACTS_PLUGIN, |_: ()| Ok(AgentContracts))
        .unwrap();
    let agent = catalog.register(LOOP_PLUGIN, Ok).unwrap();
    let model = catalog.register(mock::MODEL_PLUGIN, Ok).unwrap();
    let tool = catalog.register(mock::TOOL_PLUGIN, Ok).unwrap();
    let frontend = catalog.register(FRONTEND, |_: ()| Ok(Frontend)).unwrap();
    let remote = catalog.register(REMOTE, Ok).unwrap();
    let environment = catalog.register(ENVIRONMENT, Ok).unwrap();
    let mut host = KernelHost::new(catalog.instance());
    let mut stages = vec![
        host.plugins().prepare_load(&contracts, ()).unwrap(),
        host.plugins().prepare_load(&agent, AgentLoop).unwrap(),
        host.plugins()
            .prepare_load(
                &model,
                MockModel {
                    environment_by_prompt: [
                        ("remote".into(), "remote".into()),
                        ("missing".into(), "missing".into()),
                    ]
                    .into(),
                    ..MockModel::default()
                },
            )
            .unwrap(),
        host.plugins()
            .prepare_load(&tool, MockTool::default())
            .unwrap(),
        host.plugins().prepare_load(&frontend, ()).unwrap(),
    ];
    let mut options = mock::options();
    options.imports.allow.as_mut().unwrap().extend([
        (
            Key::new(FRONTEND, "prompt"),
            Key::new(LOOP_PLUGIN, "prompt"),
        ),
        (
            Key::new(FRONTEND, "model"),
            Key::new(mock::MODEL_PLUGIN, "model"),
        ),
    ]);
    options.composition.insert(
        Key::new(FRONTEND, "main"),
        CompositionPolicy {
            grants: [
                Footprint::Component(DECODE.into()),
                Footprint::Component(ENCODE.into()),
                Footprint::Connection(ConnectionKey::new(
                    port(DECODE, "output"),
                    port("agent-prompt", "input"),
                )),
                Footprint::Connection(ConnectionKey::new(
                    port(mock::MODEL_PATH, "events"),
                    port(ENCODE, "input"),
                )),
                Footprint::InputExport(INPUT.into()),
                Footprint::OutputExport(OUTPUT.into()),
            ]
            .into(),
            ..Default::default()
        },
    );
    if extended {
        stages.push(
            host.plugins()
                .prepare_load(
                    &environment,
                    EnvironmentRoutes {
                        environments: ["remote".into()].into(),
                    },
                )
                .unwrap(),
        );
        stages.push(
            host.plugins()
                .prepare_load(
                    &remote,
                    MockTool {
                        path: REMOTE_PATH.into(),
                        prefix: "remote".into(),
                    },
                )
                .unwrap(),
        );
        options.imports.allow.as_mut().unwrap().insert((
            Key::new(ENVIRONMENT, "env-remote"),
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
            Key::new(ENVIRONMENT, "main"),
            CompositionPolicy {
                grants: environment_grants(&[("remote".into(), REMOTE_PATH.into())].into())
                    .into_iter()
                    .collect(),
                ..Default::default()
            },
        );
    }
    let candidate = host
        .prepare(stages, options, "frontend", |_| Ok(()))
        .unwrap();
    host.publish(candidate).unwrap();
    let generation = host.active().unwrap();
    let native = generation
        .compiled()
        .bind_ingress(PROMPT, InputKey::<String>::new(PROMPT))
        .unwrap();
    let protocol = generation
        .compiled()
        .bind_ingress(INPUT, InputKey::<MockPrompt>::new(INPUT))
        .unwrap();
    let events = generation
        .compiled()
        .bind_capture(EVENTS, OutputKey::<AgentEvent>::new(EVENTS))
        .unwrap();
    let updates = generation
        .compiled()
        .bind_capture(OUTPUT, OutputKey::<MockUpdate>::new(OUTPUT))
        .unwrap();
    for (external, prompt) in [
        (true, "external"),
        (false, "native"),
        (true, "remote"),
        (false, "remote"),
        (true, "missing"),
        (false, "native"),
    ] {
        let expected = match (extended, prompt) {
            (true, "remote") => "remote(remote)".into(),
            (_, "remote" | "missing") => "tool failed: environment unavailable: ".into(),
            _ => format!("lookup({prompt})"),
        };
        generation
            .with_session(|session| {
                let request = session.begin_bound_request(None).unwrap();
                if external {
                    session
                        .inject_bound_request(request, &protocol, MockPrompt(prompt.into()))
                        .unwrap();
                } else {
                    session
                        .inject_bound_request(request, &native, prompt.to_owned())
                        .unwrap();
                }
                session.close_request(request).unwrap();
                let mut original = Vec::new();
                let mut translated = Vec::new();
                for _ in 0..128 {
                    session.run_until_idle(1);
                    assert!(session.failure().is_none());
                    assert!(session.request_failure(request).unwrap().is_none());
                    while let Some(packet) =
                        session.take_bound_request_output(request, &events).unwrap()
                    {
                        original.push(packet.value().borrow::<AgentEvent>().unwrap().clone());
                    }
                    while let Some(packet) = session
                        .take_bound_request_output(request, &updates)
                        .unwrap()
                    {
                        translated.push(packet.value().borrow::<MockUpdate>().unwrap().0.clone());
                    }
                    if matches!(
                        session.request_status(request),
                        Ok(RequestStatus::Settled { .. })
                    ) {
                        assert_eq!(original, translated);
                        assert!(original.contains(&AgentEvent::Delta(expected)));
                        assert_eq!(original.last(), Some(&AgentEvent::Completed));
                        session.release_request(request).unwrap();
                        return;
                    }
                }
                panic!("frontend request did not settle");
            })
            .unwrap();
    }
}
