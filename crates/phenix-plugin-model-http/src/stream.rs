use crate::{failed, invalid};
use phenix_basic_contracts::{
    error, BasicError, ErrorCode, ModelReply, ModelRequest, ToolCall, Usage,
};
use phenix_sdk::Portable;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct Stream {
    parser: sse_parse::SseParser,
    incomplete_utf8: Vec<u8>,
    text: String,
    calls: BTreeMap<u64, ToolCall>,
    usage: Option<Usage>,
    finish: Option<String>,
    done: bool,
    reply_bytes: usize,
}
impl Default for Stream {
    fn default() -> Self {
        Self {
            parser: sse_parse::SseParser::new().max_buffer(phenix_sdk::MAX_BYTES),
            incomplete_utf8: Vec::new(),
            text: String::new(),
            calls: BTreeMap::new(),
            usage: None,
            finish: None,
            done: false,
            reply_bytes: 0,
        }
    }
}
impl Stream {
    pub fn feed(&mut self, bytes: &[u8]) -> Result<String, BasicError> {
        // The SSE standard replaces invalid UTF-8. The Model wire contract
        // rejects it instead, including invalid sequences split across chunks.
        let mut candidate = std::mem::take(&mut self.incomplete_utf8);
        candidate.extend_from_slice(bytes);
        if let Err(problem) = std::str::from_utf8(&candidate) {
            if problem.error_len().is_some() {
                return Err(invalid("invalid SSE UTF-8"));
            }
            self.incomplete_utf8
                .extend_from_slice(&candidate[problem.valid_up_to()..]);
        }
        self.parser.feed(bytes);
        if self.parser.overflows() != 0 {
            return Err(invalid("SSE frame exceeds portable bounds"));
        }
        let mut progress = String::new();
        while let Some(event) = self.parser.next_event() {
            progress.push_str(&self.frame(&event.data)?);
        }
        Ok(progress)
    }
    fn account(&mut self, length: usize) -> Result<(), BasicError> {
        self.reply_bytes = self.reply_bytes.saturating_add(length);
        if self.reply_bytes > phenix_sdk::MAX_BYTES {
            return Err(invalid("reply exceeds portable bounds"));
        }
        Ok(())
    }
    fn frame(&mut self, data: &str) -> Result<String, BasicError> {
        if self.done {
            return Err(invalid("data after terminal SSE marker"));
        }
        if data == "[DONE]" {
            self.done = true;
            return Ok(String::new());
        }
        let frame: Value = serde_json::from_str(data).map_err(|_| invalid("malformed SSE JSON"))?;
        if frame.get("error").is_some() {
            return Err(failed("HTTP model reported an error"));
        }
        if let Some(usage) = frame.get("usage").filter(|u| !u.is_null()) {
            if self.usage.is_some() {
                return Err(invalid("duplicate model usage"));
            }
            self.usage = Some(Usage {
                input_tokens: usage
                    .get("prompt_tokens")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| invalid("invalid input usage"))?,
                output_tokens: usage
                    .get("completion_tokens")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| invalid("invalid output usage"))?,
            });
        }
        let choices = frame
            .get("choices")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("missing model choices"))?;
        if choices.is_empty() {
            if self.usage.is_none() || self.finish.is_none() {
                return Err(invalid("unexpected empty model choice"));
            }
            return Ok(String::new());
        }
        if choices.len() != 1
            || choices[0].get("index").and_then(Value::as_u64) != Some(0)
            || self.finish.is_some()
        {
            return Err(invalid("invalid model choice ordering"));
        }
        let choice = &choices[0];
        let delta = choice
            .get("delta")
            .and_then(Value::as_object)
            .ok_or_else(|| invalid("missing model delta"))?;
        if ["function_call", "audio"]
            .iter()
            .any(|key| delta.get(*key).is_some_and(|v| !v.is_null()))
        {
            return Err(invalid("unsupported model delta content"));
        }
        if delta
            .get("role")
            .is_some_and(|r| r.as_str() != Some("assistant"))
            || delta.get("refusal").is_some_and(|r| !r.is_null())
        {
            return Err(failed("unsupported model response"));
        }
        let text = match delta.get("content") {
            None | Some(Value::Null) => "",
            Some(Value::String(text)) => text,
            _ => return Err(invalid("invalid model text delta")),
        }
        .to_string();
        self.account(text.len())?;
        self.text.push_str(&text);
        if let Some(calls) = delta.get("tool_calls") {
            let calls = calls
                .as_array()
                .ok_or_else(|| invalid("invalid tool call delta"))?;
            for call in calls {
                let index = call
                    .get("index")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| invalid("missing tool call index"))?;
                if index >= phenix_sdk::MAX_COUNT as u64
                    || call
                        .get("type")
                        .is_some_and(|t| t.as_str() != Some("function"))
                {
                    return Err(invalid("unsupported tool call"));
                }
                let id = optional_text(call.get("id"))?;
                let function = call.get("function");
                if function.is_some_and(|f| !f.is_object()) {
                    return Err(invalid("invalid function delta"));
                }
                let name = optional_text(function.and_then(|f| f.get("name")))?;
                let arguments = optional_text(function.and_then(|f| f.get("arguments")))?;
                self.account(id.len() + name.len() + arguments.len())?;
                let entry = self.calls.entry(index).or_insert(ToolCall {
                    id: String::new(),
                    tool: String::new(),
                    arguments: String::new(),
                });
                entry.id.push_str(id);
                entry.tool.push_str(name);
                entry.arguments.push_str(arguments);
            }
        }
        if let Some(finish) = choice.get("finish_reason").filter(|f| !f.is_null()) {
            let finish = finish
                .as_str()
                .ok_or_else(|| invalid("invalid finish reason"))?;
            match finish {
                "stop" | "tool_calls" => self.finish = Some(finish.into()),
                "length" => {
                    return Err(error(
                        ErrorCode::TurnLimit,
                        "model completion limit reached",
                    ))
                }
                _ => return Err(failed("model completion was not successful")),
            }
        }
        Ok(text)
    }
    pub fn finish(mut self, request: &ModelRequest) -> Result<ModelReply, BasicError> {
        if !self.incomplete_utf8.is_empty() {
            return Err(invalid("invalid SSE UTF-8"));
        }
        self.parser.finish();
        if !self.done || self.finish.is_none() || self.parser.next_event().is_some() {
            return Err(invalid("incomplete model stream"));
        }
        let usage = self
            .usage
            .take()
            .ok_or_else(|| invalid("missing model usage"))?;
        if request
            .limits
            .model_tokens
            .is_some_and(|n| usage.output_tokens > n)
        {
            return Err(error(
                ErrorCode::TurnLimit,
                "model exceeded completion limit",
            ));
        }
        let mut ids = BTreeSet::new();
        let names: BTreeSet<_> = request.tools.iter().map(|t| t.id.as_str()).collect();
        let mut calls = vec![];
        for (expected, (index, mut call)) in self.calls.into_iter().enumerate() {
            if index != expected as u64
                || call.id.is_empty()
                || !ids.insert(call.id.clone())
                || !names.contains(call.tool.as_str())
            {
                return Err(invalid("invalid or unselected completed tool call"));
            }
            let arguments: Value = serde_json::from_str(&call.arguments)
                .map_err(|_| invalid("invalid tool arguments JSON"))?;
            let envelope = arguments
                .as_object()
                .filter(|o| o.len() == 1 && o.contains_key("value"))
                .ok_or_else(|| invalid("missing tool argument envelope"))?;
            call.arguments = envelope["value"].to_string();
            calls.push(call);
        }
        if (self.finish.as_deref() == Some("tool_calls")) == calls.is_empty() {
            return Err(invalid("finish reason disagrees with tool calls"));
        }
        let reply = ModelReply {
            text: self.text,
            calls,
            usage,
        };
        reply
            .to_value()
            .encode()
            .map_err(|_| invalid("reply exceeds portable bounds"))?;
        Ok(reply)
    }
}
fn optional_text(value: Option<&Value>) -> Result<&str, BasicError> {
    match value {
        None | Some(Value::Null) => Ok(""),
        Some(Value::String(s)) => Ok(s),
        _ => Err(invalid("invalid string delta")),
    }
}
