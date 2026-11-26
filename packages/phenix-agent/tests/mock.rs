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
