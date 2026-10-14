use crate::{failed, invalid};
use phenix_basic_contracts::{error, BasicError, ErrorCode, ModelRequest, Role};
use serde_json::{json, Value};
use std::{collections::BTreeSet, env};

pub(crate) struct Config {
    pub endpoint: String,
    pub model: String,
    pub credential: Option<String>,
    pub ca_file: Option<String>,
}
// Normalize with the same parsed URL that libcurl receives. This avoids
// accepting an authority under one parser and connecting to another host.
pub(crate) fn endpoint_url(input: &str) -> Result<String, BasicError> {
    let malformed = || error(ErrorCode::InvalidInput, "invalid HTTP model configuration");
    if input != input.trim() || input.chars().any(char::is_control) || input.contains('#') {
        return Err(malformed());
    }
    let parsed = url::Url::parse(input).map_err(|_| malformed())?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.has_host()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        return Err(malformed());
    }
    Ok(parsed.to_string())
}

impl Config {
    pub fn load() -> Result<Self, BasicError> {
        let endpoint = env::var("PHENIX_MODEL_HTTP_ENDPOINT")
            .map_err(|_| failed("HTTP model endpoint is not configured"))?;
        let model = env::var("PHENIX_MODEL_HTTP_MODEL")
            .map_err(|_| failed("HTTP model identity is not configured"))?;
        let endpoint = endpoint_url(&endpoint)?;
        if model.is_empty() {
            return Err(error(
                ErrorCode::InvalidInput,
                "invalid HTTP model configuration",
            ));
        }
        let credential = env::var("PHENIX_MODEL_HTTP_API_KEY").ok();
        if credential
            .as_ref()
            .is_some_and(|s| s.chars().any(char::is_control))
        {
            return Err(error(ErrorCode::InvalidInput, "invalid HTTP credential"));
        }
        Ok(Self {
            endpoint,
            model,
            credential,
            ca_file: env::var("PHENIX_MODEL_HTTP_CA_FILE").ok(),
        })
    }
}
pub(crate) fn function_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn bad(message: &str) -> BasicError {
    error(ErrorCode::InvalidInput, message)
}
pub(crate) fn encode(request: &ModelRequest, model: &str) -> Result<Vec<u8>, BasicError> {
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
        tools.push(json!({"type":"function", "function": {"name":tool.id, "description":tool.description,
            "parameters":{"type":"object", "properties":{"value":schema}, "required":["value"], "additionalProperties":false}}}));
    }
    let mut messages = vec![];
    if !request.skills.is_empty() || !request.context.is_empty() {
        messages.push(
            json!({"role":"developer", "content":format!("Selected skills and context:\n{}",
            json!({"skills":request.skills, "context":request.context}))}),
        );
    }
    let mut pending = BTreeSet::new();
    let mut has_user = false;
    for message in &request.history {
        match message.role {
            Role::User => {
                if !pending.is_empty()
                    || !message.calls.is_empty()
                    || !message.call_id.is_empty()
                    || message.error.is_some()
                {
                    return Err(bad("invalid user history"));
                }
                has_user = true;
                messages.push(json!({"role":"user", "content":message.text}));
            }
            Role::Assistant => {
                if !pending.is_empty() || !message.call_id.is_empty() || message.error.is_some() {
                    return Err(bad("invalid assistant history"));
                }
                let mut calls = vec![];
                for call in &message.calls {
                    if call.id.is_empty()
                        || !function_name(&call.tool)
                        || !pending.insert(call.id.clone())
                    {
                        return Err(bad("invalid historical tool call"));
                    }
                    let arguments: Value = serde_json::from_str(&call.arguments)
                        .map_err(|_| bad("invalid historical tool arguments"))?;
                    calls.push(
                        json!({"id":call.id,"type":"function", "function":{"name":call.tool,
                        "arguments":json!({"value":arguments}).to_string()}}),
                    );
                }
                let mut value = json!({"role":"assistant", "content":message.text});
                if !calls.is_empty() {
                    value["tool_calls"] = Value::Array(calls);
                }
                messages.push(value);
            }
            Role::Tool => {
                if !message.calls.is_empty() || !pending.remove(&message.call_id) {
                    return Err(bad("unmatched tool history"));
                }
                let text = message.error.as_ref().map_or_else(
                    || message.text.clone(),
                    |e| json!({"error":e,"output":message.text}).to_string(),
                );
                messages.push(json!({"role":"tool","tool_call_id":message.call_id,"content":text}));
            }
        }
    }
    if !has_user || !pending.is_empty() {
        return Err(bad("incomplete model history"));
    }
    let mut body = json!({"model":model,"messages":messages,"stream":true,"stream_options":{"include_usage":true}});
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    if let Some(tokens) = request.limits.model_tokens {
        body["max_completion_tokens"] = json!(tokens);
    }
    let bytes = serde_json::to_vec(&body).map_err(|_| invalid("request encoding failed"))?;
    if bytes.len() > phenix_sdk::MAX_BYTES {
        return Err(bad("request exceeds portable payload bounds"));
    }
    Ok(bytes)
}
