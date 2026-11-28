//! Non-destructive extension: the same P1 tool plugin, model configuration
//! and local dispatch contribution run with and without P2's wrapper.

use phenix_agent::{
    AgentContracts, AgentEvent, AgentLoop, CONTRACTS_PLUGIN, EVENTS, LOOP_PLUGIN, PROMPT,
    TOOL_CONTRACT, ToolCall, ToolInvocation, ToolOutcome,
    environment::{EnvironmentRoutes, environment_grants},
    mock::{self, MockModel, MockTool},
    tool_contract,
    tools::{LocalTools, local_tool_grants},
};
use phenix_graph::{
    ComponentDef, InputKey, OutputKey,
    compose::{Edit, Footprint, NetworkPatch},
    runtime::RequestStatus,
};
use phenix_kernel::{CompositionPolicy, Contribution, Key, host::KernelHost};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::sync::Arc;

const EXTRA: &str = "uppercase-tool";
const LOCAL_ROUTES: &str = "local-tool-selection";
const REMOTE: &str = "remote-provider";
const ENVIRONMENT: &str = "environment-extension";

/// Independently authored tool: no dependency on either composition or router.
struct Uppercase;

impl PluginInstance for Uppercase {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        let definition = ComponentDef::map(|invocation: ToolInvocation| {
            let output = invocation.call().prompt.to_uppercase();
            invocation.complete(ToolOutcome::Succeeded { output })
        })
        .claim(&tool_contract())
        .unwrap();
        Contribution::new(
            "main",
            NetworkPatch::new([Edit::Add {
                path: EXTRA.into(),
                definition: Arc::new(definition),
            }]),
        )
        .offer("tool", EXTRA, Key::new(CONTRACTS_PLUGIN, TOOL_CONTRACT))
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
                        ("unknown".into(), "unavailable".into()),
                    ]
                    .into(),
                    environment_by_prompt: [("remote".into(), "remote".into())].into(),
                    follow_up_by_prompt: [(
                        "remote".into(),
                        ToolCall {
                            name: "uppercase".into(),
                            prompt: "hello".into(),
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
        host.plugins()
            .prepare_load(
                &local_routes,
                LocalTools {
                    base: "lookup".into(),
                    additional: ["uppercase".into()].into(),
                },
            )
            .unwrap(),
    ];
    // Identical host approvals for the P1 tool extension in both profiles.
    options.imports.allow.as_mut().unwrap().extend([
        (
            Key::new(LOCAL_ROUTES, "tool-lookup"),
            Key::new(mock::TOOL_PLUGIN, "tool"),
        ),
        (
            Key::new(LOCAL_ROUTES, "tool-uppercase"),
            Key::new(EXTRA, "tool"),
        ),
    ]);
    options.composition.insert(
        Key::new(EXTRA, "main"),
        CompositionPolicy {
            grants: [Footprint::Component(EXTRA.into())].into(),
            ..Default::default()
        },
    );
    options.composition.insert(
        Key::new(LOCAL_ROUTES, "main"),
        CompositionPolicy {
            grants: local_tool_grants(
                mock::TOOL_PATH,
                &[
                    ("lookup".into(), mock::TOOL_PATH.into()),
                    ("uppercase".into(), EXTRA.into()),
                ]
                .into(),
            )
            .into_iter()
            .collect(),
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
                while let Some(packet) =
                    session.take_bound_request_output(request, &capture).unwrap()
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
        ("unknown", "unavailable", "tool failed: tool unavailable: "),
        ("hello", "uppercase", "HELLO"),
    ] {
        let expected = [
            AgentEvent::ToolRequested(ToolCall {
                name: name.into(),
                prompt: prompt.into(),
                environment: "local".into(),
            }),
            AgentEvent::Delta("mock: ".into()),
            AgentEvent::Delta(answer.into()),
            AgentEvent::Completed,
        ];
        assert_eq!(turn(&p1, prompt), expected);
        assert_eq!(turn(&p2, prompt), expected);
    }
    // The additional P2 route is genuinely exercised before returning to
    // the unchanged P1 local tool; P1 still rejects that remote environment.
    assert_eq!(
        turn(&p2, "remote"),
        [
            AgentEvent::ToolRequested(ToolCall {
                name: "lookup".into(),
                prompt: "remote".into(),
                environment: "remote".into(),
            }),
            AgentEvent::ToolRequested(ToolCall {
                name: "uppercase".into(),
                prompt: "hello".into(),
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
                prompt: "remote".into(),
                environment: "remote".into(),
            }),
            AgentEvent::Delta("mock: ".into()),
            AgentEvent::Delta("tool failed: environment unavailable: ".into()),
            AgentEvent::Completed,
        ]
    );
}
