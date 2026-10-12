#[phenix_sdk::plugin(
    id = "example.agent-alternate",
    version = "1.0.0",
    define(example_agent_contracts::CompleteContract),
    provide(example_agent_contracts::CompleteContract, slot = "Complete")
)]
mod plugin {
    use example_agent_contracts::{AgentError, Complete, ModelReply, ModelRequest, ReplyKind};
    use phenix_sdk::{CallResult, Host};
    #[derive(Default)]
    pub struct State;
    #[provide(Complete)]
    fn complete(
        _: &mut State,
        _: &Host<'_>,
        request: ModelRequest,
    ) -> CallResult<ModelReply, AgentError> {
        Ok(ModelReply {
            kind: ReplyKind::Final,
            call_id: String::new(),
            tool: String::new(),
            text: format!("alternate:{}", request.prompt),
        })
    }
}
