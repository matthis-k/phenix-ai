//! Verification-only Basic Model provider.
//! No state or external memory: responses depend entirely on ModelRequest.history.
use phenix_basic_contracts::{
    error, session_resource, BasicError, ErrorCode, Message, ModelReply, ModelRequest, Role,
    ToolCall, Usage,
};

fn invalid(message: &str) -> BasicError {
    error(ErrorCode::InvalidReply, message)
}

fn fact(message: &str) -> Option<(&str, &str)> {
    let (key, value) = message.strip_prefix("remember:")?.split_once('=')?;
    if key.is_empty() || value.is_empty() {
        return None;
    }
    Some((key, value))
}

fn previous_fact<'a>(history: &'a [Message], key: &str) -> Option<(usize, &'a str)> {
    history
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, message)| {
            if message.role() != Role::User {
                return None;
            }
            let (found, value) = fact(message.text())?;
            (found == key).then_some((index, value))
        })
}

/// A tool result must be linked to the assistant's recorded call ID.
fn linked_tool_result(history: &[Message]) -> Option<&str> {
    history.windows(2).rev().find_map(|pair| {
        let (assistant, result) = (&pair[0], &pair[1]);
        if assistant.role() != Role::Assistant || result.role() != Role::Tool {
            return None;
        }
        assistant
            .calls()
            .iter()
            .any(|call| call.id == result.call_id() && call.tool == "uppercase")
            .then_some(result)
            .and_then(|result| {
                (result.error().is_none() && result.text() == "HELLO").then_some(result.text())
            })
    })
}

fn tool_continuation(history: &[Message], start: usize) -> Result<String, BasicError> {
    let tail = &history[start + 1..];
    if tail.len() != 2 {
        return Err(invalid(
            "expected one assistant tool call and one tool result",
        ));
    }
    let assistant = &tail[0];
    let result = &tail[1];
    if assistant.role() != Role::Assistant
        || !assistant.text().is_empty()
        || assistant.calls().len() != 1
        || assistant.calls()[0].id != "history-tool-1"
        || assistant.calls()[0].tool != "uppercase"
        || assistant.calls()[0].arguments != r#""hello""#
        || result.role() != Role::Tool
        || result.call_id() != "history-tool-1"
        || result.error().is_some()
        || result.text() != "HELLO"
    {
        return Err(invalid("tool continuation lost or changed its transcript"));
    }
    Ok("tool-result:HELLO".into())
}

fn failing_tool_continuation(history: &[Message], start: usize) -> Result<String, BasicError> {
    let tail = &history[start + 1..];
    if tail.len() != 3 {
        return Err(invalid("expected two tool results"));
    }
    let assistant = &tail[0];
    if assistant.role() != Role::Assistant || assistant.calls().len() != 2 {
        return Err(invalid("missing two assistant tool calls"));
    }
    let first = &assistant.calls()[0];
    let second = &assistant.calls()[1];
    if first.id != "history-fail-1"
        || first.tool != "fail"
        || first.arguments != r#""hello""#
        || second.id != "history-length-2"
        || second.tool != "length"
        || second.arguments != r#""hello""#
        || tail[1].role() != Role::Tool
        || tail[1].call_id() != first.id
        || tail[1].error() != Some("scripted failure")
        || tail[2].role() != Role::Tool
        || tail[2].call_id() != second.id
        || tail[2].error().is_some()
        || tail[2].text() != "5"
    {
        return Err(invalid("failed tool continuation transcript mismatch"));
    }
    Ok("tools-recovered:scripted failure|5".into())
}

/// Test-only observation of what Basic actually supplied to the Model boundary.
/// This does not query authentication, the provider catalog, or any process
/// global state. It reports offered metadata, not executable authority.
fn inspect_capabilities(request: &ModelRequest) -> String {
    let mut tools = request.tools.clone();
    tools.sort_by(|left, right| left.id.cmp(&right.id));
    let mut skills = request
        .skills
        .iter()
        .map(|skill| skill.id.as_str())
        .collect::<Vec<_>>();
    skills.sort_unstable();
    let mut context = request
        .context
        .iter()
        .map(|item| item.id.as_str())
        .collect::<Vec<_>>();
    context.sort_unstable();
    serde_json::json!({
        "format": "phenix.test.model-capabilities.v1",
        "tools": tools,
        "skills": skills,
        "context": context,
    })
    .to_string()
}

/// Pure function. No state is retained between model calls, sessions or processes.
pub fn complete(request: &ModelRequest) -> Result<ModelReply, BasicError> {
    session_resource(&request.session)?;
    if request.run.is_empty() || request.history.is_empty() {
        return Err(error(ErrorCode::InvalidInput, "missing run or history"));
    }
    let start = request
        .history
        .iter()
        .rposition(|message| message.role() == Role::User)
        .ok_or_else(|| invalid("missing user turn"))?;
    let prompt = &request.history[start];
    if !prompt.calls().is_empty() || !prompt.call_id().is_empty() || prompt.error().is_some() {
        return Err(invalid("invalid latest user turn"));
    }

    let (text, calls) = if request
        .history
        .last()
        .is_some_and(|m| m.role() == Role::Tool)
    {
        let reply = match prompt.text() {
            "tools:hello" => tool_continuation(&request.history, start)?,
            "tools-fail:hello" => failing_tool_continuation(&request.history, start)?,
            _ => return Err(invalid("unexpected tool continuation")),
        };
        (reply, vec![])
    } else if start + 1 != request.history.len() {
        return Err(invalid("unexpected transcript tail"));
    } else if prompt.text() == "model-error" {
        return Err(error(ErrorCode::ModelFailed, "verification model failure"));
    } else if let Some(text) = prompt.text().strip_prefix("echo:") {
        (format!("echo:{text}"), vec![])
    } else if let Some((key, value)) = fact(prompt.text()) {
        (format!("stored:{key}={value}"), vec![])
    } else if let Some(key) = prompt.text().strip_prefix("recall:") {
        if key.is_empty() {
            return Err(error(ErrorCode::InvalidInput, "empty recall key"));
        }
        let previous = previous_fact(&request.history[..start], key);
        (
            match previous {
                Some((_, value)) => format!("recalled:{key}={value}"),
                None => format!("absent:{key}"),
            },
            vec![],
        )
    } else if let Some(key) = prompt.text().strip_prefix("verify-assistant:") {
        let (index, value) = previous_fact(&request.history[..start], key)
            .ok_or_else(|| invalid("prior user fact missing"))?;
        let response = request
            .history
            .get(index + 1)
            .ok_or_else(|| invalid("prior assistant answer missing"))?;
        if response.role() != Role::Assistant
            || response.text() != format!("stored:{key}={value}")
            || !response.calls().is_empty()
            || response.error().is_some()
        {
            return Err(invalid("prior assistant answer changed"));
        }
        (format!("verified:{key}={value}"), vec![])
    } else if prompt.text() == "inspect:capabilities" {
        (inspect_capabilities(request), vec![])
    } else if prompt.text() == "tools:hello" {
        if !request.tools.iter().any(|tool| tool.id == "uppercase") {
            return Err(invalid("uppercase tool unavailable"));
        }
        (
            String::new(),
            vec![ToolCall {
                id: "history-tool-1".into(),
                tool: "uppercase".into(),
                arguments: serde_json::to_string("hello").expect("string serialization"),
            }],
        )
    } else if prompt.text() == "tools-fail:hello" {
        if !request.tools.iter().any(|tool| tool.id == "fail")
            || !request.tools.iter().any(|tool| tool.id == "length")
        {
            return Err(invalid("failed tool fixture unavailable"));
        }
        (
            String::new(),
            vec![
                ToolCall {
                    id: "history-fail-1".into(),
                    tool: "fail".into(),
                    arguments: r#""hello""#.into(),
                },
                ToolCall {
                    id: "history-length-2".into(),
                    tool: "length".into(),
                    arguments: r#""hello""#.into(),
                },
            ],
        )
    } else if prompt.text() == "recall-tool" {
        let text = linked_tool_result(&request.history[..start])
            .ok_or_else(|| invalid("prior tool result missing"))?;
        (format!("tool-recalled:{text}"), vec![])
    } else {
        return Err(error(
            ErrorCode::InvalidInput,
            "unknown history verification command",
        ));
    };

    let input_size: usize = request
        .history
        .iter()
        .map(|message| message.text().len())
        .sum();
    let output_size = text.len() + calls.iter().map(|call| call.arguments.len()).sum::<usize>();
    let usage = Usage {
        input_tokens: u64::try_from(input_size / 4 + 1).unwrap_or(u64::MAX),
        output_tokens: u64::try_from(output_size / 4 + 1).unwrap_or(u64::MAX),
    };
    if request
        .limits
        .model_tokens
        .is_some_and(|limit| usage.output_tokens > limit)
    {
        return Err(error(
            ErrorCode::TurnLimit,
            "verification model token limit",
        ));
    }
    Ok(ModelReply { text, calls, usage })
}

#[phenix_sdk::plugin(
    id = "phenix.model.history-test",
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
    use phenix_basic_contracts::{Limits, ToolInfo};

    fn message(role: Role, text: &str) -> Message {
        match role {
            Role::User => Message::user(text),
            Role::Assistant => Message::assistant(text, vec![]),
            Role::Tool => panic!("tool fixtures require a call ID and explicit outcome"),
        }
    }

    fn request(history: Vec<Message>) -> ModelRequest {
        ModelRequest {
            session: "s1".into(),
            run: "run-1".into(),
            history,
            tools: vec![ToolInfo {
                id: "uppercase".into(),
                description: "test tool".into(),
                arguments_schema: r#"{"type":"string"}"#.into(),
                idempotent: true,
            }],
            skills: vec![],
            context: vec![],
            limits: Limits::default(),
        }
    }

    #[test]
    fn recalls_user_and_validates_assistant_from_history_only() {
        let mut history = vec![message(Role::User, "remember:color=violet")];
        let first = complete(&request(history.clone())).unwrap().text;
        assert_eq!(first, "stored:color=violet");
        history.push(message(Role::Assistant, &first));
        history.push(message(Role::User, "recall:color"));
        assert_eq!(
            complete(&request(history.clone())).unwrap().text,
            "recalled:color=violet"
        );
        history.pop();
        history.push(message(Role::User, "verify-assistant:color"));
        assert_eq!(
            complete(&request(history.clone())).unwrap().text,
            "verified:color=violet"
        );
        *history[1].text_mut() = "different".into();
        assert_eq!(
            complete(&request(history)).unwrap_err().code,
            ErrorCode::InvalidReply
        );
    }

    #[test]
    fn cannot_recall_from_a_new_session_or_latest_message_alone() {
        assert_eq!(
            complete(&request(vec![message(Role::User, "recall:color")]))
                .unwrap()
                .text,
            "absent:color"
        );
        let mut absent = request(vec![message(Role::User, "verify-assistant:color")]);
        assert_eq!(complete(&absent).unwrap_err().code, ErrorCode::InvalidReply);
        *absent.history[0].text_mut() = "recall-tool".into();
        assert_eq!(complete(&absent).unwrap_err().code, ErrorCode::InvalidReply);
    }

    #[test]
    fn requires_real_linked_tool_result() {
        let mut history = vec![message(Role::User, "tools:hello")];
        let calls = complete(&request(history.clone())).unwrap().calls;
        assert_eq!(calls.len(), 1);
        history.push(Message::assistant("", calls));
        let result = Message::tool(
            "history-tool-1",
            phenix_basic_contracts::ToolOutcome::Succeeded {
                text: "HELLO".into(),
            },
        );
        history.push(result);
        assert_eq!(
            complete(&request(history.clone())).unwrap().text,
            "tool-result:HELLO"
        );
        history.push(message(Role::Assistant, "tool-result:HELLO"));
        history.push(message(Role::User, "recall-tool"));
        assert_eq!(
            complete(&request(history.clone())).unwrap().text,
            "tool-recalled:HELLO"
        );
        *history[2].tool_call_id_mut().unwrap() = "wrong".into();
        assert_eq!(
            complete(&request(history)).unwrap_err().code,
            ErrorCode::InvalidReply
        );
    }
    #[test]
    fn capability_observation_is_exact_sorted_and_has_no_auth_dependency() {
        let mut input = request(vec![Message::user("inspect:capabilities")]);
        input.tools.push(ToolInfo {
            id: "alpha".into(),
            description: "alpha description".into(),
            arguments_schema: r#"{"type":"object"}"#.into(),
            idempotent: false,
        });
        input.skills.push(phenix_basic_contracts::Skill {
            id: "skill-a".into(),
            description: "fixture".into(),
            instructions: "private instructions".into(),
        });
        input.context.push(phenix_basic_contracts::ContextItem {
            id: "context-a".into(),
            text: "private context".into(),
        });
        let reply = complete(&input).expect("credential-free mock model");
        let view: serde_json::Value = serde_json::from_str(&reply.text).unwrap();
        assert_eq!(view["format"], "phenix.test.model-capabilities.v1");
        assert_eq!(view["tools"][0]["id"], "alpha");
        assert_eq!(view["tools"][1]["id"], "uppercase");
        assert_eq!(view["tools"][0]["arguments_schema"], r#"{"type":"object"}"#);
        assert_eq!(view["tools"][0]["idempotent"], false);
        assert_eq!(view["skills"], serde_json::json!(["skill-a"]));
        assert_eq!(view["context"], serde_json::json!(["context-a"]));
        assert!(!reply.text.contains("private instructions"));
        assert!(!reply.text.contains("private context"));
        assert!(reply.calls.is_empty());

        input.tools.clear();
        let empty: serde_json::Value =
            serde_json::from_str(&complete(&input).unwrap().text).unwrap();
        assert_eq!(empty["tools"], serde_json::json!([]));
    }
}
