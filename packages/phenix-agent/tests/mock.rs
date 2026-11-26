use phenix_agent::{
    AgentEvent, EVENTS, ModelInput, PROMPT, ToolCall,
    mock::{self, MockModel, MockTool},
};
use phenix_graph::{Value, runtime::RequestStatus};
use phenix_kernel::host::Generation;

fn turn(generation: &Generation, prompt: &str) -> Vec<AgentEvent> {
    generation
        .with_session(|session| {
            let request = session.begin_request(None).unwrap();
            session
                .inject_request(
                    request,
                    PROMPT,
                    Value::local(ModelInput::Prompt(prompt.into())),
                )
                .unwrap();
            session.close_request(request).unwrap();
            let mut events = Vec::new();
            for _ in 0..64 {
                session.run_until_idle(1);
                assert!(session.failure().is_none());
                while let Some(packet) = session.take_request_output(request, EVENTS).unwrap() {
                    session.validate_capture(request, EVENTS, &packet).unwrap();
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
                    prompt: prompt.into()
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
fn missing_host_graph_grant_prevents_publication() {
    let mut options = mock::options();
    options
        .composition
        .remove(&phenix_kernel::Key::new("agent-loop", "main"));
    assert!(mock::host_with_options(MockModel::default(), MockTool::default(), options).is_err());
}

#[test]
fn independently_authored_loop_does_not_require_basic_preset() {
    use phenix_agent::{AgentContracts, CONTRACTS_PLUGIN, LOOP_PLUGIN, MODEL_CONTRACT};
    use phenix_graph::{
        compose::{Footprint, NetworkPatch},
        process::LocalProcessDispatcher,
    };
    use phenix_kernel::{
        CandidateArtifacts, CompositionPolicy, Contribution, Key, host::KernelHost,
    };
    use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
    use std::{num::NonZeroUsize, sync::Arc};

    const ALTERNATE: &str = "alternate-loop";
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
                            context.expose_bound_input(
                                PROMPT,
                                &[input],
                                NonZeroUsize::new(1).unwrap(),
                            )?,
                            context.expose_bound_output(EVENTS, &events)?,
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
    let tool_key = catalog
        .register(mock::TOOL_PLUGIN, |tool: MockTool| Ok(tool))
        .unwrap();
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
            .prepare_load(&tool_key, MockTool::default())
            .unwrap(),
    ];
    let mut options = mock::options();
    options.composition.remove(&Key::new(LOOP_PLUGIN, "main"));
    options.composition.insert(
        Key::new(ALTERNATE, "main"),
        CompositionPolicy {
            grants: [
                Footprint::InputExport(PROMPT.into()),
                Footprint::OutputExport(EVENTS.into()),
            ]
            .into(),
            ..Default::default()
        },
    );
    options.imports.allow = Some(
        [(
            Key::new(ALTERNATE, "model"),
            Key::new(mock::MODEL_PLUGIN, "model"),
        )]
        .into(),
    );
    options
        .execution
        .discard_outputs
        .insert((mock::MODEL_PATH.into(), "tool".into()));
    options
        .execution
        .discard_outputs
        .insert((mock::TOOL_PATH.into(), "output".into()));
    let candidate = host
        .prepare(
            stages,
            options,
            Arc::new(LocalProcessDispatcher),
            "alternate",
            |_| Ok(()),
        )
        .unwrap();
    host.publish(candidate).unwrap();
    assert_eq!(
        turn(&host.active().unwrap(), "hello"),
        [
            AgentEvent::Delta("mock: ".into()),
            AgentEvent::Delta("hello".into()),
            AgentEvent::Completed,
        ]
    );
}
