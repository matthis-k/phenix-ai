#[phenix_sdk::plugin(id = "example.agent-model", version = "1.0.0")]
mod plugin {
    use example_agent_contracts::{
        AgentError, Complete, Feedback, FeedbackStatus, ModelReply, ModelRequest, ReplyKind,
    };
    use phenix_sdk::{CallError, CallResult, Host};

    #[derive(Default)]
    pub struct State;

    fn final_reply(text: String) -> ModelReply {
        ModelReply {
            kind: ReplyKind::Final,
            call_id: String::new(),
            tool: String::new(),
            text,
        }
    }
    #[export(Complete)]
    fn complete(
        _: &mut State,
        _: &Host<'_>,
        request: ModelRequest,
    ) -> CallResult<ModelReply, AgentError> {
        let fail = || CallError::Domain(AgentError::InvalidTranscript);
        if request.available_tool != "uppercase" || request.turn == 0 {
            return Err(fail());
        }
        if request.turn == 1 {
            if request.feedback != Feedback::default() {
                return Err(fail());
            }
            match request.prompt.as_str() {
                "model-error" => return Err(CallError::Domain(AgentError::ModelFailed)),
                "final" => return Ok(final_reply("direct".into())),
                "empty-final" => return Ok(final_reply(String::new())),
                "invalid-final" => {
                    let mut reply = final_reply("invalid".into());
                    reply.call_id = "contradiction".into();
                    return Ok(reply);
                }
                _ => {}
            }
        } else {
            if request.feedback.call_id != format!("call-{}", request.turn - 1) {
                return Err(fail());
            }
            if request.prompt == "tool-error" {
                if request.feedback.status != FeedbackStatus::Error
                    || request.feedback.error != AgentError::ToolFailed
                    || !request.feedback.text.is_empty()
                {
                    return Err(fail());
                }
                return Ok(final_reply("recovered".into()));
            }
            if request.feedback.status != FeedbackStatus::Success
                || request.feedback.error != AgentError::None
                || request.feedback.text != request.prompt.to_uppercase()
            {
                return Err(fail());
            }
            if (request.prompt != "repeat" || request.turn > 20) && request.prompt != "duplicate" {
                return Ok(final_reply(format!("mock:{}", request.feedback.text)));
            }
        }
        Ok(ModelReply {
            kind: ReplyKind::Tool,
            call_id: if request.prompt == "duplicate" {
                "call-1".into()
            } else {
                format!("call-{}", request.turn)
            },
            tool: if request.prompt == "unknown-tool" {
                "shell".into()
            } else {
                "uppercase".into()
            },
            text: if request.prompt == "tool-error" {
                "fail".into()
            } else {
                request.prompt
            },
        })
    }
}
