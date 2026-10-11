#[phenix_sdk::plugin(id = "example.agent-tool", version = "1.0.0")]
mod plugin {
    use example_agent_contracts::{AgentError, Execute, ToolCount, ToolRequest};
    use phenix_sdk::{CallError, CallResult, Host};
    #[derive(Default)]
    pub struct State {
        calls: u64,
    }
    #[export(Execute)]
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
    #[export(ToolCount)]
    fn count(state: &mut State, _: &Host<'_>, _: String) -> CallResult<u64, AgentError> {
        Ok(state.calls)
    }
}
