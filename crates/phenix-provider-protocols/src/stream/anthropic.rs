//! Anthropic Messages event stream validation; framing, bounds and Model
//! validation are shared with the other codecs.
use super::*;

#[derive(Default)]
pub(super) struct State {
    id: Option<String>,
    started: bool,
    stopped: bool,
    stop_reason: Option<String>,
    output_tokens: Option<u64>,
    blocks: BTreeMap<u64, Block>,
    next_tool: u64,
}

struct Block {
    text: bool,
    tool_id: Option<String>,
    tool_name: Option<String>,
    tool_index: Option<u64>,
    json: String,
    closed: bool,
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, BasicError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("missing Anthropic string"))
}

impl Stream {
    pub(super) fn anthropic_frame(&mut self, data: &str) -> Result<String, BasicError> {
        if self.done {
            return Err(invalid("data after Anthropic message_stop"));
        }
        let frame: Value =
            serde_json::from_str(data).map_err(|_| invalid("malformed Anthropic JSON"))?;
        let kind = string(&frame, "type")?;
        if kind == "ping" {
            return Ok(String::new());
        }
        if kind == "error" {
            return Err(failed("HTTP model reported an error"));
        }
        match kind {
            "message_start" => self.anthropic_start(&frame),
            "content_block_start" => self.anthropic_block_start(&frame),
            "content_block_delta" => self.anthropic_block_delta(&frame),
            "content_block_stop" => self.anthropic_block_stop(&frame),
            "message_delta" => self.anthropic_message_delta(&frame),
            "message_stop" => self.anthropic_message_stop(),
            _ => Err(invalid("unknown Anthropic event")),
        }
    }

    fn anthropic_require_started(&self) -> Result<(), BasicError> {
        if !self.anthropic.started {
            return Err(invalid("Anthropic event before message_start"));
        }
        Ok(())
    }

    fn anthropic_start(&mut self, frame: &Value) -> Result<String, BasicError> {
        if self.anthropic.started {
            return Err(invalid("duplicate Anthropic message_start"));
        }
        let message = &frame["message"];
        if message["type"] != "message" || message["role"] != "assistant" {
            return Err(invalid("invalid Anthropic message"));
        }
        let id = string(message, "id")?;
        if id.is_empty() {
            return Err(invalid("invalid Anthropic message id"));
        }
        if message["content"] != serde_json::json!([]) || !message["stop_reason"].is_null() {
            return Err(invalid("invalid Anthropic message start"));
        }
        self.account(id.len())?;
        let usage = &message["usage"];
        let input_tokens = usage["input_tokens"]
            .as_u64()
            .ok_or_else(|| invalid("invalid Anthropic input usage"))?
            .saturating_add(usage["cache_creation_input_tokens"].as_u64().unwrap_or(0))
            .saturating_add(usage["cache_read_input_tokens"].as_u64().unwrap_or(0));
        self.usage = Some(Usage {
            input_tokens,
            output_tokens: 0,
        });
        self.anthropic.id = Some(id.to_owned());
        self.anthropic.started = true;
        Ok(String::new())
    }

    fn anthropic_block_start(&mut self, frame: &Value) -> Result<String, BasicError> {
        self.anthropic_require_started()?;
        let index = frame["index"]
            .as_u64()
            .ok_or_else(|| invalid("missing Anthropic block index"))?;
        if index >= phenix_sdk::MAX_COUNT as u64 || self.anthropic.blocks.contains_key(&index) {
            return Err(invalid("invalid Anthropic block index"));
        }
        let block = &frame["content_block"];
        let block = match string(block, "type")? {
            "text" => {
                if frame["content_block"]["text"] != "" {
                    return Err(invalid("invalid Anthropic text block"));
                }
                Block {
                    text: true,
                    tool_id: None,
                    tool_name: None,
                    tool_index: None,
                    json: String::new(),
                    closed: false,
                }
            }
            "tool_use" => {
                let id = string(&frame["content_block"], "id")?;
                let name = string(&frame["content_block"], "name")?;
                if id.is_empty()
                    || name.is_empty()
                    || frame["content_block"]["input"] != serde_json::json!({})
                {
                    return Err(invalid("invalid Anthropic tool_use block"));
                }
                self.account(id.len() + name.len())?;
                let tool_index = self.anthropic.next_tool;
                self.anthropic.next_tool += 1;
                Block {
                    text: false,
                    tool_id: Some(id.to_owned()),
                    tool_name: Some(name.to_owned()),
                    tool_index: Some(tool_index),
                    json: String::new(),
                    closed: false,
                }
            }
            _ => return Err(failed("unsupported Anthropic content block")),
        };
        self.anthropic.blocks.insert(index, block);
        Ok(String::new())
    }

    fn anthropic_block_delta(&mut self, frame: &Value) -> Result<String, BasicError> {
        self.anthropic_require_started()?;
        let index = frame["index"]
            .as_u64()
            .ok_or_else(|| invalid("missing Anthropic block index"))?;
        let is_text = self
            .anthropic
            .blocks
            .get(&index)
            .filter(|block| !block.closed)
            .ok_or_else(|| invalid("unknown or closed Anthropic block"))?
            .text;
        let delta = &frame["delta"];
        match string(delta, "type")? {
            "text_delta" => {
                if !is_text {
                    return Err(invalid("text delta on non-text Anthropic block"));
                }
                let text = string(delta, "text")?;
                self.account(text.len())?;
                self.text.push_str(text);
                Ok(text.to_string())
            }
            "input_json_delta" => {
                if is_text {
                    return Err(invalid("json delta on text Anthropic block"));
                }
                let partial = string(delta, "partial_json")?;
                self.account(partial.len())?;
                self.anthropic
                    .blocks
                    .get_mut(&index)
                    .ok_or_else(|| invalid("missing Anthropic block"))?
                    .json
                    .push_str(partial);
                Ok(String::new())
            }
            _ => Err(failed("unsupported Anthropic delta")),
        }
    }

    fn anthropic_block_stop(&mut self, frame: &Value) -> Result<String, BasicError> {
        self.anthropic_require_started()?;
        let index = frame["index"]
            .as_u64()
            .ok_or_else(|| invalid("missing Anthropic block index"))?;
        let finished = {
            let block = self
                .anthropic
                .blocks
                .get_mut(&index)
                .filter(|block| !block.closed)
                .ok_or_else(|| invalid("unknown or closed Anthropic block"))?;
            block.closed = true;
            (
                block.text,
                block.tool_id.take(),
                block.tool_name.take(),
                block.tool_index.take(),
                std::mem::take(&mut block.json),
            )
        };
        if finished.0 {
            return Ok(String::new());
        }
        let input: Value = serde_json::from_str(&finished.4)
            .map_err(|_| invalid("invalid Anthropic tool input"))?;
        let id = finished
            .1
            .ok_or_else(|| invalid("missing Anthropic tool id"))?;
        let name = finished
            .2
            .ok_or_else(|| invalid("missing Anthropic tool name"))?;
        let tool_index = finished
            .3
            .ok_or_else(|| invalid("missing Anthropic tool index"))?;
        self.calls.insert(
            tool_index,
            ToolCall {
                id,
                tool: name,
                arguments: input.to_string(),
            },
        );
        Ok(String::new())
    }

    fn anthropic_message_delta(&mut self, frame: &Value) -> Result<String, BasicError> {
        self.anthropic_require_started()?;
        if self.anthropic.stop_reason.is_some() {
            return Err(invalid("duplicate Anthropic message_delta"));
        }
        let reason = frame["delta"]["stop_reason"]
            .as_str()
            .ok_or_else(|| invalid("missing Anthropic stop reason"))?;
        let output_tokens = frame["usage"]["output_tokens"]
            .as_u64()
            .ok_or_else(|| invalid("invalid Anthropic output usage"))?;
        self.anthropic.stop_reason = Some(reason.to_owned());
        self.anthropic.output_tokens = Some(output_tokens);
        Ok(String::new())
    }

    fn anthropic_message_stop(&mut self) -> Result<String, BasicError> {
        self.anthropic_require_started()?;
        if self.anthropic.stopped {
            return Err(invalid("duplicate Anthropic message_stop"));
        }
        if self.anthropic.blocks.values().any(|block| !block.closed) {
            return Err(invalid("Anthropic message_stop with open content block"));
        }
        let reason = self
            .anthropic
            .stop_reason
            .take()
            .ok_or_else(|| invalid("Anthropic message_stop before message_delta"))?;
        self.finish = Some(match reason.as_str() {
            "end_turn" | "stop_sequence" => "stop".into(),
            "tool_use" => "tool_calls".into(),
            "max_tokens" => {
                return Err(error(
                    ErrorCode::TurnLimit,
                    "model completion limit reached",
                ))
            }
            _ => return Err(failed("model completion was not successful")),
        });
        let output_tokens = self
            .anthropic
            .output_tokens
            .take()
            .ok_or_else(|| invalid("missing Anthropic output usage"))?;
        self.usage
            .as_mut()
            .ok_or_else(|| invalid("missing Anthropic usage"))?
            .output_tokens = output_tokens;
        self.anthropic.stopped = true;
        self.done = true;
        Ok(String::new())
    }
}

#[cfg(test)]
#[path = "anthropic_tests.rs"]
mod tests;
