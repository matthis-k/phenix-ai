#[phenix_sdk::plugin(
    id = "example.agent-tool",
    version = "1.0.0",
    define(example_agent_contracts::ExecuteContract),
    define(example_agent_contracts::ToolCountContract),
    provide(example_agent_contracts::ExecuteContract, slot = "Execute"),
    provide(example_agent_contracts::ToolCountContract, slot = "ToolCount")
)]
mod plugin {
    use example_agent_contracts::{AgentError, Execute, ToolCount, ToolRequest};
    use phenix_sdk::{CallError, CallResult, Host};
    #[derive(Default)]
    pub struct State {
        calls: u64,
    }
    #[provide(Execute)]
    fn execute(
        state: &mut State,
        _: &Host<'_>,
        request: ToolRequest,
    ) -> CallResult<String, AgentError> {
        state.calls += 1;
        if request.text == "fail" {
            return Err(CallError::Domain(AgentError::ToolFailed));
        }
        Ok(request.text.to_uppercase())
    }
    #[provide(ToolCount)]
    fn count(state: &mut State, _: &Host<'_>, _: String) -> CallResult<u64, AgentError> {
        Ok(state.calls)
    }
}
