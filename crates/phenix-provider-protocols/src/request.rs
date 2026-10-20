use crate::invalid;
use phenix_basic_contracts::{error, BasicError, ErrorCode, ModelRequest, Role};
use serde_json::{json, Value};
use std::collections::BTreeSet;

fn function_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn bad(message: &str) -> BasicError {
    error(ErrorCode::InvalidInput, message)
}
pub fn encode(request: &ModelRequest, model: &str) -> Result<Vec<u8>, BasicError> {
    encode_protocol(request, model, crate::Protocol::OpenaiChat)
}

pub fn encode_protocol(
    request: &ModelRequest,
    model: &str,
    protocol: crate::Protocol,
) -> Result<Vec<u8>, BasicError> {
    encode_protocol_with_max_tokens(request, model, protocol, None)
}

/// Encode for the selected protocol, using an optional provider-configured
/// request maximum. This is a wire requirement, not a new caller limit.
/// A caller's lower explicit limit always takes precedence.
pub fn encode_protocol_with_max_tokens(
    request: &ModelRequest,
    model: &str,
    protocol: crate::Protocol,
    provider_max_tokens: Option<u64>,
) -> Result<Vec<u8>, BasicError> {
    if provider_max_tokens == Some(0) {
        return Err(bad("invalid HTTP provider output-token maximum"));
    }
    let (messages, tools) = prepare(request)?;
    let output_tokens = match (request.limits.model_tokens, provider_max_tokens) {
        (Some(caller), Some(provider)) => Some(caller.min(provider)),
        (Some(caller), None) => Some(caller),
        (None, Some(provider)) => Some(provider),
        (None, None) => None,
    };
    let body = match protocol {
        crate::Protocol::OpenaiChat => chat_body(request, model, &messages, &tools),
        crate::Protocol::OpenaiResponses => responses_body(request, model, &messages, &tools),
        crate::Protocol::AnthropicMessages => {
            let tokens = output_tokens
                .ok_or_else(|| bad("Anthropic Messages requires configured max_tokens"))?;
            anthropic_body(request, model, &messages, &tools, tokens)
        }
    };
    let bytes = serde_json::to_vec(&body).map_err(|_| invalid("request encoding failed"))?;
    if bytes.len() > phenix_sdk::MAX_BYTES {
        return Err(bad("request exceeds portable payload bounds"));
    }
    Ok(bytes)
}

struct Tool {
    name: String,
    description: String,
    schema: Value,
}

struct PreparedCall {
    id: String,
    tool: String,
    arguments: Value,
}

enum PreparedMessage {
    User(String),
    Assistant {
        text: String,
        calls: Vec<PreparedCall>,
    },
    Tool {
        call_id: String,
        text: String,
        error: Option<String>,
    },
}

fn prepare(request: &ModelRequest) -> Result<(Vec<PreparedMessage>, Vec<Tool>), BasicError> {
    phenix_basic_contracts::session_resource(&request.session)?;
    if request.run.is_empty()
        || request.history.is_empty()
        || request.limits.model_tokens == Some(0)
        || request.limits.http_timeout_ms == Some(0)
    {
        return Err(bad("empty model request or zero execution limit"));
    }
    let mut tools = vec![];
    let mut names = BTreeSet::new();
    for tool in &request.tools {
        if !function_name(&tool.id) || !names.insert(&tool.id) {
            return Err(bad("invalid or duplicate tool identity"));
        }
        let schema: Value = serde_json::from_str(&tool.arguments_schema)
            .map_err(|_| bad("invalid tool schema JSON"))?;
        if !schema.is_object() && !schema.is_boolean() {
            return Err(bad("invalid tool schema"));
        }
        tools.push(Tool {
            name: tool.id.clone(),
            description: tool.description.clone(),
            schema,
        });
    }
    let mut pending = BTreeSet::new();
    let mut has_user = false;
    let mut messages = vec![];
    for message in &request.history {
        match message.role() {
            Role::User => {
                if !pending.is_empty()
                    || !message.calls().is_empty()
                    || !message.call_id().is_empty()
                    || message.error().is_some()
                {
                    return Err(bad("invalid user history"));
                }
                has_user = true;
                messages.push(PreparedMessage::User(message.text().to_owned()));
            }
            Role::Assistant => {
                if !pending.is_empty() || !message.call_id().is_empty() || message.error().is_some() {
                    return Err(bad("invalid assistant history"));
                }
                let mut calls = vec![];
                for call in message.calls() {
                    if call.id.is_empty()
                        || !function_name(&call.tool)
                        || !pending.insert(call.id.clone())
                    {
                        return Err(bad("invalid historical tool call"));
                    }
                    let arguments: Value = serde_json::from_str(&call.arguments)
                        .map_err(|_| bad("invalid historical tool arguments"))?;
                    calls.push(PreparedCall {
                        id: call.id.clone(),
                        tool: call.tool.clone(),
                        arguments,
                    });
                }
                messages.push(PreparedMessage::Assistant {
                    text: message.text().to_owned(),
                    calls,
                });
            }
            Role::Tool => {
                if !message.calls().is_empty() || !pending.remove(&message.call_id()) {
                    return Err(bad("unmatched tool history"));
                }
                messages.push(PreparedMessage::Tool {
                    call_id: message.call_id().to_owned(),
                    text: message.text().to_owned(),
                    error: message.error().map(str::to_owned),
                });
            }
        }
    }
    if !has_user || !pending.is_empty() {
        return Err(bad("incomplete model history"));
    }
    Ok((messages, tools))
}

fn system(request: &ModelRequest) -> Option<Value> {
    if request.skills.is_empty() && request.context.is_empty() {
        return None;
    }
    Some(
        json!({"role":"developer", "content":format!("Selected skills and context:\n{}",
        json!({"skills":request.skills, "context":request.context}))}),
    )
}

fn tool_arguments(arguments: &Value) -> String {
    json!({"value": arguments}).to_string()
}

fn tool_error(text: &str, error: &str) -> String {
    json!({"error": error, "output": text}).to_string()
}

fn chat_body(
    request: &ModelRequest,
    model: &str,
    messages: &[PreparedMessage],
    tools: &[Tool],
) -> Value {
    let mut msgs = vec![];
    if let Some(developer) = system(request) {
        msgs.push(developer);
    }
    for message in messages {
        match message {
            PreparedMessage::User(text) => msgs.push(json!({"role":"user", "content":text})),
            PreparedMessage::Assistant { text, calls } => {
                let calls: Vec<Value> = calls
                    .iter()
                    .map(|call| {
                        json!({"id":call.id,"type":"function", "function":{"name":call.tool,
                            "arguments":tool_arguments(&call.arguments)}})
                    })
                    .collect();
                let mut value = json!({"role":"assistant", "content":text});
                if !calls.is_empty() {
                    value["tool_calls"] = Value::Array(calls);
                }
                msgs.push(value);
            }
            PreparedMessage::Tool {
                call_id,
                text,
                error,
            } => {
                let content = error
                    .as_ref()
                    .map_or_else(|| text.clone(), |e| tool_error(text, e));
                msgs.push(json!({"role":"tool","tool_call_id":call_id,"content":content}));
            }
        }
    }
    let tools: Vec<Value> = tools
        .iter()
        .map(|tool| {
            json!({"type":"function", "function": {"name":tool.name, "description":tool.description,
                "parameters":{"type":"object", "properties":{"value":tool.schema}, "required":["value"], "additionalProperties":false}}})
        })
        .collect();
    let mut body = json!({"model":model,"messages":msgs,"stream":true,"stream_options":{"include_usage":true}});
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    if let Some(tokens) = request.limits.model_tokens {
        body["max_completion_tokens"] = json!(tokens);
    }
    body
}

fn responses_body(
    request: &ModelRequest,
    model: &str,
    messages: &[PreparedMessage],
    tools: &[Tool],
) -> Value {
    let mut input = vec![];
    if let Some(developer) = system(request) {
        input.push(developer);
    }
    for message in messages {
        match message {
            PreparedMessage::User(text) => input.push(json!({"role":"user", "content":text})),
            PreparedMessage::Assistant { text, calls } => {
                input.push(json!({"role":"assistant", "content":text}));
                for call in calls {
                    input.push(json!({"type":"function_call", "call_id":call.id,
                        "name":call.tool, "arguments":tool_arguments(&call.arguments)}));
                }
            }
            PreparedMessage::Tool {
                call_id,
                text,
                error,
            } => {
                let output = error
                    .as_ref()
                    .map_or_else(|| text.clone(), |e| tool_error(text, e));
                input
                    .push(json!({"type":"function_call_output","call_id":call_id,"output":output}));
            }
        }
    }
    let tools: Vec<Value> = tools
        .iter()
        .map(|tool| {
            json!({"type":"function", "name":tool.name, "description":tool.description,
                "parameters":{"type":"object", "properties":{"value":tool.schema}, "required":["value"], "additionalProperties":false}})
        })
        .collect();
    let mut body = json!({"model":model,"input":input,"stream":true,"store":false});
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    if let Some(tokens) = request.limits.model_tokens {
        body["max_output_tokens"] = json!(tokens);
    }
    body
}

fn anthropic_body(
    request: &ModelRequest,
    model: &str,
    messages: &[PreparedMessage],
    tools: &[Tool],
    max_tokens: u64,
) -> Value {
    let system = system(request).map(|developer| {
        developer["content"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    });
    let mut msgs = vec![];
    for message in messages {
        match message {
            PreparedMessage::User(text) => msgs.push(json!({"role":"user", "content":text})),
            PreparedMessage::Assistant { text, calls } => {
                let mut content = vec![];
                if !text.is_empty() {
                    content.push(json!({"type":"text", "text":text}));
                }
                for call in calls {
                    content.push(json!({"type":"tool_use", "id":call.id, "name":call.tool,
                        "input":json!({"value": call.arguments})}));
                }
                msgs.push(json!({"role":"assistant", "content":content}));
            }
            PreparedMessage::Tool {
                call_id,
                text,
                error,
            } => {
                let content = error
                    .as_ref()
                    .map_or_else(|| text.clone(), |e| tool_error(text, e));
                let mut block =
                    json!({"type":"tool_result","tool_use_id":call_id,"content":content});
                if error.is_some() {
                    block["is_error"] = json!(true);
                }
                // All results for the preceding assistant tool-use batch must
                // share one user turn. Do not manufacture intervening turns.
                if let Some(last) = msgs.last_mut() {
                    if last["role"] == "user"
                        && last["content"].as_array().is_some_and(|blocks| {
                            !blocks.is_empty()
                                && blocks.iter().all(|entry| entry["type"] == "tool_result")
                        })
                    {
                        last["content"]
                            .as_array_mut()
                            .expect("checked array")
                            .push(block);
                        continue;
                    }
                }
                msgs.push(json!({"role":"user", "content":[block]}));
            }
        }
    }
    let tools: Vec<Value> = tools
        .iter()
        .map(|tool| {
            json!({"name":tool.name, "description":tool.description,
                "input_schema":{"type":"object", "properties":{"value":tool.schema}, "required":["value"], "additionalProperties":false}})
        })
        .collect();
    let mut body = json!({"model":model,"messages":msgs,"stream":true});
    if let Some(system) = system {
        body["system"] = json!(system);
    }
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    body["max_tokens"] = json!(max_tokens);
    body
}
