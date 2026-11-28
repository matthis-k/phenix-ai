//! Non-destructive extension: the same P1 tool plugin, model configuration
//! and local dispatch contribution run with and without P2's wrapper.

use phenix_agent::{
    AgentContracts, AgentEvent, AgentLoop, CONTRACTS_PLUGIN, EVENTS, LOOP_PLUGIN, PROMPT, ToolCall,
    environment::{EnvironmentRoutes, environment_grants},
    mock::{self, MockModel, MockTool},
    tools::{LocalTools, SelectedTool, Tool},
};
use phenix_graph::{InputKey, OutputKey, compose::Footprint, runtime::RequestStatus};
use phenix_kernel::{CompositionPolicy, Key, host::KernelHost};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};

const EXTRA: &str = "uppercase";
const STRUCTURED: &str = "repeat-text";

#[derive(phenix_graph::ContractValue)]
#[contract(schema = "agent.test.repeat")]
struct RepeatArgs {
    text: String,
    count: i64,
}

const LOCAL_ROUTES: &str = "local-tool-selection";
const REMOTE: &str = "remote-provider";
const ENVIRONMENT: &str = "environment-extension";

/// Independently authored tool: no dependency on either composition or router.
struct Uppercase;

impl PluginInstance for Uppercase {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        Tool::new(EXTRA, |text: String| {
            if text == "denied" {
                Err("denied".into())
            } else {
                Ok(text.to_uppercase())
            }
        })
        .register(registrar)?;
        Tool::new(STRUCTURED, |args: RepeatArgs| {
            let count = usize::try_from(args.count).map_err(|_| "negative count".to_owned())?;
            if count > 8 {
                return Err("count too large".into());
            }
            Ok(args.text.repeat(count))
        })
        .description("Repeat text up to eight times")
        .register(registrar)
    }
}

fn compose(advanced: bool) -> KernelHost {
    let mut catalog = PluginCatalog::new();
    let contracts = catalog
        .register(CONTRACTS_PLUGIN, |_: ()| Ok(AgentContracts))
        .unwrap();
    let agent = catalog.register(LOOP_PLUGIN, Ok).unwrap();
    let model = catalog.register(mock::MODEL_PLUGIN, Ok).unwrap();
    let local = catalog.register(mock::TOOL_PLUGIN, Ok).unwrap();
    let extra = catalog.register(EXTRA, |_: ()| Ok(Uppercase)).unwrap();
    let local_routes = catalog.register(LOCAL_ROUTES, Ok).unwrap();
    let remote = catalog.register(REMOTE, Ok).unwrap();
    let environment = catalog.register(ENVIRONMENT, Ok).unwrap();
    let mut host = KernelHost::new(catalog.instance());
    let mut options = mock::options();
    let mut stages = vec![
        host.plugins().prepare_load(&contracts, ()).unwrap(),
        host.plugins().prepare_load(&agent, AgentLoop).unwrap(),
        host.plugins()
            .prepare_load(
                &model,
                MockModel {
                    tool_by_prompt: [
                        ("hello".into(), "uppercase".into()),
                        ("repeat".into(), STRUCTURED.into()),
                        ("wrong-type".into(), STRUCTURED.into()),
                        ("denied".into(), "uppercase".into()),
                        ("unknown".into(), "unavailable".into()),
                    ]
                    .into(),
                    arguments_by_prompt: [(
                        "repeat".into(),
                        phenix_agent::ToolValue::new(&RepeatArgs {
                            text: "ab".into(),
                            count: 3,
                        }),
                    )]
                    .into(),
                    environment_by_prompt: [("remote".into(), "remote".into())].into(),
                    follow_up_by_prompt: [(
                        "remote".into(),
                        ToolCall {
                            name: "uppercase".into(),
                            arguments: "hello".into(),
                            environment: "local".into(),
                        },
                    )]
                    .into(),
                    ..Default::default()
                },
            )
            .unwrap(),
        host.plugins()
            .prepare_load(&local, MockTool::default())
            .unwrap(),
        host.plugins().prepare_load(&extra, ()).unwrap(),
    ];
    // Discover only the provider snapshots chosen by the host. No tool
    // placement or extra tool offer key is repeated in the routing policy.
    let snapshots = host.plugins().candidate_snapshots(&stages).unwrap();
    let mut providers = SelectedTool::discover(
        snapshots
            .into_iter()
            .filter(|snapshot| [mock::TOOL_PLUGIN, EXTRA].contains(&snapshot.name())),
    )
    .unwrap();
    // The original fixture advertises "tool"; the host selects its alias.
    let base = providers.remove("tool").unwrap();
    providers.insert("lookup".into(), base);
    assert_eq!(
        providers[STRUCTURED].descriptor.as_ref().unwrap().description,
        "Repeat text up to eight times"
    );
    let selection = LocalTools::configure(&mut options, LOCAL_ROUTES, "lookup", providers).unwrap();
    stages.push(
        host.plugins()
            .prepare_load(&local_routes, selection)
            .unwrap(),
    );
    options.composition.insert(
        Key::new(EXTRA, EXTRA),
        CompositionPolicy {
            grants: [Footprint::Component(EXTRA.into())].into(),
            ..Default::default()
        },
    );
    options.composition.insert(
        Key::new(EXTRA, STRUCTURED),
        CompositionPolicy {
            grants: [Footprint::Component(STRUCTURED.into())].into(),
            ..Default::default()
        },
    );
    // P2 is composed separately. Its configuration does not mention EXTRA,
    // uppercase, LOCAL_ROUTES, or the tool's implementation.
    if advanced {
        stages.push(
            host.plugins()
                .prepare_load(
                    &remote,
                    MockTool {
                        prefix: "remote".into(),
                        path: "remote-tool".into(),
                    },
                )
                .unwrap(),
        );
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
        options.imports.allow.as_mut().unwrap().insert((
            Key::new(ENVIRONMENT, "env-remote"),
            Key::new(REMOTE, "tool"),
        ));
        options.composition.insert(
            Key::new(REMOTE, "main"),
            CompositionPolicy {
                grants: [Footprint::Component("remote-tool".into())].into(),
                ..Default::default()
            },
        );
        options.composition.insert(
            Key::new(ENVIRONMENT, "main"),
            CompositionPolicy {
                grants: environment_grants(&[("remote".into(), "remote-tool".into())].into())
                    .into_iter()
                    .collect(),
                ..Default::default()
            },
        );
    }
    let candidate = host
        .prepare(stages, options, "tool-extension", |_| Ok(()))
        .unwrap();
    host.publish(candidate).unwrap();
    host
}

fn turn(host: &KernelHost, prompt: &str) -> Vec<AgentEvent> {
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
                .inject_bound_request(request, &ingress, prompt.to_owned())
                .unwrap();
            session.close_request(request).unwrap();
            let mut events = Vec::new();
            for _ in 0..256 {
                session.run_until_idle(1);
                assert!(session.failure().is_none());
                assert!(session.request_failure(request).unwrap().is_none());
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
                    session.request_status(request).unwrap(),
                    RequestStatus::Settled { .. }
                ) {
                    session.release_request(request).unwrap();
                    return events;
                }
            }
            panic!("tool-extension turn did not settle");
        })
        .unwrap()
}

#[test]
fn p2_preserves_the_same_p1_tool_extension_without_adapter_changes() {
    let p1 = compose(false);
    let p2 = compose(true);
    for (prompt, name, answer) in [
        ("hello", "uppercase", "HELLO"),
        ("ordinary", "lookup", "lookup(ordinary)"),
        ("denied", "uppercase", "tool failed: denied: "),
        ("unknown", "unavailable", "tool failed: tool unavailable: "),
        ("hello", "uppercase", "HELLO"),
    ] {
        let expected = [
            AgentEvent::ToolRequested(ToolCall {
                name: name.into(),
                arguments: prompt.into(),
                environment: "local".into(),
            }),
            AgentEvent::Delta("mock: ".into()),
            AgentEvent::Delta(answer.into()),
            AgentEvent::Completed,
        ];
        assert_eq!(turn(&p1, prompt), expected);
        assert_eq!(turn(&p2, prompt), expected);
    }
    for host in [&p1, &p2] {
        assert_eq!(
            turn(host, "repeat"),
            [
                AgentEvent::ToolRequested(ToolCall {
                    name: STRUCTURED.into(),
                    arguments: phenix_agent::ToolValue::new(&RepeatArgs {
                        text: "ab".into(),
                        count: 3,
                    }),
                    environment: "local".into(),
                }),
                AgentEvent::Delta("mock: ".into()),
                AgentEvent::Delta("ababab".into()),
                AgentEvent::Completed,
            ]
        );
        assert_eq!(
            turn(host, "wrong-type"),
            [
                AgentEvent::ToolRequested(ToolCall {
                    name: STRUCTURED.into(),
                    arguments: "wrong-type".into(),
                    environment: "local".into(),
                }),
                AgentEvent::Delta("mock: ".into()),
                AgentEvent::Delta("tool failed: tool value schema mismatch: ".into()),
                AgentEvent::Completed,
            ]
        );
    }
    // The additional P2 route is genuinely exercised before returning to
    // the unchanged P1 local tool; P1 still rejects that remote environment.
    assert_eq!(
        turn(&p2, "remote"),
        [
            AgentEvent::ToolRequested(ToolCall {
                name: "lookup".into(),
                arguments: "remote".into(),
                environment: "remote".into(),
            }),
            AgentEvent::ToolRequested(ToolCall {
                name: "uppercase".into(),
                arguments: "hello".into(),
                environment: "local".into(),
            }),
            AgentEvent::Delta("mock: ".into()),
            AgentEvent::Delta("HELLO".into()),
            AgentEvent::Completed,
        ]
    );
    assert_eq!(
        turn(&p1, "remote"),
        [
            AgentEvent::ToolRequested(ToolCall {
                name: "lookup".into(),
                arguments: "remote".into(),
                environment: "remote".into(),
            }),
            AgentEvent::Delta("mock: ".into()),
            AgentEvent::Delta("tool failed: environment unavailable: ".into()),
            AgentEvent::Completed,
        ]
    );
}
