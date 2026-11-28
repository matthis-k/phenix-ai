//! Run either mock composition through the same typed frontend:
//! cargo run -p phenix-agent --example mock -- first second
//! cargo run -p phenix-agent --example mock -- --advanced chain remote missing
use phenix_agent::{
    AgentEvent, EVENTS, PROMPT, ToolCall,
    mock::{self, MockModel, MockTool},
};
use phenix_graph::{InputKey, OutputKey, runtime::RequestStatus};
use phenix_kernel::{CallbackError, host::{Generation, KernelHost}};

fn fixture(advanced: bool) -> Result<KernelHost, CallbackError> {
    let model = MockModel {
        environment_by_prompt: [
            ("remote".into(), "remote".into()),
            ("missing".into(), "unconfigured".into()),
        ]
        .into(),
        follow_up_by_prompt: [
            (
                "chain".into(),
                ToolCall {
                    prompt: "second".into(),
                    environment: if advanced { "remote" } else { "local" }.into(),
                },
            ),
            (
                "second".into(),
                ToolCall {
                    prompt: "third".into(),
                    environment: if advanced { "sandbox" } else { "local" }.into(),
                },
            ),
        ]
        .into(),
        ..Default::default()
    };
    if advanced {
        mock::host_with_environments(
            model,
            [
                ("remote".into(), MockTool {
                    prefix: "remote".into(),
                    path: "remote-tool".into(),
                }),
                ("sandbox".into(), MockTool {
                    prefix: "sandbox".into(),
                    path: "sandbox-tool".into(),
                }),
            ]
            .into(),
        )
    } else {
        mock::host(model, MockTool::default())
    }
}

fn turn(
    generation: &Generation,
    prompt: String,
    mut emit: impl FnMut(&AgentEvent),
) -> Result<(), CallbackError> {
    let ingress = generation
        .compiled()
        .bind_ingress(PROMPT, InputKey::<String>::new(PROMPT))?;
    let capture = generation
        .compiled()
        .bind_capture(EVENTS, OutputKey::<AgentEvent>::new(EVENTS))?;
    let request = generation.with_session(|session| -> Result<_, CallbackError> {
        let request = session.begin_bound_request(None)?;
        session.inject_bound_request(request, &ingress, prompt)?;
        session.close_request(request)?;
        Ok(request)
    })??;
    let mut completed = false;
    for _ in 0..256 {
        let (events, settled) = generation.with_session(|session| -> Result<_, CallbackError> {
            session.run_until_idle(1);
            if let Some(failure) = session.failure() {
                return Err(std::io::Error::other(failure.summary()).into());
            }
            if let Some(failure) = session.request_failure(request)? {
                return Err(std::io::Error::other(failure.summary()).into());
            }
            let mut events = Vec::new();
            while let Some(packet) = session.take_bound_request_output(request, &capture)? {
                session.validate_bound_capture(request, &capture, &packet)?;
                events.push(packet.value().borrow::<AgentEvent>().expect("typed event").clone());
            }
            let settled = matches!(session.request_status(request)?, RequestStatus::Settled { .. });
            if settled {
                session.release_request(request)?;
            }
            Ok((events, settled))
        })??;
        // Frontend callbacks execute outside the session lease.
        for event in events {
            if completed {
                return Err(std::io::Error::other("event after completion").into());
            }
            completed = event == AgentEvent::Completed;
            emit(&event);
        }
        if settled {
            return if completed {
                Ok(())
            } else {
                Err(std::io::Error::other("request settled without completion").into())
            };
        }
    }
    Err(std::io::Error::other("mock turn did not settle").into())
}

fn main() -> Result<(), CallbackError> {
    let mut args = std::env::args().skip(1).peekable();
    let advanced = args.peek().is_some_and(|arg| arg == "--advanced");
    if advanced {
        args.next();
    }
    let mut prompts: Vec<_> = args.collect();
    if prompts.is_empty() {
        prompts.push("chain".into());
    }
    let host = fixture(advanced)?;
    let generation = host.active().expect("published mock generation");
    for prompt in prompts {
        turn(&generation, prompt, |event| println!("{event:?}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_mock_profiles_drive_the_same_client_across_turns() {
        for advanced in [false, true] {
            let host = fixture(advanced).unwrap();
            let generation = host.active().unwrap();
            for (prompt, environments, output) in [
                ("chain", if advanced { ["local", "remote", "sandbox"].as_slice() } else { ["local", "local", "local"].as_slice() },
                    if advanced { "sandbox(third)" } else { "lookup(third)" }),
                ("remote", ["remote"].as_slice(),
                    if advanced { "remote(remote)" } else { "tool failed: environment unavailable: " }),
                ("missing", ["unconfigured"].as_slice(), "tool failed: environment unavailable: "),
                ("again", ["local"].as_slice(), "lookup(again)"),
            ] {
                let mut events = Vec::new();
                turn(&generation, prompt.into(), |event| events.push(event.clone())).unwrap();
                let actual: Vec<_> = events.iter().filter_map(|event| match event {
                    AgentEvent::ToolRequested(call) => Some(call.environment.as_str()),
                    _ => None,
                }).collect();
                assert_eq!(actual, environments);
                assert_eq!(&events[events.len()-3..], [
                    AgentEvent::Delta("mock: ".into()),
                    AgentEvent::Delta(output.into()),
                    AgentEvent::Completed,
                ]);
            }
        }
    }
}
