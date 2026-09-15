use crate::{Endpoint, ProviderError, ProviderRequest, ProviderResponse, RateLimits};
use phenix_core::{
    CallableId, ModelInferenceRequest, ModelInferenceResponse, ModelToolCall, ModelToolDescriptor,
    PhenixSchema, ValueCodec,
};
use reqwest::header::CONTENT_TYPE;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub trait ProtocolAdapter: Send + Sync {
    fn name(&self) -> &'static str;

    fn encode(
        &self,
        endpoint: &Endpoint,
        request: &ModelInferenceRequest,
    ) -> Result<ProviderRequest, ProviderError>;

    fn decode(&self, response: &ProviderResponse) -> Result<ModelInferenceResponse, ProviderError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    OpenAiResponses,
    OpenAiChatCompletions,
    AnthropicMessages,
}

impl ProtocolAdapter for Protocol {
    fn name(&self) -> &'static str {
        match self {
            Self::OpenAiResponses => "openai_responses",
            Self::OpenAiChatCompletions => "openai_chat_completions",
            Self::AnthropicMessages => "anthropic_messages",
        }
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
        }
    }

    fn decode(&self, response: &ProviderResponse) -> Result<ModelInferenceResponse, ProviderError> {
        match self {
            Self::OpenAiResponses => openai_responses_response(response),
            Self::OpenAiChatCompletions => openai_chat_response(response),
            Self::AnthropicMessages => anthropic_response(response),
        }
    }
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
        PhenixSchema::Variant(_) | PhenixSchema::Callable { .. } | PhenixSchema::Object { .. } => {
            return Err(ProviderError::InvalidRequest {
                message: "model tool input schema cannot be represented as provider JSON Schema"
                    .to_owned(),
            });
        }
    };
    Ok(schema)
}

fn openai_tool(tool: &ModelToolDescriptor) -> Result<Value, ProviderError> {
    Ok(serde_json::json!({
        "type": "function",
        "name": tool.id.as_str(),
        "description": tool.description,
        "parameters": json_schema(&tool.input_schema)?,
    }))
}

fn openai_chat_tool(tool: &ModelToolDescriptor) -> Result<Value, ProviderError> {
    Ok(serde_json::json!({
        "type": "function",
        "function": {
            "name": tool.id.as_str(),
            "description": tool.description,
            "parameters": json_schema(&tool.input_schema)?,
        },
    }))
}

fn anthropic_tool(tool: &ModelToolDescriptor) -> Result<Value, ProviderError> {
    Ok(serde_json::json!({
        "name": tool.id.as_str(),
        "description": tool.description,
        "input_schema": json_schema(&tool.input_schema)?,
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

fn openai_responses_request(
    endpoint: &Endpoint,
    request: &ModelInferenceRequest,
) -> Result<ProviderRequest, ProviderError> {
    let (mut body, text) = request_object(request, &["model", "input", "tools"])?;
    body.insert(
        "model".to_owned(),
        Value::String(request.model.as_str().to_owned()),
    );
    body.insert("input".to_owned(), Value::String(text));
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
    body.insert(
        "model".to_owned(),
        Value::String(request.model.as_str().to_owned()),
    );
    body.insert(
        "messages".to_owned(),
        serde_json::json!([{"role":"user","content":text}]),
    );
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
    body.insert(
        "messages".to_owned(),
        serde_json::json!([{"role":"user","content":text}]),
    );
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

fn response_with_content(
    value: &Value,
    text: String,
    tool_calls: Vec<ModelToolCall>,
) -> ModelInferenceResponse {
    let mut provider_metadata = BTreeMap::new();
    if let Some(id) = value.get("id").cloned() {
        provider_metadata.insert("id".to_owned(), id.into());
    }
    if let Some(usage) = value.get("usage").cloned() {
        provider_metadata.insert("usage".to_owned(), usage.into());
    }
    ModelInferenceResponse {
        output: text.into_bytes().into(),
        provider_metadata,
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
                callable_id: parse_callable_id(name)?,
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
    Ok(response_with_content(&value, text, tool_calls))
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
                callable_id: parse_callable_id(name)?,
                input: parse_arguments(arguments, "OpenAI chat")?,
            })
        })
        .collect::<Result<Vec<_>, ProviderError>>()?;
    if text.is_empty() && tool_calls.is_empty() {
        return Err(ProviderError::Protocol {
            message: "OpenAI chat payload contained neither content nor tool calls".to_owned(),
        });
    }
    Ok(response_with_content(&value, text, tool_calls))
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
    Ok(response_with_content(&value, text, tool_calls))
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
        .or_else(|| value.pointer("/error/code").and_then(Value::as_str))
        .or_else(|| value.get("error").and_then(Value::as_str))
        .unwrap_or("provider request failed")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DurationMs, ProviderResponse};
    use phenix_core::{Key, PhenixValue};
    use std::collections::BTreeMap;

    fn request() -> ModelInferenceRequest {
        ModelInferenceRequest {
            model: phenix_core::ModelId::parse("test-model").unwrap(),
            input: b"hello".to_vec().into(),
            options: BTreeMap::new(),
            tools: Vec::new(),
        }
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
        assert_eq!(
            decoded.provider_metadata["id"],
            PhenixValue::String("response-1".into())
        );
    }

    #[test]
    fn provider_protocols_encode_the_same_model_tool_surface() {
        let endpoint = Endpoint::parse("https://example.com/v1").unwrap();

        let responses = Protocol::OpenAiResponses
            .encode(&endpoint, &request_with_tool())
            .unwrap();
        let responses: Value = serde_json::from_slice(&responses.body).unwrap();
        assert_eq!(responses["tools"][0]["type"], "function");
        assert_eq!(responses["tools"][0]["name"], "fixture.echo");
        assert_eq!(
            responses["tools"][0]["parameters"]["properties"]["value"]["type"],
            "string"
        );

        let chat = Protocol::OpenAiChatCompletions
            .encode(&endpoint, &request_with_tool())
            .unwrap();
        let chat: Value = serde_json::from_slice(&chat.body).unwrap();
        assert_eq!(chat["tools"][0]["type"], "function");
        assert_eq!(chat["tools"][0]["function"]["name"], "fixture.echo");

        let anthropic = Protocol::AnthropicMessages
            .encode(&endpoint, &request_with_tool())
            .unwrap();
        let anthropic: Value = serde_json::from_slice(&anthropic.body).unwrap();
        assert_eq!(anthropic["tools"][0]["name"], "fixture.echo");
        assert_eq!(anthropic["tools"][0]["input_schema"]["type"], "object");
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
                        "name":"fixture.echo",
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
                                "name":"fixture.echo",
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
