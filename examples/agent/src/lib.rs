#[phenix_sdk::plugin(
    id = "example.agent",
    version = "1.0.0",
    define(example_agent_contracts::RunContract),
    define(example_agent_contracts::CompleteContract),
    define(example_agent_contracts::ExecuteContract),
    provide(example_agent_contracts::RunContract, slot = "Run"),
    require(example_agent_contracts::CompleteContract, slot = "Model"),
    require(example_agent_contracts::ExecuteContract, slot = "Tool")
)]
mod plugin {
    use example_agent_contracts::{
        AgentError, Complete, Execute, Feedback, FeedbackStatus, ModelRequest, ReplyKind, Run,
        RunRequest, RunResponse, ToolRequest,
    };
    use phenix_sdk::{CallError, CallResult, Code, Failure, Host};
    use std::collections::BTreeSet;

    #[derive(Default)]
    pub struct State;
    type Model = Complete;
    type Tool = Execute;

    #[provide(Run)]
    fn run(
        _: &mut State,
        host: &Host<'_>,
        request: RunRequest,
    ) -> CallResult<RunResponse, AgentError> {
        let mut feedback = Feedback::default();
        let mut model_turns = 0_u64;
        let mut tool_calls = 0_u64;
        let mut call_ids = BTreeSet::new();
        loop {
            if request.max_turns != 0 && model_turns >= request.max_turns {
                return Err(CallError::Domain(AgentError::TurnLimit));
            }
            model_turns = model_turns.checked_add(1).ok_or_else(|| {
                Failure::new(Code::InfrastructureFailure, "model turn counter exhausted")
            })?;
            let reply = host.invoke::<Model>(
                "Model",
                &ModelRequest {
                    prompt: request.prompt.clone(),
                    turn: model_turns,
                    available_tool: "uppercase".into(),
                    feedback,
                },
            )?;
            match reply.kind {
                ReplyKind::Final => {
                    if !reply.call_id.is_empty() || !reply.tool.is_empty() {
                        return Err(CallError::Domain(AgentError::InvalidReply));
                    }
                    return Ok(RunResponse {
                        answer: reply.text,
                        model_turns,
                        tool_calls,
                    });
                }
                ReplyKind::Tool => {
                    if reply.tool != "uppercase" {
                        return Err(CallError::Domain(AgentError::UnknownTool));
                    }
                    if reply.call_id.is_empty() || !call_ids.insert(reply.call_id.clone()) {
                        return Err(CallError::Domain(AgentError::InvalidReply));
                    }
                    tool_calls = tool_calls.checked_add(1).ok_or_else(|| {
                        Failure::new(Code::InfrastructureFailure, "tool call counter exhausted")
                    })?;
                    feedback = match host.invoke::<Tool>(
                        "Tool",
                        &ToolRequest {
                            call_id: reply.call_id.clone(),
                            text: reply.text,
                        },
                    ) {
                        Ok(text) => Feedback {
                            status: FeedbackStatus::Success,
                            call_id: reply.call_id,
                            text,
                            error: AgentError::None,
                        },
                        Err(CallError::Domain(error)) => Feedback {
                            status: FeedbackStatus::Error,
                            call_id: reply.call_id,
                            text: String::new(),
                            error,
                        },
                        Err(CallError::Kernel(failure)) => return Err(failure.into()),
                    };
                }
            }
        }
    }
}
