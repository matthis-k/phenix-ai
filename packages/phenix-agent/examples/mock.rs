//! Run either mock composition through the same typed frontend:
//! cargo run -p phenix-agent --example mock -- first second
//! cargo run -p phenix-agent --example mock -- --advanced chain remote missing
use phenix_agent::{
    ToolCall,
    frontend::run_turn,
    mock::{self, MockModel, MockTool},
};
use phenix_kernel::{CallbackError, host::KernelHost};

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
                    name: "lookup".into(),
                    arguments: "second".into(),
                    environment: if advanced { "remote" } else { "local" }.into(),
                },
            ),
            (
                "second".into(),
                ToolCall {
                    name: "lookup".into(),
                    arguments: "third".into(),
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
                (
                    "remote".into(),
                    MockTool {
                        prefix: "remote".into(),
                        path: "remote-tool".into(),
                    },
                ),
                (
                    "sandbox".into(),
                    MockTool {
                        prefix: "sandbox".into(),
                        path: "sandbox-tool".into(),
                    },
                ),
            ]
            .into(),
        )
    } else {
        mock::host(model, MockTool::default())
    }
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
        run_turn(&generation, prompt, 256, |event| println!("{event:?}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_agent::AgentEvent;

    #[test]
    fn p1_constructs_and_executes_basic_loop() {
        exercise(false);
    }

    #[test]
    fn p2_constructs_and_executes_extended_loop() {
        exercise(true);
    }

    fn exercise(advanced: bool) {
        let host = fixture(advanced).unwrap();
        let generation = host.active().unwrap();
        // Inspect the actual compiled composition, not a second fixture.
        let compiled = generation.compiled();
        let mut expected = std::collections::BTreeSet::from([
            "agent-prompt",
            "agent-tool-invocation",
            "agent-tool-router",
            "agent-tool-feedback",
            "model",
            "tool",
        ]);
        if advanced {
            expected.extend(["agent-environment-router", "remote-tool", "sandbox-tool"]);
        }
        let actual: std::collections::BTreeSet<_> = compiled
            .processes()
            .iter()
            .map(|process| process.path())
            .collect();
        assert_eq!(actual, expected);
        let chain_environments: &[&str] = if advanced {
            &["local", "remote", "sandbox"]
        } else {
            &["local", "local", "local"]
        };
        let chain_output = if advanced {
            "sandbox(third)"
        } else {
            "lookup(third)"
        };
        let unavailable = "tool failed: environment unavailable: ";
        let remote_output = if advanced {
            "remote(remote)"
        } else {
            unavailable
        };
        for (prompt, environments, output) in [
            ("chain", chain_environments, chain_output),
            ("remote", &["remote"], remote_output),
            ("missing", &["unconfigured"], unavailable),
            ("again", &["local"], "lookup(again)"),
        ] {
            let mut events = Vec::new();
            run_turn(&generation, prompt.into(), 256, |event| {
                events.push(event.clone())
            })
            .unwrap();
            let actual: Vec<_> = events
                .iter()
                .filter_map(|event| match event {
                    AgentEvent::ToolRequested(call) => Some(call.environment.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(actual, environments);
            assert_eq!(
                &events[events.len() - 3..],
                [
                    AgentEvent::Delta("mock: ".into()),
                    AgentEvent::Delta(output.into()),
                    AgentEvent::Completed,
                ]
            );
        }
    }
}
