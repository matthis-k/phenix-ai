use crate::{Endpoint, ProviderError, ProviderRequest, ProviderResponse, RateLimits};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use phenix_core::{
    CallableId, ModelCacheControl, ModelCacheRetention, ModelCacheWritePolicy, ModelId,
    Key, ModelInferenceRequest, ModelInferenceResponse, ModelToolCall, ModelToolDescriptor,
    ModelToolResult, ModelToolTurn, ModelTurnUsage, PhenixSchema, PhenixValue, UsageQuantity,
    ValueCodec,
};
use reqwest::header::{CONTENT_TYPE, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub trait ProtocolAdapter: Send + Sync {
    fn name(&self) -> &'static str;

    fn supports_model_catalog(&self) -> bool {
        false
    }

    fn model_catalog_request(
        &self,
        _endpoint: &Endpoint,
    ) -> Result<Option<ProviderRequest>, ProviderError> {
        Ok(None)
    }

    fn decode_model_catalog(
        &self,
        _response: &ProviderResponse,
    ) -> Result<Vec<ModelId>, ProviderError> {
        Err(ProviderError::Protocol {
            message: format!("protocol {} does not define model discovery", self.name()),
        })
    }

    fn encode(
        &self,
        endpoint: &Endpoint,
        request: &ModelInferenceRequest,
    ) -> Result<ProviderRequest, ProviderError>;

    fn decode(&self, response: &ProviderResponse) -> Result<ModelInferenceResponse, ProviderError>;

    fn decode_for_request(
        &self,
        request: &ModelInferenceRequest,
        response: &ProviderResponse,
    ) -> Result<ModelInferenceResponse, ProviderError> {
        let mut decoded = self.decode(response)?;
        normalize_model_tool_calls(request, &mut decoded, self.name())?;
        Ok(decoded)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    OpenAiResponses,
    OpenAiChatCompletions,
    AnthropicMessages,
    OpenCodeGo,
    OpenCodeZen,
}

impl ProtocolAdapter for Protocol {
    fn name(&self) -> &'static str {
        match self {
            Self::OpenAiResponses => "openai_responses",
            Self::OpenAiChatCompletions => "openai_chat_completions",
            Self::AnthropicMessages => "anthropic_messages",
            Self::OpenCodeGo => "opencode_go",
            Self::OpenCodeZen => "opencode_zen",
        }
    }

    fn supports_model_catalog(&self) -> bool {
        matches!(
            self,
            Self::OpenAiResponses | Self::OpenAiChatCompletions | Self::AnthropicMessages
        )
    }

    fn model_catalog_request(
        &self,
        endpoint: &Endpoint,
    ) -> Result<Option<ProviderRequest>, ProviderError> {
        if !self.supports_model_catalog() {
            return Ok(None);
        }
        let mut headers = BTreeMap::new();
        if matches!(self, Self::AnthropicMessages) {
            headers.insert("anthropic-version".to_owned(), "2023-06-01".to_owned());
        }
        Ok(Some(ProviderRequest {
            method: crate::HttpMethod::Get,
            url: endpoint.join("models")?,
            headers,
            body: Vec::new(),
        }))
    }

    fn decode_model_catalog(
        &self,
        response: &ProviderResponse,
    ) -> Result<Vec<ModelId>, ProviderError> {
        if !self.supports_model_catalog() {
            return Err(ProviderError::Protocol {
                message: format!("protocol {} does not define model discovery", self.name()),
            });
        }
        decode_standard_model_catalog(response)
    }

    fn encode(
        &self,
        endpoint: &Endpoint,
        request: &ModelInferenceRequest,
    ) -> Result<ProviderRequest, ProviderError> {
        match self {
            Self::OpenAiResponses => openai_responses_request(endpoint, request),
            Self::OpenAiChatCompletions => openai_chat_request(endpoint, request),
            Self::AnthropicMessages => anthropic_request(endpoint, request),
            Self::OpenCodeGo => opencode_go_request(endpoint, request),
            Self::OpenCodeZen => opencode_zen_protocol(request)?.encode(endpoint, request),
        }
    }

    fn decode(&self, response: &ProviderResponse) -> Result<ModelInferenceResponse, ProviderError> {
        match self {
            Self::OpenAiResponses => openai_responses_response(response),
            Self::OpenAiChatCompletions => openai_chat_response(response),
            Self::AnthropicMessages => anthropic_response(response),
            Self::OpenCodeGo | Self::OpenCodeZen => opencode_response(response),
        }
    }
}

fn decode_standard_model_catalog(
    response: &ProviderResponse,
) -> Result<Vec<ModelId>, ProviderError> {
    let value = parse_json(response)?;
    let data =
        value
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| ProviderError::Protocol {
                message: "provider model catalog response is missing data[]".to_owned(),
            })?;
    let mut models = Vec::with_capacity(data.len());
    for item in data {
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| ProviderError::Protocol {
                message: "provider model catalog entry is missing a string id".to_owned(),
            })?;
        models.push(ModelId::parse(id).map_err(|error| ProviderError::Protocol {
            message: format!("provider returned invalid model id {id:?}: {error}"),
        })?);
    }
    models.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    models.dedup();
    Ok(models)
}

// OpenCode gateways multiplex several provider wire protocols behind one
// provider identity, so protocol selection remains a pure function of the model
// and never leaks into routing or authentication semantics.
fn opencode_go_protocol(request: &ModelInferenceRequest) -> Protocol {
    let model = request.model.as_str();
    if model.starts_with("gpt-") {
        return Protocol::OpenAiResponses;
    }
    if model.starts_with("minimax-") || model.starts_with("qwen") {
        return Protocol::AnthropicMessages;
    }
    Protocol::OpenAiChatCompletions
}

const OPENCODE_SESSION_HEADER: &str = "x-opencode-session";
const PHENIX_USER_AGENT: &str = concat!("phenix-ai/", env!("CARGO_PKG_VERSION"));

fn opencode_go_request(
    endpoint: &Endpoint,
    request: &ModelInferenceRequest,
) -> Result<ProviderRequest, ProviderError> {
    let mut outgoing = opencode_go_protocol(request).encode(endpoint, request)?;
    if let Some(session_id) = request.session_id.as_ref() {
        outgoing.headers.insert(
            OPENCODE_SESSION_HEADER.to_owned(),
            session_id.as_str().to_owned(),
        );
    }
    outgoing
        .headers
        .insert(USER_AGENT.as_str().to_owned(), PHENIX_USER_AGENT.to_owned());
    Ok(outgoing)
}

fn opencode_zen_protocol(request: &ModelInferenceRequest) -> Result<Protocol, ProviderError> {
    let model = request.model.as_str();
    if model.starts_with("gemini-") {
        return Err(ProviderError::InvalidRequest {
            message: format!(
                "OpenCode Zen model {model:?} requires the Google-native endpoint, which Phenix does not expose yet"
            ),
        });
    }
    if model.starts_with("gpt-") || model.starts_with("grok-") {
        return Ok(Protocol::OpenAiResponses);
    }
    if model.starts_with("claude-") || model.starts_with("qwen") {
        return Ok(Protocol::AnthropicMessages);
    }
    Ok(Protocol::OpenAiChatCompletions)
}

fn opencode_response(response: &ProviderResponse) -> Result<ModelInferenceResponse, ProviderError> {
    let value = parse_json(response)?;
    if value.get("output").is_some() {
        return openai_responses_response(response);
    }
    if value.get("choices").is_some() {
        return openai_chat_response(response);
    }
    if value.get("content").is_some() {
        return anthropic_response(response);
    }
    Err(ProviderError::Protocol {
        message: "OpenCode response does not match a supported provider protocol".to_owned(),
    })
}

fn base_request(
    endpoint: &Endpoint,
    path: &str,
    body: Value,
    headers: BTreeMap<String, String>,
) -> Result<ProviderRequest, ProviderError> {
    Ok(ProviderRequest {
        method: crate::HttpMethod::Post,
        url: endpoint.join(path)?,
        headers,
        body: serde_json::to_vec(&body).map_err(|error| ProviderError::Protocol {
            message: error.to_string(),
        })?,
    })
}

fn json_headers() -> BTreeMap<String, String> {
    BTreeMap::from([(
        CONTENT_TYPE.as_str().to_owned(),
        "application/json".to_owned(),
    )])
}

fn request_object(
    request: &ModelInferenceRequest,
    reserved: &[&str],
) -> Result<(Map<String, Value>, String), ProviderError> {
    let text = std::str::from_utf8(request.input.as_ref())
        .map_err(|_| ProviderError::InvalidRequest {
            message: "provider protocols require UTF-8 model input".to_owned(),
        })?
        .to_owned();
    let mut body = Map::new();
    for (key, value) in &request.options {
        if reserved.contains(&key.as_str()) {
            return Err(ProviderError::InvalidRequest {
                message: format!(
                    "provider option {key:?} conflicts with a required protocol field"
                ),
            });
        }
        body.insert(
            key.clone(),
            Value::from_value(value).map_err(|error| ProviderError::InvalidRequest {
                message: format!("provider option {key:?} is not JSON-compatible: {error:?}"),
            })?,
        );
    }
    Ok((body, text))
}

fn json_schema(schema: &PhenixSchema) -> Result<Value, ProviderError> {
    let schema = match schema {
        PhenixSchema::Any => serde_json::json!({}),
        PhenixSchema::Never => serde_json::json!({"not": {}}),
        PhenixSchema::Unit => serde_json::json!({"type": "null"}),
        PhenixSchema::Bool => serde_json::json!({"type": "boolean"}),
        PhenixSchema::I64 => serde_json::json!({"type": "integer"}),
        PhenixSchema::U64 => serde_json::json!({"type": "integer", "minimum": 0}),
        PhenixSchema::F64 => serde_json::json!({"type": "number"}),
        PhenixSchema::String => serde_json::json!({"type": "string"}),
        PhenixSchema::Bytes => {
            serde_json::json!({"type": "string", "contentEncoding": "base64"})
        }
        PhenixSchema::Option(item) => {
            serde_json::json!({"anyOf": [json_schema(item)?, {"type": "null"}]})
        }
        PhenixSchema::Array { item, len } => serde_json::json!({
            "type": "array",
            "items": json_schema(item)?,
            "minItems": len,
            "maxItems": len,
        }),
        PhenixSchema::List(item) => {
            serde_json::json!({"type": "array", "items": json_schema(item)?})
        }
        PhenixSchema::Map(item) => {
            serde_json::json!({"type": "object", "additionalProperties": json_schema(item)?})
        }
        PhenixSchema::Table(fields) => {
            let properties = fields
                .iter()
                .map(|(key, schema)| Ok((key.as_str().to_owned(), json_schema(schema)?)))
                .collect::<Result<Map<String, Value>, ProviderError>>()?;
            let required = fields
                .keys()
                .map(|key| key.as_str().to_owned())
                .collect::<Vec<_>>();
            serde_json::json!({
                "type": "object",
                "properties": properties,
                "required": required,
                "additionalProperties": false,
            })
        }
        PhenixSchema::Variant(variants) => {
            let variants = variants
                .iter()
                .map(|(tag, schema)| {
                    Ok(serde_json::json!({
                        "type": "object",
                        "properties": {
                            "tag": {
                                "type": "string",
                                "enum": [tag.as_str()],
                            },
                            "value": json_schema(schema)?,
                        },
                        "required": ["tag", "value"],
                        "additionalProperties": false,
                    }))
                })
                .collect::<Result<Vec<_>, ProviderError>>()?;
            serde_json::json!({"anyOf": variants})
        }
        PhenixSchema::Callable { .. } | PhenixSchema::Object { .. } => {
            return Err(ProviderError::InvalidRequest {
                message: "model tool input schema cannot be represented as provider JSON Schema"
                    .to_owned(),
            });
        }
    };
    Ok(schema)
}

const OPENAI_TOOL_NAME_PREFIX: &str = "phx1_";

fn openai_tool_name(id: &CallableId) -> String {
    let mut name = String::with_capacity(OPENAI_TOOL_NAME_PREFIX.len() + id.as_str().len());
    name.push_str(OPENAI_TOOL_NAME_PREFIX);
    for byte in id.as_str().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' => name.push(char::from(byte)),
            b'_' => name.push_str("_u"),
            b'.' => name.push_str("_d"),
            b'/' => name.push_str("_s"),
            b':' => name.push_str("_c"),
            b'@' => name.push_str("_a"),
            _ => unreachable!("CallableId validation rejects unsupported bytes"),
        }
    }
    name
}

fn parse_openai_callable_id(name: &str) -> Result<CallableId, ProviderError> {
    let Some(encoded) = name.strip_prefix(OPENAI_TOOL_NAME_PREFIX) else {
        return parse_callable_id(name);
    };
    let mut id = String::with_capacity(encoded.len());
    let mut bytes = encoded.bytes();
    while let Some(byte) = bytes.next() {
        if byte != b'_' {
            id.push(char::from(byte));
            continue;
        }
        let escape = bytes.next().ok_or_else(|| ProviderError::Protocol {
            message: format!("provider returned malformed encoded OpenAI tool name {name:?}"),
        })?;
        id.push(match escape {
            b'u' => '_',
            b'd' => '.',
            b's' => '/',
            b'c' => ':',
            b'a' => '@',
            _ => {
                return Err(ProviderError::Protocol {
                    message: format!(
                        "provider returned malformed encoded OpenAI tool name {name:?}"
                    ),
                });
            }
        });
    }
    parse_callable_id(&id)
}

fn model_tool_json_schema(schema: &PhenixSchema) -> Result<Value, ProviderError> {
    match schema {
        PhenixSchema::Unit => Ok(serde_json::json!({
            "type": "object",
            "properties": {},
            "required": [],
            "additionalProperties": false,
        })),
        PhenixSchema::Table(_) | PhenixSchema::Map(_) => json_schema(schema),
        PhenixSchema::Variant(_) => {
            let Value::Object(mut object) = json_schema(schema)? else {
                unreachable!("variant JSON Schema is an object")
            };
            object.insert("type".to_owned(), Value::String("object".to_owned()));
            Ok(Value::Object(object))
        }
        _ => Ok(serde_json::json!({
            "type": "object",
            "properties": {
                "value": json_schema(schema)?,
            },
            "required": ["value"],
            "additionalProperties": false,
        })),
    }
}

fn openai_tool(tool: &ModelToolDescriptor) -> Result<Value, ProviderError> {
    Ok(serde_json::json!({
        "type": "function",
        "name": openai_tool_name(&tool.id),
        "description": tool.description,
        "parameters": model_tool_json_schema(&tool.input_schema)?,
    }))
}

fn openai_chat_tool(tool: &ModelToolDescriptor) -> Result<Value, ProviderError> {
    Ok(serde_json::json!({
        "type": "function",
        "function": {
            "name": openai_tool_name(&tool.id),
            "description": tool.description,
            "parameters": model_tool_json_schema(&tool.input_schema)?,
        },
    }))
}

fn anthropic_tool(tool: &ModelToolDescriptor) -> Result<Value, ProviderError> {
    Ok(serde_json::json!({
        "name": tool.id.as_str(),
        "description": tool.description,
        "input_schema": model_tool_json_schema(&tool.input_schema)?,
    }))
}

fn encode_tools(
    tools: &[ModelToolDescriptor],
    encode: fn(&ModelToolDescriptor) -> Result<Value, ProviderError>,
) -> Result<Option<Value>, ProviderError> {
    if tools.is_empty() {
        return Ok(None);
    }
    tools
        .iter()
        .map(encode)
        .collect::<Result<Vec<_>, _>>()
        .map(|tools| Some(Value::Array(tools)))
}

fn phenix_json(value: &PhenixValue) -> Result<Value, ProviderError> {
    match value {
        PhenixValue::Unit => Ok(Value::Null),
        PhenixValue::Bool(value) => Ok(Value::Bool(*value)),
        PhenixValue::I64(value) => Ok(Value::Number((*value).into())),
        PhenixValue::U64(value) => Ok(Value::Number((*value).into())),
        PhenixValue::F64(value) => serde_json::Number::from_f64(*value)
            .map(Value::Number)
            .ok_or_else(|| ProviderError::InvalidRequest {
                message: "model tool value contains a non-finite float".to_owned(),
            }),
        PhenixValue::String(value) => Ok(Value::String(value.clone())),
        PhenixValue::Bytes(value) => Ok(Value::String(BASE64_STANDARD.encode(value))),
        PhenixValue::Option(None) => Ok(Value::Null),
        PhenixValue::Option(Some(value)) => phenix_json(value),
        PhenixValue::List(values) => values
            .iter()
            .map(phenix_json)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        PhenixValue::Map(values) => values
            .iter()
            .map(|(key, value)| phenix_json(value).map(|value| (key.clone(), value)))
            .collect::<Result<Map<_, _>, _>>()
            .map(Value::Object),
        PhenixValue::Table(values) => values
            .iter()
            .map(|(key, value)| phenix_json(value).map(|value| (key.as_str().to_owned(), value)))
            .collect::<Result<Map<_, _>, _>>()
            .map(Value::Object),
        PhenixValue::Variant { tag, value } => Ok(serde_json::json!({
            "tag": tag.as_str(),
            "value": phenix_json(value)?,
        })),
        PhenixValue::Callable(_) | PhenixValue::Object(_) => Err(ProviderError::InvalidRequest {
            message: "model tool value contains an opaque capability reference".to_owned(),
        }),
    }
}

fn model_tool_input_json(
    schema: &PhenixSchema,
    value: &PhenixValue,
) -> Result<Value, ProviderError> {
    schema
        .parse(value)
        .map_err(|error| ProviderError::InvalidRequest {
            message: format!("model tool input violates its declared schema: {error}"),
        })?;
    match schema {
        PhenixSchema::Unit => Ok(serde_json::json!({})),
        PhenixSchema::Table(_) | PhenixSchema::Map(_) | PhenixSchema::Variant(_) => {
            phenix_json(value)
        }
        _ => Ok(serde_json::json!({"value": phenix_json(value)?})),
    }
}

fn model_tool_input_for_call(
    tools: &[ModelToolDescriptor],
    call: &ModelToolCall,
) -> Result<Value, ProviderError> {
    let tool = tools
        .iter()
        .find(|tool| tool.id == call.callable_id)
        .ok_or_else(|| ProviderError::InvalidRequest {
            message: format!(
                "model tool continuation references unavailable callable {}",
                call.callable_id
            ),
        })?;
    model_tool_input_json(&tool.input_schema, &call.input)
}

fn tool_arguments(
    tools: &[ModelToolDescriptor],
    call: &ModelToolCall,
) -> Result<String, ProviderError> {
    serde_json::to_string(&model_tool_input_for_call(tools, call)?).map_err(|error| {
        ProviderError::Protocol {
            message: format!("cannot encode model tool arguments: {error}"),
        }
    })
}

fn tool_output(result: &ModelToolResult) -> Result<String, ProviderError> {
    let value = phenix_json(&result.output)?;
    let value = if result.is_error {
        serde_json::json!({"error": value})
    } else {
        value
    };
    serde_json::to_string(&value).map_err(|error| ProviderError::Protocol {
        message: format!("cannot encode model tool result: {error}"),
    })
}

fn assistant_text(turn: &ModelToolTurn) -> Result<String, ProviderError> {
    std::str::from_utf8(turn.assistant_output.as_ref())
        .map(str::to_owned)
        .map_err(|_| ProviderError::InvalidRequest {
            message: "provider protocols require UTF-8 assistant continuation output".to_owned(),
        })
}

fn validate_tool_turn(turn: &ModelToolTurn) -> Result<(), ProviderError> {
    if turn.tool_calls.len() != turn.tool_results.len() {
        return Err(ProviderError::InvalidRequest {
            message: "model tool continuation must contain one result per tool call".to_owned(),
        });
    }
    for call in &turn.tool_calls {
        let matches = turn
            .tool_results
            .iter()
            .filter(|result| {
                result.call_id == call.call_id && result.callable_id == call.callable_id
            })
            .count();
        if matches != 1 {
            return Err(ProviderError::InvalidRequest {
                message: format!(
                    "model tool continuation must contain exactly one matching result for call {}",
                    call.call_id
                ),
            });
        }
    }
    Ok(())
}

fn take_inference_effort(body: &mut Map<String, Value>) -> Result<Option<Value>, ProviderError> {
    let Some(inference) = body.remove("inference") else {
        return Ok(None);
    };
    if inference.is_null() {
        return Ok(None);
    }
    let Value::Object(mut inference) = inference else {
        return Err(ProviderError::InvalidRequest {
            message: "provider-neutral inference options must be an object".to_owned(),
        });
    };
    let effort = inference.remove("effort");
    if !inference.is_empty() {
        return Err(ProviderError::InvalidRequest {
            message: format!(
                "unsupported provider-neutral inference options: {}",
                inference.keys().cloned().collect::<Vec<_>>().join(", ")
            ),
        });
    }
    match effort {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(effort)) => Ok(Some(Value::String(effort))),
        Some(_) => Err(ProviderError::InvalidRequest {
            message: "provider-neutral inference effort must be a string".to_owned(),
        }),
    }
}

fn has_effective_cache_controls(cache: &ModelCacheControl) -> bool {
    cache.write != ModelCacheWritePolicy::ProviderDefault
        || cache.retention != ModelCacheRetention::ProviderDefault
        || cache.partition_key.is_some()
        || cache.explicit_prefix_bytes.is_some()
}

fn reject_nondefault_cache(cache: &ModelCacheControl, protocol: &str) -> Result<(), ProviderError> {
    if has_effective_cache_controls(cache) {
        Err(ProviderError::InvalidRequest {
            message: format!("{protocol} does not support provider-neutral cache controls"),
        })
    } else {
        Ok(())
    }
}

fn openai_has_explicit_cache_options(model: &str) -> bool {
    model.starts_with("gpt-5.6") || model.starts_with("gpt-6")
}

fn explicit_cache_prefix<'a>(
    text: &'a str,
    cache: &ModelCacheControl,
) -> Result<Option<(&'a str, &'a str)>, ProviderError> {
    if cache.write != ModelCacheWritePolicy::ExplicitPrefix {
        return Ok(None);
    }
    let bytes = cache
        .explicit_prefix_bytes
        .ok_or_else(|| ProviderError::InvalidRequest {
            message: "explicit cache prefix policy requires a prefix byte boundary".to_owned(),
        })?;
    let boundary = usize::try_from(bytes).map_err(|_| ProviderError::InvalidRequest {
        message: "cache prefix byte boundary does not fit this platform".to_owned(),
    })?;
    if boundary == 0 || boundary > text.len() || !text.is_char_boundary(boundary) {
        return Err(ProviderError::InvalidRequest {
            message: format!(
                "cache prefix byte boundary {boundary} is not a valid UTF-8 boundary for {} bytes",
                text.len()
            ),
        });
    }
    Ok(Some(text.split_at(boundary)))
}

fn apply_openai_responses_cache(
    body: &mut Map<String, Value>,
    request: &ModelInferenceRequest,
) -> Result<(), ProviderError> {
    if let Some(key) = request.cache.partition_key.as_deref() {
        if body.contains_key("prompt_cache_key") {
            return Err(ProviderError::InvalidRequest {
                message: "provider option prompt_cache_key conflicts with typed cache control"
                    .to_owned(),
            });
        }
        if key.trim().is_empty() {
            return Err(ProviderError::InvalidRequest {
                message: "cache partition key must not be empty".to_owned(),
            });
        }
        body.insert("prompt_cache_key".to_owned(), Value::String(key.to_owned()));
    }

    let needs_options = request.cache.write != ModelCacheWritePolicy::ProviderDefault
        || request.cache.retention != ModelCacheRetention::ProviderDefault;
    if needs_options && !openai_has_explicit_cache_options(request.model.as_str()) {
        return Err(ProviderError::InvalidRequest {
            message: format!(
                "OpenAI Responses model {} does not support the requested cache controls",
                request.model
            ),
        });
    }
    if needs_options && body.contains_key("prompt_cache_options") {
        return Err(ProviderError::InvalidRequest {
            message: "provider option prompt_cache_options conflicts with typed cache control"
                .to_owned(),
        });
    }

    let mut options = Map::new();
    match request.cache.write {
        ModelCacheWritePolicy::ProviderDefault => {}
        ModelCacheWritePolicy::CacheThroughRequestEnd => {
            options.insert("mode".to_owned(), Value::String("implicit".to_owned()));
        }
        ModelCacheWritePolicy::ExplicitPrefix => {
            if request.cache.explicit_prefix_bytes.is_none() {
                return Err(ProviderError::InvalidRequest {
                    message: "explicit cache prefix policy requires a prefix byte boundary"
                        .to_owned(),
                });
            }
            options.insert("mode".to_owned(), Value::String("explicit".to_owned()));
        }
    }
    match request.cache.retention {
        ModelCacheRetention::ProviderDefault => {}
        ModelCacheRetention::ThirtyMinutes => {
            options.insert("ttl".to_owned(), Value::String("30m".to_owned()));
        }
        retention => {
            return Err(ProviderError::InvalidRequest {
                message: format!(
                    "OpenAI Responses does not support requested cache retention {retention:?} for model {}",
                    request.model
                ),
            });
        }
    }
    if !options.is_empty() {
        body.insert("prompt_cache_options".to_owned(), Value::Object(options));
    }
    Ok(())
}

fn anthropic_cache_control_value(cache: &ModelCacheControl) -> Result<Value, ProviderError> {
    let mut control = Map::from_iter([("type".to_owned(), Value::String("ephemeral".to_owned()))]);
    match cache.retention {
        ModelCacheRetention::ProviderDefault => {}
        ModelCacheRetention::FiveMinutes => {
            control.insert("ttl".to_owned(), Value::String("5m".to_owned()));
        }
        ModelCacheRetention::OneHour => {
            control.insert("ttl".to_owned(), Value::String("1h".to_owned()));
        }
        retention => {
            return Err(ProviderError::InvalidRequest {
                message: format!(
                    "Anthropic Messages does not support requested cache retention {retention:?}"
                ),
            });
        }
    }
    Ok(Value::Object(control))
}

fn apply_anthropic_cache(
    body: &mut Map<String, Value>,
    cache: &ModelCacheControl,
) -> Result<(), ProviderError> {
    if cache.partition_key.is_some() {
        return Err(ProviderError::InvalidRequest {
            message: "Anthropic Messages does not support a cache partition key".to_owned(),
        });
    }

    match cache.write {
        ModelCacheWritePolicy::ProviderDefault => {
            if cache.retention != ModelCacheRetention::ProviderDefault {
                return Err(ProviderError::InvalidRequest {
                    message: "Anthropic cache retention requires an explicit cache write policy"
                        .to_owned(),
                });
            }
        }
        ModelCacheWritePolicy::CacheThroughRequestEnd => {
            if body.contains_key("cache_control") {
                return Err(ProviderError::InvalidRequest {
                    message: "provider option cache_control conflicts with typed cache control"
                        .to_owned(),
                });
            }
            body.insert(
                "cache_control".to_owned(),
                anthropic_cache_control_value(cache)?,
            );
        }
        ModelCacheWritePolicy::ExplicitPrefix => {
            if cache.explicit_prefix_bytes.is_none() {
                return Err(ProviderError::InvalidRequest {
                    message: "explicit cache prefix policy requires a prefix byte boundary"
                        .to_owned(),
                });
            }
            if body.contains_key("cache_control") {
                return Err(ProviderError::InvalidRequest {
                    message: "provider option cache_control conflicts with typed cache control"
                        .to_owned(),
                });
            }
            // The cache_control marker is attached to the prefix content block below.
            let _ = anthropic_cache_control_value(cache)?;
        }
    }
    Ok(())
}

fn openai_initial_input(text: String, cache: &ModelCacheControl) -> Result<Value, ProviderError> {
    let Some((prefix, suffix)) = explicit_cache_prefix(&text, cache)? else {
        return Ok(serde_json::json!({"role": "user", "content": text}));
    };
    let mut content = vec![serde_json::json!({
        "type": "input_text",
        "text": prefix,
        "prompt_cache_breakpoint": {"mode": "explicit"}
    })];
    if !suffix.is_empty() {
        content.push(serde_json::json!({"type": "input_text", "text": suffix}));
    }
    Ok(serde_json::json!({"role": "user", "content": content}))
}

fn anthropic_initial_message(
    text: String,
    cache: &ModelCacheControl,
) -> Result<Value, ProviderError> {
    let Some((prefix, suffix)) = explicit_cache_prefix(&text, cache)? else {
        return Ok(serde_json::json!({"role": "user", "content": text}));
    };
    let mut content = vec![serde_json::json!({
        "type": "text",
        "text": prefix,
        "cache_control": anthropic_cache_control_value(cache)?
    })];
    if !suffix.is_empty() {
        content.push(serde_json::json!({"type": "text", "text": suffix}));
    }
    Ok(serde_json::json!({"role": "user", "content": content}))
}

fn apply_openai_responses_inference(body: &mut Map<String, Value>) -> Result<(), ProviderError> {
    let Some(effort) = take_inference_effort(body)? else {
        return Ok(());
    };
    if body.contains_key("reasoning") {
        return Err(ProviderError::InvalidRequest {
            message:
                "provider-neutral inference effort conflicts with provider option \"reasoning\""
                    .to_owned(),
        });
    }
    body.insert(
        "reasoning".to_owned(),
        serde_json::json!({ "effort": effort }),
    );
    Ok(())
}

fn apply_openai_chat_inference(body: &mut Map<String, Value>) -> Result<(), ProviderError> {
    let Some(effort) = take_inference_effort(body)? else {
        return Ok(());
    };
    if body.contains_key("reasoning_effort") {
        return Err(ProviderError::InvalidRequest {
            message:
                "provider-neutral inference effort conflicts with provider option \"reasoning_effort\""
                    .to_owned(),
        });
    }
    body.insert("reasoning_effort".to_owned(), effort);
    Ok(())
}

fn openai_responses_request(
    endpoint: &Endpoint,
    request: &ModelInferenceRequest,
) -> Result<ProviderRequest, ProviderError> {
    let (mut body, text) = request_object(request, &["model", "input", "tools"])?;
    apply_openai_responses_inference(&mut body)?;
    apply_openai_responses_cache(&mut body, request)?;
    body.insert(
        "model".to_owned(),
        Value::String(request.model.as_str().to_owned()),
    );
    if request.continuation.is_empty()
        && request.cache.write != ModelCacheWritePolicy::ExplicitPrefix
    {
        body.insert("input".to_owned(), Value::String(text));
    } else {
        let mut input = vec![openai_initial_input(text, &request.cache)?];
        for turn in &request.continuation {
            validate_tool_turn(turn)?;
            let text = assistant_text(turn)?;
            if !text.is_empty() {
                input.push(serde_json::json!({"role": "assistant", "content": text}));
            }
            for call in &turn.tool_calls {
                input.push(serde_json::json!({
                    "type": "function_call",
                    "call_id": call.call_id,
                    "name": openai_tool_name(&call.callable_id),
                    "arguments": tool_arguments(&request.tools, call)?,
                }));
            }
            for result in &turn.tool_results {
                input.push(serde_json::json!({
                    "type": "function_call_output",
                    "call_id": result.call_id,
                    "output": tool_output(result)?,
                }));
            }
        }
        body.insert("input".to_owned(), Value::Array(input));
    }
    if let Some(tools) = encode_tools(&request.tools, openai_tool)? {
        body.insert("tools".to_owned(), tools);
    }
    base_request(endpoint, "responses", Value::Object(body), json_headers())
}

fn openai_chat_request(
    endpoint: &Endpoint,
    request: &ModelInferenceRequest,
) -> Result<ProviderRequest, ProviderError> {
    let (mut body, text) = request_object(request, &["model", "messages", "tools"])?;
    apply_openai_chat_inference(&mut body)?;
    reject_nondefault_cache(&request.cache, "OpenAI Chat Completions")?;
    body.insert(
        "model".to_owned(),
        Value::String(request.model.as_str().to_owned()),
    );
    let mut messages = vec![serde_json::json!({"role":"user","content":text})];
    for turn in &request.continuation {
        validate_tool_turn(turn)?;
        let calls = turn
            .tool_calls
            .iter()
            .map(|call| {
                Ok(serde_json::json!({
                    "id": call.call_id,
                    "type": "function",
                    "function": {
                        "name": openai_tool_name(&call.callable_id),
                        "arguments": tool_arguments(&request.tools, call)?,
                    },
                }))
            })
            .collect::<Result<Vec<Value>, ProviderError>>()?;
        messages.push(serde_json::json!({
        "role": "assistant",
        "content": assistant_text(turn)?,
        "tool_calls": calls,
              }));
        for result in &turn.tool_results {
            messages.push(serde_json::json!({
                "role": "tool",
                "tool_call_id": result.call_id,
                "content": tool_output(result)?,
            }));
        }
    }
    body.insert("messages".to_owned(), Value::Array(messages));
    if let Some(tools) = encode_tools(&request.tools, openai_chat_tool)? {
        body.insert("tools".to_owned(), tools);
    }
    base_request(
        endpoint,
        "chat/completions",
        Value::Object(body),
        json_headers(),
    )
}

fn anthropic_request(
    endpoint: &Endpoint,
    request: &ModelInferenceRequest,
) -> Result<ProviderRequest, ProviderError> {
    let (mut body, text) = request_object(request, &["model", "messages", "tools"])?;
    body.insert(
        "model".to_owned(),
        Value::String(request.model.as_str().to_owned()),
    );
    apply_anthropic_cache(&mut body, &request.cache)?;
    let mut messages = vec![anthropic_initial_message(text, &request.cache)?];
    for turn in &request.continuation {
        validate_tool_turn(turn)?;
        let mut assistant = Vec::new();
        let text = assistant_text(turn)?;
        if !text.is_empty() {
            assistant.push(serde_json::json!({"type": "text", "text": text}));
        }
        for call in &turn.tool_calls {
            assistant.push(serde_json::json!({
                "type": "tool_use",
                "id": call.call_id,
                "name": call.callable_id.as_str(),
                "input": model_tool_input_for_call(&request.tools, call)?,
            }));
        }
        messages.push(serde_json::json!({"role": "assistant", "content": assistant}));
        let mut results = Vec::new();
        for result in &turn.tool_results {
            results.push(serde_json::json!({
                "type": "tool_result",
                "tool_use_id": result.call_id,
                "content": tool_output(result)?,
                "is_error": result.is_error,
            }));
        }
        messages.push(serde_json::json!({"role": "user", "content": results}));
    }
    body.insert("messages".to_owned(), Value::Array(messages));
    if let Some(tools) = encode_tools(&request.tools, anthropic_tool)? {
        body.insert("tools".to_owned(), tools);
    }
    body.entry("max_tokens".to_owned())
        .or_insert_with(|| Value::from(4096_u64));
    let mut headers = json_headers();
    headers.insert("anthropic-version".to_owned(), "2023-06-01".to_owned());
    base_request(endpoint, "messages", Value::Object(body), headers)
}

fn parse_json(response: &ProviderResponse) -> Result<Value, ProviderError> {
    serde_json::from_slice(&response.body).map_err(|error| ProviderError::Protocol {
        message: format!("cannot parse provider JSON response: {error}"),
    })
}

fn parse_callable_id(name: &str) -> Result<CallableId, ProviderError> {
    CallableId::parse(name).map_err(|error| ProviderError::Protocol {
        message: format!("provider returned invalid tool name {name:?}: {error}"),
    })
}

fn parse_arguments(
    value: &Value,
    provider: &str,
) -> Result<phenix_core::PhenixValue, ProviderError> {
    let value = if let Some(arguments) = value.as_str() {
        serde_json::from_str(arguments).map_err(|error| ProviderError::Protocol {
            message: format!("{provider} returned invalid tool arguments JSON: {error}"),
        })?
    } else {
        value.clone()
    };
    Ok(value.into())
}

fn incompatible_tool_input(provider: &str, message: impl Into<String>) -> ProviderError {
    ProviderError::Protocol {
        message: format!("{provider} returned incompatible tool input: {}", message.into()),
    }
}

fn into_tool_object(
    value: PhenixValue,
    provider: &str,
) -> Result<BTreeMap<String, PhenixValue>, ProviderError> {
    match value {
        PhenixValue::Map(values) => Ok(values),
        PhenixValue::Table(values) => Ok(values
            .into_iter()
            .map(|(key, value)| (key.as_str().to_owned(), value))
            .collect()),
        other => Err(incompatible_tool_input(
            provider,
            format!("expected object, got {}", other.kind()),
        )),
    }
}

fn project_tool_value(
    schema: &PhenixSchema,
    value: PhenixValue,
    provider: &str,
) -> Result<PhenixValue, ProviderError> {
    let projected = match schema {
        PhenixSchema::Any => value,
        PhenixSchema::Never => {
            return Err(incompatible_tool_input(
                provider,
                "declared schema does not accept any value",
            ));
        }
        PhenixSchema::Unit => match value {
            PhenixValue::Unit => PhenixValue::Unit,
            PhenixValue::Map(values) if values.is_empty() => PhenixValue::Unit,
            PhenixValue::Table(values) if values.is_empty() => PhenixValue::Unit,
            other => {
                return Err(incompatible_tool_input(
                    provider,
                    format!("expected empty object for unit input, got {}", other.kind()),
                ));
            }
        },
        PhenixSchema::Bool => match value {
            value @ PhenixValue::Bool(_) => value,
            other => {
                return Err(incompatible_tool_input(
                    provider,
                    format!("expected boolean, got {}", other.kind()),
                ));
            }
        },
        PhenixSchema::I64 => match value {
            value @ PhenixValue::I64(_) => value,
            PhenixValue::U64(value) => PhenixValue::I64(i64::try_from(value).map_err(|_| {
                incompatible_tool_input(provider, "integer does not fit signed 64-bit input")
            })?),
            other => {
                return Err(incompatible_tool_input(
                    provider,
                    format!("expected integer, got {}", other.kind()),
                ));
            }
        },
        PhenixSchema::U64 => match value {
            value @ PhenixValue::U64(_) => value,
            PhenixValue::I64(value) => PhenixValue::U64(u64::try_from(value).map_err(|_| {
                incompatible_tool_input(provider, "integer is negative for unsigned input")
            })?),
            other => {
                return Err(incompatible_tool_input(
                    provider,
                    format!("expected unsigned integer, got {}", other.kind()),
                ));
            }
        },
        PhenixSchema::F64 => match value {
            value @ PhenixValue::F64(_) => value,
            PhenixValue::I64(value) => PhenixValue::F64(
                value
                    .to_string()
                    .parse()
                    .expect("i64 string always parses as finite f64"),
            ),
            PhenixValue::U64(value) => PhenixValue::F64(
                value
                    .to_string()
                    .parse()
                    .expect("u64 string always parses as finite f64"),
            ),
            other => {
                return Err(incompatible_tool_input(
                    provider,
                    format!("expected number, got {}", other.kind()),
                ));
            }
        },
        PhenixSchema::String => match value {
            value @ PhenixValue::String(_) => value,
            other => {
                return Err(incompatible_tool_input(
                    provider,
                    format!("expected string, got {}", other.kind()),
                ));
            }
        },
        PhenixSchema::Bytes => match value {
            value @ PhenixValue::Bytes(_) => value,
            PhenixValue::String(value) => PhenixValue::Bytes(
                BASE64_STANDARD.decode(value).map_err(|error| {
                    incompatible_tool_input(provider, format!("invalid base64 bytes: {error}"))
                })?,
            ),
            other => {
                return Err(incompatible_tool_input(
                    provider,
                    format!("expected base64 string, got {}", other.kind()),
                ));
            }
        },
        PhenixSchema::Option(item) => match value {
            PhenixValue::Unit => PhenixValue::Option(None),
            PhenixValue::Option(None) => PhenixValue::Option(None),
            PhenixValue::Option(Some(value)) => PhenixValue::Option(Some(Box::new(
                project_tool_value(item, *value, provider)?,
            ))),
            value => PhenixValue::Option(Some(Box::new(project_tool_value(
                item, value, provider,
            )?))),
        },
        PhenixSchema::Array { item, .. } | PhenixSchema::List(item) => {
            let PhenixValue::List(values) = value else {
                return Err(incompatible_tool_input(
                    provider,
                    format!("expected array, got {}", value.kind()),
                ));
            };
            PhenixValue::List(
                values
                    .into_iter()
                    .map(|value| project_tool_value(item, value, provider))
                    .collect::<Result<Vec<_>, _>>()?,
            )
        }
        PhenixSchema::Map(item) => {
            let values = into_tool_object(value, provider)?;
            PhenixValue::Map(
                values
                    .into_iter()
                    .map(|(key, value)| {
                        project_tool_value(item, value, provider).map(|value| (key, value))
                    })
                    .collect::<Result<BTreeMap<_, _>, _>>()?,
            )
        }
        PhenixSchema::Table(fields) => {
            let mut values = into_tool_object(value, provider)?;
            let mut projected = BTreeMap::new();
            for (key, field_schema) in fields {
                let value = values.remove(key.as_str()).ok_or_else(|| {
                    incompatible_tool_input(provider, format!("missing field {}", key.as_str()))
                })?;
                projected.insert(
                    key.clone(),
                    project_tool_value(field_schema, value, provider)?,
                );
            }
            if let Some(key) = values.keys().next() {
                return Err(incompatible_tool_input(
                    provider,
                    format!("unexpected field {key}"),
                ));
            }
            PhenixValue::Table(projected)
        }
        PhenixSchema::Variant(variants) => {
            if let PhenixValue::Variant { tag, value } = value {
                let payload_schema = variants.get(&tag).ok_or_else(|| {
                    incompatible_tool_input(provider, format!("unknown variant {tag}"))
                })?;
                PhenixValue::Variant {
                    tag,
                    value: Box::new(project_tool_value(payload_schema, *value, provider)?),
                }
            } else {
                let mut values = into_tool_object(value, provider)?;
                let tag = match values.remove("tag") {
                    Some(PhenixValue::String(tag)) => Key::parse(tag).map_err(|error| {
                        incompatible_tool_input(provider, format!("invalid variant tag: {error}"))
                    })?,
                    Some(other) => {
                        return Err(incompatible_tool_input(
                            provider,
                            format!("variant tag must be a string, got {}", other.kind()),
                        ));
                    }
                    None => {
                        return Err(incompatible_tool_input(provider, "variant has no tag"));
                    }
                };
                let value = values
                    .remove("value")
                    .ok_or_else(|| incompatible_tool_input(provider, "variant has no value"))?;
                if let Some(key) = values.keys().next() {
                    return Err(incompatible_tool_input(
                        provider,
                        format!("unexpected variant field {key}"),
                    ));
                }
                let payload_schema = variants.get(&tag).ok_or_else(|| {
                    incompatible_tool_input(provider, format!("unknown variant {tag}"))
                })?;
                PhenixValue::Variant {
                    tag,
                    value: Box::new(project_tool_value(payload_schema, value, provider)?),
                }
            }
        }
        PhenixSchema::Callable { .. } | PhenixSchema::Object { .. } => {
            return Err(incompatible_tool_input(
                provider,
                "opaque capability references cannot cross the model boundary",
            ));
        }
    };
    schema
        .parse(&projected)
        .map_err(|error| incompatible_tool_input(provider, error.to_string()))?;
    Ok(projected)
}

fn project_model_tool_input(
    schema: &PhenixSchema,
    value: PhenixValue,
    provider: &str,
) -> Result<PhenixValue, ProviderError> {
    match schema {
        PhenixSchema::Unit
        | PhenixSchema::Table(_)
        | PhenixSchema::Map(_)
        | PhenixSchema::Variant(_) => project_tool_value(schema, value, provider),
        _ => {
            let mut values = into_tool_object(value, provider)?;
            let value = values
                .remove("value")
                .ok_or_else(|| incompatible_tool_input(provider, "input envelope has no value"))?;
            if let Some(key) = values.keys().next() {
                return Err(incompatible_tool_input(
                    provider,
                    format!("unexpected input envelope field {key}"),
                ));
            }
            project_tool_value(schema, value, provider)
        }
    }
}

fn normalize_model_tool_calls(
    request: &ModelInferenceRequest,
    response: &mut ModelInferenceResponse,
    provider: &str,
) -> Result<(), ProviderError> {
    for call in &mut response.tool_calls {
        let tool = request
            .tools
            .iter()
            .find(|tool| tool.id == call.callable_id)
            .ok_or_else(|| ProviderError::Protocol {
                message: format!(
                    "{provider} returned undeclared tool call {}",
                    call.callable_id
                ),
            })?;
        let input = std::mem::replace(&mut call.input, PhenixValue::Unit);
        call.input = project_model_tool_input(&tool.input_schema, input, provider)?;
    }
    Ok(())
}

const OPENAI_USAGE_MAPPING_REVISION: &str = "openai-inclusive-input-v1";
const ANTHROPIC_USAGE_MAPPING_REVISION: &str = "anthropic-exclusive-input-v1";

fn reported(value: Option<u64>) -> UsageQuantity {
    value
        .map(|value| UsageQuantity::Reported { value })
        .unwrap_or_default()
}

fn inclusive_input_usage(
    total_input: Option<u64>,
    cache_read: Option<u64>,
    cache_write: Option<u64>,
) -> Result<(UsageQuantity, UsageQuantity, UsageQuantity), ProviderError> {
    let known_cached = cache_read
        .unwrap_or(0)
        .checked_add(cache_write.unwrap_or(0))
        .ok_or_else(|| ProviderError::Protocol {
            message: "provider cache usage overflowed input accounting".to_owned(),
        })?;
    let fresh_input = match total_input {
        Some(total) if known_cached <= total => reported(Some(total - known_cached)),
        Some(total) => {
            return Err(ProviderError::Protocol {
                message: format!(
                    "provider cache usage exceeds total input: total={total}, cache_read={}, cache_write={}",
                    cache_read.unwrap_or(0),
                    cache_write.unwrap_or(0)
                ),
            });
        }
        None => UsageQuantity::Unavailable,
    };
    Ok((fresh_input, reported(cache_read), reported(cache_write)))
}

fn openai_responses_usage(value: &Value) -> Result<ModelTurnUsage, ProviderError> {
    let total_input = value.pointer("/usage/input_tokens").and_then(Value::as_u64);
    let cache_read = value
        .pointer("/usage/input_tokens_details/cached_tokens")
        .and_then(Value::as_u64);
    let cache_write = value
        .pointer("/usage/input_tokens_details/cache_write_tokens")
        .and_then(Value::as_u64);
    let (fresh_input_tokens, cache_read_tokens, cache_write_tokens) =
        inclusive_input_usage(total_input, cache_read, cache_write)?;
    Ok(ModelTurnUsage {
        fresh_input_tokens,
        cache_read_tokens,
        cache_write_tokens,
        output_tokens: reported(
            value
                .pointer("/usage/output_tokens")
                .and_then(Value::as_u64),
        ),
        reasoning_tokens: reported(
            value
                .pointer("/usage/output_tokens_details/reasoning_tokens")
                .and_then(Value::as_u64),
        ),
    })
}

fn openai_chat_usage(value: &Value) -> Result<ModelTurnUsage, ProviderError> {
    let total_input = value
        .pointer("/usage/prompt_tokens")
        .and_then(Value::as_u64);
    let cache_read = value
        .pointer("/usage/prompt_tokens_details/cached_tokens")
        .and_then(Value::as_u64);
    let cache_write = value
        .pointer("/usage/prompt_tokens_details/cache_write_tokens")
        .and_then(Value::as_u64);
    let (fresh_input_tokens, cache_read_tokens, cache_write_tokens) =
        inclusive_input_usage(total_input, cache_read, cache_write)?;
    Ok(ModelTurnUsage {
        fresh_input_tokens,
        cache_read_tokens,
        cache_write_tokens,
        output_tokens: reported(
            value
                .pointer("/usage/completion_tokens")
                .and_then(Value::as_u64),
        ),
        reasoning_tokens: reported(
            value
                .pointer("/usage/completion_tokens_details/reasoning_tokens")
                .and_then(Value::as_u64),
        ),
    })
}

fn anthropic_usage(value: &Value) -> ModelTurnUsage {
    ModelTurnUsage {
        fresh_input_tokens: reported(value.pointer("/usage/input_tokens").and_then(Value::as_u64)),
        cache_read_tokens: reported(
            value
                .pointer("/usage/cache_read_input_tokens")
                .and_then(Value::as_u64),
        ),
        cache_write_tokens: reported(
            value
                .pointer("/usage/cache_creation_input_tokens")
                .and_then(Value::as_u64),
        ),
        output_tokens: reported(
            value
                .pointer("/usage/output_tokens")
                .and_then(Value::as_u64),
        ),
        reasoning_tokens: UsageQuantity::Unavailable,
    }
}

fn response_with_content(
    value: &Value,
    text: String,
    tool_calls: Vec<ModelToolCall>,
    usage: ModelTurnUsage,
    usage_mapping_revision: &str,
) -> ModelInferenceResponse {
    let mut provider_metadata = BTreeMap::new();
    if let Some(id) = value.get("id").cloned() {
        provider_metadata.insert("id".to_owned(), id.into());
    }
    if let Some(raw_usage) = value.get("usage").cloned() {
        provider_metadata.insert("usage".to_owned(), raw_usage.into());
    }
    provider_metadata.insert(
        "usage_mapping_revision".to_owned(),
        Value::String(usage_mapping_revision.to_owned()).into(),
    );
    ModelInferenceResponse {
        output: text.into_bytes().into(),
        provider_metadata,
        usage: Box::new(usage),
        tool_calls,
    }
}

fn openai_responses_response(
    response: &ProviderResponse,
) -> Result<ModelInferenceResponse, ProviderError> {
    let value = parse_json(response)?;
    let output = value
        .get("output")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut text = value
        .get("output_text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if text.is_empty() {
        text = output
            .iter()
            .filter_map(|item| item.get("content").and_then(Value::as_array))
            .flatten()
            .filter_map(|part| {
                part.get("text")
                    .and_then(Value::as_str)
                    .or_else(|| part.get("output_text").and_then(Value::as_str))
            })
            .collect::<Vec<_>>()
            .join("");
    }
    let tool_calls = output
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("function_call"))
        .map(|item| {
            let call_id = item
                .get("call_id")
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
                .ok_or_else(|| ProviderError::Protocol {
                    message: "OpenAI responses tool call contained no call id".to_owned(),
                })?;
            let name = item.get("name").and_then(Value::as_str).ok_or_else(|| {
                ProviderError::Protocol {
                    message: "OpenAI responses tool call contained no function name".to_owned(),
                }
            })?;
            let arguments = item
                .get("arguments")
                .ok_or_else(|| ProviderError::Protocol {
                    message: "OpenAI responses tool call contained no arguments".to_owned(),
                })?;
            Ok(ModelToolCall {
                call_id: call_id.to_owned(),
                callable_id: parse_openai_callable_id(name)?,
                input: parse_arguments(arguments, "OpenAI responses")?,
            })
        })
        .collect::<Result<Vec<_>, ProviderError>>()?;
    if text.is_empty() && tool_calls.is_empty() {
        return Err(ProviderError::Protocol {
            message: "OpenAI responses payload contained neither output text nor tool calls"
                .to_owned(),
        });
    }
    Ok(response_with_content(
        &value,
        text,
        tool_calls,
        openai_responses_usage(&value)?,
        OPENAI_USAGE_MAPPING_REVISION,
    ))
}

fn openai_chat_response(
    response: &ProviderResponse,
) -> Result<ModelInferenceResponse, ProviderError> {
    let value = parse_json(response)?;
    let message = value
        .pointer("/choices/0/message")
        .and_then(Value::as_object)
        .ok_or_else(|| ProviderError::Protocol {
            message: "OpenAI chat payload contained no first choice message".to_owned(),
        })?;
    let content = message.get("content").unwrap_or(&Value::Null);
    let text = if let Some(text) = content.as_str() {
        text.to_owned()
    } else {
        content
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("")
    };
    let tool_calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|call| {
            let call_id =
                call.get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ProviderError::Protocol {
                        message: "OpenAI chat tool call contained no call id".to_owned(),
                    })?;
            let function = call
                .get("function")
                .and_then(Value::as_object)
                .ok_or_else(|| ProviderError::Protocol {
                    message: "OpenAI chat tool call contained no function".to_owned(),
                })?;
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| ProviderError::Protocol {
                    message: "OpenAI chat tool call contained no function name".to_owned(),
                })?;
            let arguments = function
                .get("arguments")
                .ok_or_else(|| ProviderError::Protocol {
                    message: "OpenAI chat tool call contained no arguments".to_owned(),
                })?;
            Ok(ModelToolCall {
                call_id: call_id.to_owned(),
                callable_id: parse_openai_callable_id(name)?,
                input: parse_arguments(arguments, "OpenAI chat")?,
            })
        })
        .collect::<Result<Vec<_>, ProviderError>>()?;
    if text.is_empty() && tool_calls.is_empty() {
        return Err(ProviderError::Protocol {
            message: "OpenAI chat payload contained neither content nor tool calls".to_owned(),
        });
    }
    Ok(response_with_content(
        &value,
        text,
        tool_calls,
        openai_chat_usage(&value)?,
        OPENAI_USAGE_MAPPING_REVISION,
    ))
}

fn anthropic_response(
    response: &ProviderResponse,
) -> Result<ModelInferenceResponse, ProviderError> {
    let value = parse_json(response)?;
    let content = value
        .get("content")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let text = content
        .iter()
        .filter_map(|part| {
            (part.get("type").and_then(Value::as_str) == Some("text"))
                .then(|| part.get("text").and_then(Value::as_str))
                .flatten()
        })
        .collect::<Vec<_>>()
        .join("");
    let tool_calls = content
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("tool_use"))
        .map(|part| {
            let call_id =
                part.get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ProviderError::Protocol {
                        message: "Anthropic tool use contained no call id".to_owned(),
                    })?;
            let name = part.get("name").and_then(Value::as_str).ok_or_else(|| {
                ProviderError::Protocol {
                    message: "Anthropic tool use contained no tool name".to_owned(),
                }
            })?;
            let input = part.get("input").ok_or_else(|| ProviderError::Protocol {
                message: "Anthropic tool use contained no input".to_owned(),
            })?;
            Ok(ModelToolCall {
                call_id: call_id.to_owned(),
                callable_id: parse_callable_id(name)?,
                input: parse_arguments(input, "Anthropic")?,
            })
        })
        .collect::<Result<Vec<_>, ProviderError>>()?;
    if text.is_empty() && tool_calls.is_empty() {
        return Err(ProviderError::Protocol {
            message: "Anthropic messages payload contained neither text content nor tool use"
                .to_owned(),
        });
    }
    Ok(response_with_content(
        &value,
        text,
        tool_calls,
        anthropic_usage(&value),
        ANTHROPIC_USAGE_MAPPING_REVISION,
    ))
}

pub fn normalize_http_error(response: &ProviderResponse) -> ProviderError {
    let message = error_message(&response.body);
    let normalized = message.to_ascii_lowercase();
    if response.status == 413
        || [
            "context_length_exceeded",
            "maximum context length",
            "context window",
            "too many tokens",
            "input is too long",
            "prompt is too long",
        ]
        .iter()
        .any(|needle| normalized.contains(needle))
    {
        return ProviderError::ContextLimit { message };
    }
    match response.status {
        400 | 409 | 422 => ProviderError::InvalidRequest { message },
        401 => ProviderError::Authentication { message },
        403 => ProviderError::Permission { message },
        404 => ProviderError::NotFound { message },
        408 | 425 | 500..=599 => ProviderError::Unavailable { message },
        429 => ProviderError::RateLimited {
            message,
            limits: Box::new(RateLimits::from_headers(&response.headers)),
        },
        _ => ProviderError::Protocol {
            message: format!("HTTP {}: {message}", response.status),
        },
    }
}

fn error_message(body: &[u8]) -> String {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        let text = String::from_utf8_lossy(body).trim().to_owned();
        return if text.is_empty() {
            "provider request failed".to_owned()
        } else {
            text
        };
    };
    value
        .pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| value.get("message").and_then(Value::as_str))
        .or_else(|| value.get("detail").and_then(Value::as_str))
        .or_else(|| value.pointer("/error/code").and_then(Value::as_str))
        .or_else(|| value.pointer("/error/type").and_then(Value::as_str))
        .or_else(|| value.get("error").and_then(Value::as_str))
        .unwrap_or("provider request failed")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DurationMs, ProviderResponse};
    use phenix_core::{Key, PhenixValue, SessionId};
    use std::collections::BTreeMap;

    fn request() -> ModelInferenceRequest {
        ModelInferenceRequest {
            session_id: None,
            model: phenix_core::ModelId::parse("test-model").unwrap(),
            input: b"hello".to_vec().into(),
            options: BTreeMap::new(),
            cache: ModelCacheControl::default(),
            tools: Vec::new(),
            continuation: Vec::new(),
        }
    }

    fn request_for_model(model: &str) -> ModelInferenceRequest {
        let mut request = request();
        request.model = phenix_core::ModelId::parse(model).unwrap();
        request.session_id = Some(SessionId::parse("session-test").unwrap());
        request
    }

    fn tool() -> ModelToolDescriptor {
        ModelToolDescriptor {
            id: CallableId::parse("fixture.echo").unwrap(),
            description: "Echo a value".to_owned(),
            input_schema: PhenixSchema::Table(BTreeMap::from([(
                Key::parse("value").unwrap(),
                PhenixSchema::String,
            )])),
            output_schema: PhenixSchema::String,
        }
    }

    fn request_with_tool() -> ModelInferenceRequest {
        let mut request = request();
        request.tools.push(tool());
        request
    }

    fn unit_tool() -> ModelToolDescriptor {
        ModelToolDescriptor {
            id: CallableId::parse("fixture.unit").unwrap(),
            description: "No-argument tool".to_owned(),
            input_schema: PhenixSchema::Unit,
            output_schema: PhenixSchema::Unit,
        }
    }

    fn request_with_unit_tool() -> ModelInferenceRequest {
        let mut request = request();
        request.tools.push(unit_tool());
        request
    }

    fn response(status: u16, headers: &[(&str, &str)], body: Value) -> ProviderResponse {
        ProviderResponse {
            status,
            headers: headers
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
            body: serde_json::to_vec(&body).unwrap(),
        }
    }

    #[test]
    fn openai_protocols_lower_provider_neutral_inference_effort() {
        let endpoint = Endpoint::parse("https://example.com/v1").unwrap();
        let mut request = request();
        request.options.insert(
            "inference".to_owned(),
            serde_json::json!({"effort": "medium"}).into(),
        );

        let encoded = Protocol::OpenAiResponses
            .encode(&endpoint, &request)
            .unwrap();
        let body: Value = serde_json::from_slice(&encoded.body).unwrap();
        assert_eq!(body["reasoning"]["effort"], "medium");
        assert!(body.get("inference").is_none());

        let encoded = Protocol::OpenAiChatCompletions
            .encode(&endpoint, &request)
            .unwrap();
        let body: Value = serde_json::from_slice(&encoded.body).unwrap();
        assert_eq!(body["reasoning_effort"], "medium");
        assert!(body.get("inference").is_none());
    }

    #[test]
    fn openai_responses_maps_typed_cache_controls_for_supported_models() {
        let endpoint = Endpoint::parse("https://example.com/v1").unwrap();
        let mut request = request_for_model("gpt-5.6-sol");
        request.cache = ModelCacheControl {
            write: ModelCacheWritePolicy::CacheThroughRequestEnd,
            retention: ModelCacheRetention::ThirtyMinutes,
            partition_key: Some("workspace-1".into()),
            explicit_prefix_bytes: None,
            local_prefix_identity: None,
            local_feature_generation: None,
            local_authority_identity: None,
        };

        let encoded = Protocol::OpenAiResponses
            .encode(&endpoint, &request)
            .unwrap();
        let body: Value = serde_json::from_slice(&encoded.body).unwrap();
        assert_eq!(body["prompt_cache_key"], "workspace-1");
        assert_eq!(body["prompt_cache_options"]["mode"], "implicit");
        assert_eq!(body["prompt_cache_options"]["ttl"], "30m");
    }

    #[test]
    fn openai_responses_places_explicit_breakpoint_at_stable_prefix_boundary() {
        let endpoint = Endpoint::parse("https://example.com/v1").unwrap();
        let mut request = request_for_model("gpt-5.6-sol");
        request.input = b"stablevolatile".to_vec().into();
        request.cache = ModelCacheControl {
            write: ModelCacheWritePolicy::ExplicitPrefix,
            retention: ModelCacheRetention::ThirtyMinutes,
            partition_key: None,
            explicit_prefix_bytes: Some(6),
            local_prefix_identity: Some("sha256:prefix".into()),
            local_feature_generation: None,
            local_authority_identity: None,
        };

        let encoded = Protocol::OpenAiResponses
            .encode(&endpoint, &request)
            .unwrap();
        let body: Value = serde_json::from_slice(&encoded.body).unwrap();
        assert_eq!(body["prompt_cache_options"]["mode"], "explicit");
        assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(body["input"][0]["content"][0]["text"], "stable");
        assert_eq!(
            body["input"][0]["content"][0]["prompt_cache_breakpoint"]["mode"],
            "explicit"
        );
        assert_eq!(body["input"][0]["content"][1]["text"], "volatile");
    }

    #[test]
    fn anthropic_places_cache_control_on_explicit_prefix_block() {
        let endpoint = Endpoint::parse("https://example.com/v1").unwrap();
        let mut request = request_for_model("claude-sonnet-5");
        request.input = b"stablevolatile".to_vec().into();
        request.cache = ModelCacheControl {
            write: ModelCacheWritePolicy::ExplicitPrefix,
            retention: ModelCacheRetention::OneHour,
            partition_key: None,
            explicit_prefix_bytes: Some(6),
            local_prefix_identity: Some("sha256:prefix".into()),
            local_feature_generation: None,
            local_authority_identity: None,
        };

        let encoded = Protocol::AnthropicMessages
            .encode(&endpoint, &request)
            .unwrap();
        let body: Value = serde_json::from_slice(&encoded.body).unwrap();
        assert!(body.get("cache_control").is_none());
        assert_eq!(body["messages"][0]["content"][0]["type"], "text");
        assert_eq!(body["messages"][0]["content"][0]["text"], "stable");
        assert_eq!(
            body["messages"][0]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
        assert_eq!(
            body["messages"][0]["content"][0]["cache_control"]["ttl"],
            "1h"
        );
        assert_eq!(body["messages"][0]["content"][1]["text"], "volatile");
    }

    #[test]
    fn explicit_prefix_requires_a_valid_utf8_boundary() {
        let endpoint = Endpoint::parse("https://example.com/v1").unwrap();
        let mut request = request_for_model("gpt-5.6-sol");
        request.input = "évolatile".as_bytes().to_vec().into();
        request.cache = ModelCacheControl {
            write: ModelCacheWritePolicy::ExplicitPrefix,
            retention: ModelCacheRetention::ProviderDefault,
            partition_key: None,
            explicit_prefix_bytes: Some(1),
            local_prefix_identity: None,
            local_feature_generation: None,
            local_authority_identity: None,
        };

        assert!(matches!(
            Protocol::OpenAiResponses.encode(&endpoint, &request),
            Err(ProviderError::InvalidRequest { .. })
        ));
    }

    #[test]
    fn anthropic_maps_request_end_cache_control_and_ttl() {
        let endpoint = Endpoint::parse("https://example.com/v1").unwrap();
        let mut request = request_for_model("claude-sonnet-5");
        request.cache = ModelCacheControl {
            write: ModelCacheWritePolicy::CacheThroughRequestEnd,
            retention: ModelCacheRetention::OneHour,
            partition_key: None,
            explicit_prefix_bytes: None,
            local_prefix_identity: None,
            local_feature_generation: None,
            local_authority_identity: None,
        };

        let encoded = Protocol::AnthropicMessages
            .encode(&endpoint, &request)
            .unwrap();
        let body: Value = serde_json::from_slice(&encoded.body).unwrap();
        assert_eq!(body["cache_control"]["type"], "ephemeral");
        assert_eq!(body["cache_control"]["ttl"], "1h");
    }

    #[test]
    fn unsupported_cache_controls_fail_instead_of_being_ignored() {
        let endpoint = Endpoint::parse("https://example.com/v1").unwrap();

        let mut anthropic = request_for_model("claude-sonnet-5");
        anthropic.cache = ModelCacheControl {
            write: ModelCacheWritePolicy::CacheThroughRequestEnd,
            retention: ModelCacheRetention::ThirtyMinutes,
            partition_key: None,
            explicit_prefix_bytes: None,
            local_prefix_identity: None,
            local_feature_generation: None,
            local_authority_identity: None,
        };
        assert!(matches!(
            Protocol::AnthropicMessages.encode(&endpoint, &anthropic),
            Err(ProviderError::InvalidRequest { .. })
        ));

        let mut chat = request_for_model("legacy-chat-model");
        chat.cache.write = ModelCacheWritePolicy::CacheThroughRequestEnd;
        assert!(matches!(
            Protocol::OpenAiChatCompletions.encode(&endpoint, &chat),
            Err(ProviderError::InvalidRequest { .. })
        ));
    }

    #[test]
    fn inclusive_cache_usage_separates_fresh_reads_and_writes() {
        let (fresh, reads, writes) =
            inclusive_input_usage(Some(1_000), Some(600), Some(100)).unwrap();
        assert_eq!(fresh.value(), Some(300));
        assert_eq!(reads.value(), Some(600));
        assert_eq!(writes.value(), Some(100));
    }

    #[test]
    fn inclusive_cache_usage_rejects_impossible_subsets() {
        assert!(matches!(
            inclusive_input_usage(Some(100), Some(90), Some(20)),
            Err(ProviderError::Protocol { .. })
        ));
    }

    #[test]
    fn absent_cache_breakdown_stays_unavailable() {
        let (fresh, reads, writes) = inclusive_input_usage(Some(300), None, None).unwrap();
        assert_eq!(fresh.value(), Some(300));
        assert_eq!(reads, UsageQuantity::Unavailable);
        assert_eq!(writes, UsageQuantity::Unavailable);
    }

    #[test]
    fn openai_cache_usage_treats_cached_tokens_as_inclusive_input_subset() {
        let decoded = Protocol::OpenAiResponses
            .decode(&response(
                200,
                &[],
                serde_json::json!({
                    "output":[{"content":[{"type":"output_text","text":"ok"}]}],
                    "usage":{
                        "input_tokens":1000,
                        "input_tokens_details":{"cached_tokens":600,"cache_write_tokens":100},
                        "output_tokens":1
                    }
                }),
            ))
            .unwrap();

        assert_eq!(decoded.usage.fresh_input_tokens.value(), Some(300));
        assert_eq!(decoded.usage.cache_read_tokens.value(), Some(600));
        assert_eq!(decoded.usage.cache_write_tokens.value(), Some(100));
        assert_eq!(
            decoded.provider_metadata["usage_mapping_revision"],
            PhenixValue::String(OPENAI_USAGE_MAPPING_REVISION.into())
        );
        assert!(decoded.provider_metadata.contains_key("usage"));
    }

    #[test]
    fn anthropic_cache_usage_keeps_fresh_input_exclusive() {
        let decoded = Protocol::AnthropicMessages
            .decode(&response(
                200,
                &[],
                serde_json::json!({
                    "content":[{"type":"text","text":"ok"}],
                    "usage":{
                        "input_tokens":300,
                        "cache_read_input_tokens":600,
                        "cache_creation_input_tokens":100,
                        "output_tokens":1
                    }
                }),
            ))
            .unwrap();

        assert_eq!(decoded.usage.fresh_input_tokens.value(), Some(300));
        assert_eq!(decoded.usage.cache_read_tokens.value(), Some(600));
        assert_eq!(decoded.usage.cache_write_tokens.value(), Some(100));
        assert_eq!(
            decoded.provider_metadata["usage_mapping_revision"],
            PhenixValue::String(ANTHROPIC_USAGE_MAPPING_REVISION.into())
        );
        assert!(decoded.provider_metadata.contains_key("usage"));
    }

    #[test]
    fn openai_cache_usage_rejects_cached_tokens_above_total_input() {
        assert!(matches!(
            Protocol::OpenAiResponses.decode(&response(
                200,
                &[],
                serde_json::json!({
                    "output":[{"content":[{"type":"output_text","text":"ok"}]}],
                    "usage":{
                        "input_tokens":100,
                        "input_tokens_details":{"cached_tokens":101},
                        "output_tokens":1
                    }
                }),
            )),
            Err(ProviderError::Protocol { .. })
        ));
    }

    #[test]
    fn openai_responses_maps_internal_request_and_response() {
        let endpoint = Endpoint::parse("https://example.com/v1").unwrap();
        let encoded = Protocol::OpenAiResponses
            .encode(&endpoint, &request())
            .unwrap();
        assert_eq!(encoded.url, "https://example.com/v1/responses");
        let body: Value = serde_json::from_slice(&encoded.body).unwrap();
        assert_eq!(body["model"], "test-model");
        assert_eq!(body["input"], "hello");

        let decoded = Protocol::OpenAiResponses
            .decode(&response(
                200,
                &[],
                serde_json::json!({
                    "id":"response-1",
                    "output":[{"content":[{"type":"output_text","text":"world"}]}],
                    "usage":{"input_tokens":1,"output_tokens":1}
                }),
            ))
            .unwrap();
        assert_eq!(decoded.output.as_ref(), b"world");
        assert_eq!(decoded.usage.fresh_input_tokens.value(), Some(1));
        assert_eq!(decoded.usage.output_tokens.value(), Some(1));
        assert_eq!(
            decoded.provider_metadata["id"],
            PhenixValue::String("response-1".into())
        );
    }

    #[test]
    fn opencode_go_selects_wire_protocol_by_model() {
        let endpoint = Endpoint::parse("https://opencode.ai/zen/go/v1").unwrap();
        for (model, path) in [
            ("gpt-5.6-luna", "/zen/go/v1/responses"),
            ("qwen3.7-plus", "/zen/go/v1/messages"),
            ("minimax-m3", "/zen/go/v1/messages"),
            ("deepseek-v4-flash", "/zen/go/v1/chat/completions"),
            ("mimo-v2.5", "/zen/go/v1/chat/completions"),
        ] {
            let encoded = Protocol::OpenCodeGo
                .encode(&endpoint, &request_for_model(model))
                .unwrap();
            assert!(
                encoded.url.ends_with(path),
                "{model} routed to unexpected URL {}",
                encoded.url
            );
        }
    }

    #[test]
    fn opencode_go_forwards_stable_session_identity_and_user_agent() {
        let endpoint = Endpoint::parse("https://opencode.ai/zen/go/v1").unwrap();
        let mut request = request_for_model("qwen3.7-plus");
        request.session_id = Some(SessionId::parse("session-a").unwrap());

        let first = Protocol::OpenCodeGo.encode(&endpoint, &request).unwrap();
        let second = Protocol::OpenCodeGo.encode(&endpoint, &request).unwrap();
        assert_eq!(
            first
                .headers
                .get(OPENCODE_SESSION_HEADER)
                .map(String::as_str),
            Some("session-a")
        );
        assert_eq!(
            second.headers.get(OPENCODE_SESSION_HEADER),
            first.headers.get(OPENCODE_SESSION_HEADER)
        );
        assert_eq!(
            first.headers.get(USER_AGENT.as_str()).map(String::as_str),
            Some(PHENIX_USER_AGENT)
        );

        request.session_id = Some(SessionId::parse("session-b").unwrap());
        let other = Protocol::OpenCodeGo.encode(&endpoint, &request).unwrap();
        assert_eq!(
            other
                .headers
                .get(OPENCODE_SESSION_HEADER)
                .map(String::as_str),
            Some("session-b")
        );

        let mut missing = request_for_model("qwen3.7-plus");
        missing.session_id = None;
        let missing = Protocol::OpenCodeGo.encode(&endpoint, &missing).unwrap();
        assert!(!missing.headers.contains_key(OPENCODE_SESSION_HEADER));
        assert_eq!(
            missing.headers.get(USER_AGENT.as_str()).map(String::as_str),
            Some(PHENIX_USER_AGENT)
        );
    }

    #[test]
    fn opencode_zen_selects_wire_protocol_by_model() {
        let endpoint = Endpoint::parse("https://opencode.ai/zen/v1").unwrap();
        for (model, path) in [
            ("gpt-5.6-terra", "/zen/v1/responses"),
            ("grok-4", "/zen/v1/responses"),
            ("claude-sonnet-5", "/zen/v1/messages"),
            ("qwen3.7-plus", "/zen/v1/messages"),
            ("mimo-v2.5-free", "/zen/v1/chat/completions"),
        ] {
            let encoded = Protocol::OpenCodeZen
                .encode(&endpoint, &request_for_model(model))
                .unwrap();
            assert!(
                encoded.url.ends_with(path),
                "{model} routed to unexpected URL {}",
                encoded.url
            );
        }
        assert!(matches!(
            Protocol::OpenCodeZen.encode(&endpoint, &request_for_model("gemini-3-pro")),
            Err(ProviderError::InvalidRequest { .. })
        ));
    }

    #[test]
    fn opencode_protocol_decodes_supported_response_shapes() {
        let responses = Protocol::OpenCodeGo
            .decode(&response(
                200,
                &[],
                serde_json::json!({
                    "output":[{"content":[{"type":"output_text","text":"responses"}]}]
                }),
            ))
            .unwrap();
        assert_eq!(responses.output.as_ref(), b"responses");

        let chat = Protocol::OpenCodeGo
            .decode(&response(
                200,
                &[],
                serde_json::json!({
                    "choices":[{"message":{"content":"chat"}}]
                }),
            ))
            .unwrap();
        assert_eq!(chat.output.as_ref(), b"chat");

        let anthropic = Protocol::OpenCodeGo
            .decode(&response(
                200,
                &[],
                serde_json::json!({
                    "content":[{"type":"text","text":"anthropic"}]
                }),
            ))
            .unwrap();
        assert_eq!(anthropic.output.as_ref(), b"anthropic");
    }

    #[test]
    fn provider_protocols_encode_unit_tool_inputs_as_empty_objects() {
        let endpoint = Endpoint::parse("https://example.com/v1").unwrap();
        let request = request_with_unit_tool();

        let responses = Protocol::OpenAiResponses
            .encode(&endpoint, &request)
            .unwrap();
        let responses: Value = serde_json::from_slice(&responses.body).unwrap();
        assert_eq!(responses["tools"][0]["parameters"]["type"], "object");
        assert_eq!(responses["tools"][0]["parameters"]["properties"], serde_json::json!({}));
        assert_eq!(responses["tools"][0]["parameters"]["required"], serde_json::json!([]));
        assert_eq!(responses["tools"][0]["parameters"]["additionalProperties"], false);

        let chat = Protocol::OpenAiChatCompletions
            .encode(&endpoint, &request)
            .unwrap();
        let chat: Value = serde_json::from_slice(&chat.body).unwrap();
        assert_eq!(chat["tools"][0]["function"]["parameters"]["type"], "object");

        let anthropic = Protocol::AnthropicMessages
            .encode(&endpoint, &request)
            .unwrap();
        let anthropic: Value = serde_json::from_slice(&anthropic.body).unwrap();
        assert_eq!(anthropic["tools"][0]["input_schema"]["type"], "object");
    }

    #[test]
    fn provider_protocols_project_empty_object_calls_back_to_unit() {
        let request = request_with_unit_tool();

        let responses = Protocol::OpenAiResponses
            .decode_for_request(
                &request,
                &response(
                    200,
                    &[],
                    serde_json::json!({
                        "output":[{
                            "type":"function_call",
                            "call_id":"call-unit",
                            "name":"phx1_fixture_dunit",
                            "arguments":"{}"
                        }]
                    }),
                ),
            )
            .unwrap();
        assert_eq!(responses.tool_calls[0].input, PhenixValue::Unit);

        let chat = Protocol::OpenAiChatCompletions
            .decode_for_request(
                &request,
                &response(
                    200,
                    &[],
                    serde_json::json!({
                        "choices":[{"message":{
                            "content":null,
                            "tool_calls":[{
                                "id":"call-unit",
                                "type":"function",
                                "function":{
                                    "name":"phx1_fixture_dunit",
                                    "arguments":"{}"
                                }
                            }]
                        }}]
                    }),
                ),
            )
            .unwrap();
        assert_eq!(chat.tool_calls[0].input, PhenixValue::Unit);

        let anthropic = Protocol::AnthropicMessages
            .decode_for_request(
                &request,
                &response(
                    200,
                    &[],
                    serde_json::json!({
                        "content":[{
                            "type":"tool_use",
                            "id":"call-unit",
                            "name":"fixture.unit",
                            "input":{}
                        }]
                    }),
                ),
            )
            .unwrap();
        assert_eq!(anthropic.tool_calls[0].input, PhenixValue::Unit);
    }

    #[test]
    fn request_aware_decode_restores_structural_table_inputs() {
        let request = request_with_tool();
        let decoded = Protocol::OpenAiResponses
            .decode_for_request(
                &request,
                &response(
                    200,
                    &[],
                    serde_json::json!({
                        "output":[{
                            "type":"function_call",
                            "call_id":"call-table",
                            "name":"phx1_fixture_decho",
                            "arguments":"{\"value\":\"typed\"}"
                        }]
                    }),
                ),
            )
            .unwrap();

        assert_eq!(
            decoded.tool_calls[0].input,
            PhenixValue::Table(BTreeMap::from([(
                Key::parse("value").unwrap(),
                PhenixValue::String("typed".to_owned())
            )]))
        );
    }

    #[test]
    fn scalar_model_tool_inputs_use_a_value_envelope() {
        let endpoint = Endpoint::parse("https://example.com/v1").unwrap();
        let mut request = request();
        request.tools.push(ModelToolDescriptor {
            id: CallableId::parse("fixture.scalar").unwrap(),
            description: "Scalar input".to_owned(),
            input_schema: PhenixSchema::U64,
            output_schema: PhenixSchema::Unit,
        });

        let encoded = Protocol::OpenAiResponses
            .encode(&endpoint, &request)
            .unwrap();
        let body: Value = serde_json::from_slice(&encoded.body).unwrap();
        assert_eq!(body["tools"][0]["parameters"]["type"], "object");
        assert_eq!(
            body["tools"][0]["parameters"]["properties"]["value"]["type"],
            "integer"
        );

        let decoded = Protocol::OpenAiResponses
            .decode_for_request(
                &request,
                &response(
                    200,
                    &[],
                    serde_json::json!({
                        "output":[{
                            "type":"function_call",
                            "call_id":"call-scalar",
                            "name":"phx1_fixture_dscalar",
                            "arguments":"{\"value\":7}"
                        }]
                    }),
                ),
            )
            .unwrap();
        assert_eq!(decoded.tool_calls[0].input, PhenixValue::U64(7));
    }

    #[test]
    fn provider_protocols_encode_the_same_model_tool_surface() {
        let endpoint = Endpoint::parse("https://example.com/v1").unwrap();

        let responses = Protocol::OpenAiResponses
            .encode(&endpoint, &request_with_tool())
            .unwrap();
        let responses: Value = serde_json::from_slice(&responses.body).unwrap();
        assert_eq!(responses["tools"][0]["type"], "function");
        assert_eq!(responses["tools"][0]["name"], "phx1_fixture_decho");
        assert_eq!(
            responses["tools"][0]["parameters"]["properties"]["value"]["type"],
            "string"
        );

        let chat = Protocol::OpenAiChatCompletions
            .encode(&endpoint, &request_with_tool())
            .unwrap();
        let chat: Value = serde_json::from_slice(&chat.body).unwrap();
        assert_eq!(chat["tools"][0]["type"], "function");
        assert_eq!(chat["tools"][0]["function"]["name"], "phx1_fixture_decho");

        let anthropic = Protocol::AnthropicMessages
            .encode(&endpoint, &request_with_tool())
            .unwrap();
        let anthropic: Value = serde_json::from_slice(&anthropic.body).unwrap();
        assert_eq!(anthropic["tools"][0]["name"], "fixture.echo");
        assert_eq!(anthropic["tools"][0]["input_schema"]["type"], "object");
    }

    #[test]
    fn provider_protocols_encode_variant_model_tool_inputs() {
        let endpoint = Endpoint::parse("https://example.com/v1").unwrap();
        let mut request = request();
        request.tools.push(ModelToolDescriptor {
            id: CallableId::parse("fixture.variant").unwrap(),
            description: "Exercise a structural variant".to_owned(),
            input_schema: PhenixSchema::Table(BTreeMap::from([(
                Key::parse("selection").unwrap(),
                PhenixSchema::Variant(BTreeMap::from([
                    (
                        Key::parse("named").unwrap(),
                        PhenixSchema::Table(BTreeMap::from([(
                            Key::parse("name").unwrap(),
                            PhenixSchema::String,
                        )])),
                    ),
                    (Key::parse("all").unwrap(), PhenixSchema::Unit),
                ])),
            )])),
            output_schema: PhenixSchema::Unit,
        });

        for protocol in [
            Protocol::OpenAiResponses,
            Protocol::OpenAiChatCompletions,
            Protocol::AnthropicMessages,
        ] {
            protocol.encode(&endpoint, &request).unwrap();
        }

        let schema = json_schema(&request.tools[0].input_schema).unwrap();
        let variants = schema["properties"]["selection"]["anyOf"]
            .as_array()
            .expect("variant lowers to provider JSON Schema alternatives");
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0]["properties"]["tag"]["enum"][0], "all");
        assert_eq!(variants[1]["properties"]["tag"]["enum"][0], "named");
        assert_eq!(
            variants[1]["properties"]["value"]["properties"]["name"]["type"],
            "string"
        );
    }

    #[test]
    fn openai_tool_names_round_trip_provider_safe_callable_ids() {
        for id in [
            "workspace.shell",
            "workspace/read",
            "provider:model@1",
            "already_safe",
            "contains-hyphen",
        ] {
            let id = CallableId::parse(id).unwrap();
            let wire = openai_tool_name(&id);
            assert!(
                wire.bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            );
            assert_eq!(parse_openai_callable_id(&wire).unwrap(), id);
        }
        assert_ne!(
            openai_tool_name(&CallableId::parse("workspace.shell").unwrap()),
            openai_tool_name(&CallableId::parse("workspace_shell").unwrap())
        );
    }

    #[test]
    fn provider_protocols_decode_structured_tool_calls() {
        let responses = Protocol::OpenAiResponses
            .decode(&response(
                200,
                &[],
                serde_json::json!({
                    "output":[{
                        "type":"function_call",
                        "call_id":"call-responses",
                        "name":"phx1_fixture_decho",
                        "arguments":"{\"value\":\"responses\"}"
                    }]
                }),
            ))
            .unwrap();
        assert_eq!(responses.output.as_ref(), b"");
        assert_eq!(responses.tool_calls[0].call_id, "call-responses");
        assert_eq!(responses.tool_calls[0].callable_id.as_str(), "fixture.echo");
        assert_eq!(
            responses.tool_calls[0].input,
            PhenixValue::Map(BTreeMap::from([(
                "value".to_owned(),
                PhenixValue::String("responses".to_owned())
            )]))
        );

        let chat = Protocol::OpenAiChatCompletions
            .decode(&response(
                200,
                &[],
                serde_json::json!({
                    "choices":[{"message":{
                        "content":null,
                        "tool_calls":[{
                            "id":"call-chat",
                            "type":"function",
                            "function":{
                                "name":"phx1_fixture_decho",
                                "arguments":"{\"value\":\"chat\"}"
                            }
                        }]
                    }}]
                }),
            ))
            .unwrap();
        assert_eq!(chat.output.as_ref(), b"");
        assert_eq!(chat.tool_calls[0].call_id, "call-chat");
        assert_eq!(chat.tool_calls[0].callable_id.as_str(), "fixture.echo");

        let anthropic = Protocol::AnthropicMessages
            .decode(&response(
                200,
                &[],
                serde_json::json!({
                    "content":[{
                        "type":"tool_use",
                        "id":"call-anthropic",
                        "name":"fixture.echo",
                        "input":{"value":"anthropic"}
                    }]
                }),
            ))
            .unwrap();
        assert_eq!(anthropic.output.as_ref(), b"");
        assert_eq!(anthropic.tool_calls[0].call_id, "call-anthropic");
        assert_eq!(anthropic.tool_calls[0].callable_id.as_str(), "fixture.echo");
    }

    #[test]
    fn model_tool_results_project_structural_values_before_provider_encoding() {
        let result = ModelToolResult {
            call_id: "call-1".to_owned(),
            callable_id: CallableId::parse("fixture.echo").unwrap(),
            output: PhenixValue::Variant {
                tag: Key::parse("process").unwrap(),
                value: Box::new(PhenixValue::Table(BTreeMap::from([
                    (Key::parse("exit_code").unwrap(), PhenixValue::I64(0)),
                    (
                        Key::parse("stdout").unwrap(),
                        PhenixValue::String("ok".to_owned()),
                    ),
                    (
                        Key::parse("payload").unwrap(),
                        PhenixValue::Option(Some(Box::new(PhenixValue::Bytes(vec![1, 2, 3])))),
                    ),
                ]))),
            },
            is_error: false,
        };

        let encoded: Value = serde_json::from_str(&tool_output(&result).unwrap()).unwrap();
        assert_eq!(
            encoded,
            serde_json::json!({
                "tag": "process",
                "value": {
                    "exit_code": 0,
                    "stdout": "ok",
                    "payload": "AQID"
                }
            })
        );
    }

    #[test]
    fn model_tool_errors_use_the_same_structural_projection() {
        let result = ModelToolResult {
            call_id: "call-1".to_owned(),
            callable_id: CallableId::parse("fixture.echo").unwrap(),
            output: PhenixValue::Variant {
                tag: Key::parse("invalid_input").unwrap(),
                value: Box::new(PhenixValue::Table(BTreeMap::from([(
                    Key::parse("message").unwrap(),
                    PhenixValue::String("missing command".to_owned()),
                )]))),
            },
            is_error: true,
        };

        let encoded: Value = serde_json::from_str(&tool_output(&result).unwrap()).unwrap();
        assert_eq!(
            encoded,
            serde_json::json!({
                "error": {
                    "tag": "invalid_input",
                    "value": {
                        "message": "missing command"
                    }
                }
            })
        );
    }

    #[test]
    fn non_json_options_stop_at_protocol_adapter() {
        let endpoint = Endpoint::parse("https://example.com/v1").unwrap();
        let mut request = request();
        request
            .options
            .insert("binary".into(), PhenixValue::Bytes(vec![1, 2, 3]));

        let error = Protocol::OpenAiResponses
            .encode(&endpoint, &request)
            .unwrap_err();

        assert!(matches!(
            error,
            ProviderError::InvalidRequest { message }
                if message.contains("not JSON-compatible")
        ));
    }

    #[test]
    fn anthropic_messages_maps_internal_request_and_response() {
        let endpoint = Endpoint::parse("https://example.com/v1").unwrap();
        let encoded = Protocol::AnthropicMessages
            .encode(&endpoint, &request())
            .unwrap();
        assert_eq!(encoded.url, "https://example.com/v1/messages");
        let body: Value = serde_json::from_slice(&encoded.body).unwrap();
        assert_eq!(body["max_tokens"], 4096);

        let decoded = Protocol::AnthropicMessages
            .decode(&response(
                200,
                &[],
                serde_json::json!({
                    "content":[{"type":"text","text":"world"}]
                }),
            ))
            .unwrap();
        assert_eq!(decoded.output.as_ref(), b"world");
    }

    #[test]
    fn common_http_failures_are_normalized() {
        let error = normalize_http_error(&response(
            429,
            &[("retry-after", "3")],
            serde_json::json!({"error":{"message":"rate limit"}}),
        ));
        let ProviderError::RateLimited { limits, .. } = error else {
            panic!("expected rate-limited error");
        };
        assert_eq!(limits.retry_after, Some(DurationMs(3000)));

        let error = normalize_http_error(&response(
            400,
            &[],
            serde_json::json!({
                "error":{
                    "code":"context_length_exceeded",
                    "message":"too many tokens"
                }
            }),
        ));
        assert!(matches!(error, ProviderError::ContextLimit { .. }));
    }
}
