//! Deterministic model provider for the public Basic Model contract.
//! All behavior is derived from the request's transcript; no hidden turn state.
use phenix_basic_contracts::{
    error, session_resource, BasicError, ErrorCode, ModelReply, ModelRequest, Role, ToolCall, Usage,
};
use std::collections::BTreeSet;

fn invalid(message: &str) -> BasicError {
    error(ErrorCode::InvalidReply, message)
}

fn usage(request: &ModelRequest, text: &str, calls: &[ToolCall]) -> Usage {
    let input = request
        .history
        .iter()
        .map(|message| message.text().len())
        .sum::<usize>();
    let output = text.len()
        + calls
            .iter()
            .map(|call| call.arguments.len() + call.tool.len())
            .sum::<usize>();
    Usage {
        input_tokens: u64::try_from(input / 4 + 1).unwrap_or(u64::MAX),
        output_tokens: u64::try_from(output / 4 + 1).unwrap_or(u64::MAX),
    }
}

/// Pure scripted model behavior. The native handler additionally verifies
/// the authenticated session resource before invoking this function.
pub fn complete(request: &ModelRequest) -> Result<ModelReply, BasicError> {
    session_resource(&request.session)?;
    if request.run.is_empty() {
        return Err(error(ErrorCode::InvalidInput, "missing run identity"));
    }
    let start = request
        .history
        .iter()
        .rposition(|message| message.role() == Role::User)
        .ok_or_else(|| invalid("missing user prompt"))?;
    let prompt = &request.history[start];
    if !prompt.calls().is_empty() || !prompt.call_id().is_empty() || prompt.error().is_some() {
        return Err(invalid("invalid user message"));
    }
    let tail = &request.history[start + 1..];
    // Two sequential tool rounds, derived entirely from the current transcript.
    if let Some(input) = prompt.text().strip_prefix("rounds:") {
        if !request.tools.iter().any(|tool| tool.id == "uppercase") {
            return Err(error(ErrorCode::InvalidInput, "required tool unavailable"));
        }
        if tail.len() > 4 || !tail.len().is_multiple_of(2) {
            return Err(invalid("incomplete repeated tool round"));
        }
        for (index, round) in tail.chunks_exact(2).enumerate() {
            let expected = ToolCall {
                id: format!("call-{}", index + 1),
                tool: "uppercase".into(),
                arguments: serde_json::to_string(input).expect("serialize string"),
            };
            if round[0].role() != Role::Assistant
                || round[0].calls() != [expected.clone()]
                || round[1].role() != Role::Tool
                || round[1].call_id() != expected.id
            {
                return Err(invalid("invalid repeated tool transcript"));
            }
        }
        let failed = tail
            .chunks_exact(2)
            .find_map(|round| round[1].error().map(|reason| (round[1].call_id(), reason)));
        let (text, calls) = if let Some((call_id, reason)) = failed {
            (format!("mock:{call_id}:error:{reason}"), vec![])
        } else if tail.len() == 4 {
            (format!("mock:rounds:{}", tail[3].text()), vec![])
        } else {
            (
                String::new(),
                vec![ToolCall {
                    id: format!("call-{}", tail.len() / 2 + 1),
                    tool: "uppercase".into(),
                    arguments: serde_json::to_string(input).expect("serialize string"),
                }],
            )
        };
        let metrics = usage(request, &text, &calls);
        if request
            .limits
            .model_tokens
            .is_some_and(|limit| metrics.output_tokens > limit)
        {
            return Err(error(ErrorCode::TurnLimit, "model token limit"));
        }
        return Ok(ModelReply {
            text,
            calls,
            usage: metrics,
        });
    }

    let (text, calls) = if tail.is_empty() {
        if prompt.text() == "model-error" {
            return Err(error(ErrorCode::ModelFailed, "scripted model failure"));
        }
        let single = prompt.text().strip_prefix("tool:");
        let plan = prompt
            .text()
            .strip_prefix("tools:")
            .map(|input| (input, ["uppercase", "length"]))
            .or_else(|| {
                prompt
                    .text()
                    .strip_prefix("tools-fail:")
                    .map(|input| (input, ["fail", "length"]))
            });
        if let Some(input) = single {
            if !request.tools.iter().any(|entry| entry.id == "uppercase") {
                return Err(error(ErrorCode::InvalidInput, "required tool unavailable"));
            }
            (
                String::new(),
                vec![ToolCall {
                    id: "call-1".into(),
                    tool: "uppercase".into(),
                    arguments: serde_json::to_string(input).expect("serialize string"),
                }],
            )
        } else if let Some((input, requested)) = plan {
            for tool in requested {
                if !request.tools.iter().any(|entry| entry.id == tool) {
                    return Err(error(ErrorCode::InvalidInput, "required tool unavailable"));
                }
            }
            let calls = requested
                .into_iter()
                .enumerate()
                .map(|(index, tool)| ToolCall {
                    id: format!("call-{}", index + 1),
                    tool: tool.into(),
                    arguments: serde_json::to_string(input).expect("serialize string"),
                })
                .collect();
            (String::new(), calls)
        } else {
            (format!("mock:{}", prompt.text()), Vec::new())
        }
    } else {
        let (assistant, results) = tail
            .split_first()
            .ok_or_else(|| invalid("missing assistant tool calls"))?;
        if assistant.role() != Role::Assistant
            || assistant.calls().is_empty()
            || !assistant.call_id().is_empty()
            || assistant.error().is_some()
        {
            return Err(invalid("missing assistant tool calls"));
        }
        let mut expected = BTreeSet::new();
        for call in assistant.calls() {
            if call.id.is_empty() || !expected.insert(call.id.as_str()) {
                return Err(invalid("duplicate or empty assistant call id"));
            }
        }
        let mut seen = BTreeSet::new();
        for result in results {
            if result.role() != Role::Tool
                || !result.calls().is_empty()
                || !expected.contains(result.call_id())
                || !seen.insert(result.call_id())
            {
                return Err(invalid("unexpected or duplicate tool result"));
            }
        }
        if seen != expected {
            return Err(invalid("missing tool result"));
        }
        let mut responses = Vec::new();
        for call in assistant.calls() {
            let item = results
                .iter()
                .find(|item| item.call_id() == call.id)
                .ok_or_else(|| invalid("missing tool result"))?;
            responses.push(match item.error() {
                Some(error) => format!("{}:error:{}", call.id, error),
                None => format!("{}:{}", call.id, item.text()),
            });
        }
        (format!("mock:{}", responses.join("|")), Vec::new())
    };
    let metrics = usage(request, &text, &calls);
    if request
        .limits
        .model_tokens
        .is_some_and(|limit| metrics.output_tokens > limit)
    {
        return Err(error(ErrorCode::TurnLimit, "model token limit"));
    }
    Ok(ModelReply {
        text,
        calls,
        usage: metrics,
    })
}

#[cfg(feature = "controlled-pending")]
mod controlled;

#[cfg(not(feature = "controlled-pending"))]
#[phenix_sdk::plugin(
    id = "phenix.model.scripted",
    version = "1.0.0",
    provide(phenix_basic_contracts::Model, slot = "Model")
)]
mod plugin {
    use phenix_basic_contracts::{BasicError, Model, ModelReply, ModelRequest};
    use phenix_sdk::{CallError, CallResult, Host};

    #[derive(Default)]
    pub struct State;

    #[provide(Model::complete)]
    fn complete(
        _: &mut State,
        host: &Host<'_>,
        request: ModelRequest,
    ) -> CallResult<ModelReply, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        super::complete(&request).map_err(CallError::Domain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_basic_contracts::{Limits, Message, ToolInfo};

    fn request(text: &str) -> ModelRequest {
        ModelRequest {
            session: "s1".into(),
            run: "r1".into(),
            history: vec![Message::user(text)],
            tools: vec![
                ToolInfo {
                    id: "uppercase".into(),
                    description: String::new(),
                    arguments_schema: String::new(),
                    idempotent: true,
                },
                ToolInfo {
                    id: "length".into(),
                    description: String::new(),
                    arguments_schema: String::new(),
                    idempotent: true,
                },
            ],
            skills: vec![],
            context: vec![],
            limits: Limits::default(),
        }
    }

    #[test]
    fn repeated_rounds_require_two_complete_tool_transcripts() {
        let mut input = request("rounds:hello");
        for index in 1..=2 {
            let reply = complete(&input).unwrap();
            assert_eq!(reply.calls[0].id, format!("call-{index}"));
            input
                .history
                .push(Message::assistant(reply.text, reply.calls));
            assert_eq!(complete(&input).unwrap_err().code, ErrorCode::InvalidReply);
            input.history.push(Message::tool(
                format!("call-{index}"),
                phenix_basic_contracts::ToolOutcome::Succeeded {
                    text: "HELLO".into(),
                },
            ));
        }
        let reply = complete(&input).unwrap();
        assert!(reply.calls.is_empty());
        assert_eq!(reply.text, "mock:rounds:HELLO");
    }

    #[test]
    fn repeated_rounds_recover_failures_without_further_tool_calls() {
        for failed_round in 1..=2 {
            let mut input = request("rounds:hello");
            for index in 1..=failed_round {
                let reply = complete(&input).unwrap();
                assert_eq!(reply.calls.len(), 1);
                assert_eq!(reply.calls[0].id, format!("call-{index}"));
                input
                    .history
                    .push(Message::assistant(reply.text, reply.calls));
                input.history.push(Message::tool(
                    format!("call-{index}"),
                    if index == failed_round {
                        phenix_basic_contracts::ToolOutcome::Failed {
                            output: "partial output".into(),
                            reason: "scripted round failure".into(),
                        }
                    } else {
                        phenix_basic_contracts::ToolOutcome::Succeeded {
                            text: "HELLO".into(),
                        }
                    },
                ));
            }
            let reply = complete(&input).unwrap();
            assert!(reply.calls.is_empty());
            assert_eq!(
                reply.text,
                format!("mock:call-{failed_round}:error:scripted round failure")
            );
            *input
                .history
                .last_mut()
                .unwrap()
                .tool_call_id_mut()
                .unwrap() = "wrong-call".into();
            assert_eq!(complete(&input).unwrap_err().code, ErrorCode::InvalidReply);
        }
    }

    #[test]
    fn direct_reply_and_caller_limit() {
        let mut input = request("hello");
        assert_eq!(complete(&input).unwrap().text, "mock:hello");
        input.limits.model_tokens = Some(0);
        assert_eq!(complete(&input).unwrap_err().code, ErrorCode::TurnLimit);
        input.run.clear();
        assert_eq!(complete(&input).unwrap_err().code, ErrorCode::InvalidInput);
    }

    #[test]
    fn multiple_calls_use_transcript_and_recover_tool_errors() {
        let mut input = request("tools:hello");
        let first = complete(&input).unwrap();
        assert_eq!(first.calls.len(), 2);
        assert_eq!(first.calls[0].id, "call-1");
        assert_eq!(first.calls[1].tool, "length");
        input
            .history
            .push(Message::assistant(String::new(), first.calls));
        input.history.push(Message::tool(
            "call-1",
            phenix_basic_contracts::ToolOutcome::Succeeded {
                text: "HELLO".into(),
            },
        ));
        assert_eq!(complete(&input).unwrap_err().code, ErrorCode::InvalidReply);
        input.history.push(Message::tool(
            "call-2",
            phenix_basic_contracts::ToolOutcome::Failed {
                output: String::new(),
                reason: "failed".into(),
            },
        ));
        assert_eq!(
            complete(&input).unwrap().text,
            "mock:call-1:HELLO|call-2:error:failed"
        );
        *input
            .history
            .last_mut()
            .unwrap()
            .tool_call_id_mut()
            .unwrap() = "call-1".into();
        assert_eq!(complete(&input).unwrap_err().code, ErrorCode::InvalidReply);
    }

    #[test]
    fn failed_tool_is_recovered_by_the_model() {
        let mut input = request("tools-fail:hello");
        input.tools.push(ToolInfo {
            id: "fail".into(),
            description: String::new(),
            arguments_schema: String::new(),
            idempotent: true,
        });
        let first = complete(&input).unwrap();
        assert_eq!(first.calls.len(), 2);
        assert_eq!(first.calls[0].tool, "fail");
        assert_eq!(first.calls[1].tool, "length");
        input
            .history
            .push(Message::assistant(String::new(), first.calls));
        input.history.push(Message::tool(
            "call-1",
            phenix_basic_contracts::ToolOutcome::Failed {
                output: String::new(),
                reason: "scripted failure".into(),
            },
        ));
        input.history.push(Message::tool(
            "call-2",
            phenix_basic_contracts::ToolOutcome::Succeeded { text: "5".into() },
        ));
        assert_eq!(
            complete(&input).unwrap().text,
            "mock:call-1:error:scripted failure|call-2:5"
        );
    }

    #[test]
    fn missing_tool_and_model_error() {
        let mut input = request("tools:hi");
        input.tools.pop();
        assert_eq!(complete(&input).unwrap_err().code, ErrorCode::InvalidInput);
        *input.history[0].text_mut() = "model-error".into();
        assert_eq!(complete(&input).unwrap_err().code, ErrorCode::ModelFailed);
    }
}
