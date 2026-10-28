//! Responses lifecycle validation; framing, bounds and Model validation are shared.
use super::*;

#[derive(Default)]
pub(super) struct State {
    id: Option<String>,
    items: Vec<Item>,
}

struct Item {
    value: Value,
    closed: bool,
    text_done: bool,
    part_done: bool,
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, BasicError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("missing Responses string"))
}

impl Stream {
    pub(super) fn responses_frame(&mut self, data: &str) -> Result<String, BasicError> {
        if self.done {
            return Err(invalid("data after Responses completion"));
        }
        let frame: Value =
            serde_json::from_str(data).map_err(|_| invalid("malformed Responses JSON"))?;
        let kind = string(&frame, "type")?;
        if matches!(kind, "error" | "response.failed") {
            // Subscription-plan errors may arrive *inside* an SSE stream,
            // after an HTTP 200. Keep only explicitly documented machine
            // codes; raw provider text must not reach graph or ACP errors.
            let error = if kind == "response.failed" {
                &frame["response"]["error"]
            } else {
                &frame["error"]
            };
            let code = error["code"].as_str();
            let documented = matches!(
                code,
                Some(
                    "subscription_sharing_user_not_eligible"
                        | "subscription_sharing_usage_limit_exceeded"
                        | "subscription_sharing_usage_unavailable"
                        | "subscription_sharing_unsupported_capability"
                        | "subscription_sharing_route_not_supported"
                        | "subscription_sharing_invalid_user"
                        | "chatpass_v2_scope_not_authorized"
                        | "chatpass_v2_invalid_authorization_context"
                        | "subscription_sharing_user_unavailable"
                )
            );
            let message = if documented {
                code.unwrap_or("HTTP model reported an error")
            } else {
                "HTTP model reported an error"
            };
            return Err(failed(message));
        }
        if kind == "response.incomplete" {
            return Err(
                if frame["response"]["incomplete_details"]["reason"] == "max_output_tokens" {
                    error(ErrorCode::TurnLimit, "model completion limit reached")
                } else {
                    failed("model completion was not successful")
                },
            );
        }
        if kind == "response.created" {
            let response = &frame["response"];
            let id = string(response, "id")?;
            if self.responses.id.is_some() || id.is_empty() || response["status"] != "in_progress" {
                return Err(invalid("invalid Responses creation"));
            }
            self.account(id.len())?;
            self.responses.id = Some(id.to_owned());
            return Ok(String::new());
        }
        let Some(id) = &self.responses.id else {
            return Err(invalid("Responses event before creation"));
        };
        if kind == "response.in_progress" {
            if frame["response"]["id"] != *id || frame["response"]["status"] != "in_progress" {
                return Err(invalid("invalid Responses identity"));
            }
            return Ok(String::new());
        }
        if kind == "response.completed" {
            let response = &frame["response"];
            let output = response["output"]
                .as_array()
                .ok_or_else(|| invalid("missing Responses output"))?;
            if response["id"] != *id
                || response["status"] != "completed"
                || output.len() != self.responses.items.len()
                || self.responses.items.is_empty()
                || self
                    .responses
                    .items
                    .iter()
                    .zip(output)
                    .any(|(item, final_item)| !item.closed || item.value != *final_item)
            {
                return Err(invalid(
                    "Responses completion disagrees with streamed output",
                ));
            }
            let usage = &response["usage"];
            self.usage = Some(Usage {
                input_tokens: usage["input_tokens"]
                    .as_u64()
                    .ok_or_else(|| invalid("invalid input usage"))?,
                output_tokens: usage["output_tokens"]
                    .as_u64()
                    .ok_or_else(|| invalid("invalid output usage"))?,
            });
            for item in &self.responses.items {
                if item.value["type"] == "function_call" {
                    self.calls.insert(
                        self.calls.len() as u64,
                        ToolCall {
                            id: string(&item.value, "call_id")?.into(),
                            tool: string(&item.value, "name")?.into(),
                            arguments: string(&item.value, "arguments")?.into(),
                        },
                    );
                }
            }
            self.finish = Some(
                if self.calls.is_empty() {
                    "stop"
                } else {
                    "tool_calls"
                }
                .into(),
            );
            self.done = true;
            return Ok(String::new());
        }
        let index = frame["output_index"]
            .as_u64()
            .ok_or_else(|| invalid("missing Responses output index"))?;
        if index >= phenix_sdk::MAX_COUNT as u64 {
            return Err(invalid("Responses output index exceeds bounds"));
        }
        let index = index as usize;
        if kind == "response.output_item.added" {
            let value = frame["item"].clone();
            let item_id = string(&value, "id")?;
            if index != self.responses.items.len()
                || item_id.is_empty()
                || self
                    .responses
                    .items
                    .iter()
                    .any(|item| item.value["id"] == item_id)
                || value["status"] != "in_progress"
            {
                return Err(invalid("invalid Responses output item ordering"));
            }
            match string(&value, "type")? {
                "message"
                    if value["role"] == "assistant"
                        && value["content"] == serde_json::json!([]) => {}
                "function_call"
                    if !string(&value, "call_id")?.is_empty()
                        && !string(&value, "name")?.is_empty()
                        && value["arguments"] == "" => {}
                _ => return Err(invalid("unsupported Responses output item")),
            }
            self.account(value.to_string().len())?;
            self.responses.items.push(Item {
                value,
                closed: false,
                text_done: false,
                part_done: false,
            });
            return Ok(String::new());
        }
        // Account deltas before borrowing their destination. Final snapshots are
        // compared, not appended, so they cannot duplicate streamed text/calls.
        let delta = if kind.ends_with(".delta") {
            string(&frame, "delta")?
        } else {
            ""
        };
        self.account(delta.len())?;
        let item = self
            .responses
            .items
            .get_mut(index)
            .filter(|item| !item.closed)
            .ok_or_else(|| invalid("unknown or closed Responses output item"))?;
        if kind == "response.output_item.done" {
            let mut expected = item.value.clone();
            expected["status"] = serde_json::json!("completed");
            if frame["item"] != expected
                || !item.text_done
                || (item.value["type"] == "message" && !item.part_done)
            {
                return Err(invalid("Responses item completion disagrees with deltas"));
            }
            item.value = expected;
            item.closed = true;
            return Ok(String::new());
        }
        if frame["item_id"] != item.value["id"] {
            return Err(invalid("Responses item identity mismatch"));
        }
        if item.value["type"] == "function_call" {
            match kind {
                "response.function_call_arguments.delta" if !item.text_done => {
                    item.value["arguments"]
                        .as_str()
                        .ok_or_else(|| invalid("invalid tool arguments"))?;
                    let mut arguments = string(&item.value, "arguments")?.to_owned();
                    arguments.push_str(delta);
                    item.value["arguments"] = Value::String(arguments);
                }
                "response.function_call_arguments.done"
                    if !item.text_done && frame["arguments"] == item.value["arguments"] =>
                {
                    item.text_done = true
                }
                _ => return Err(invalid("invalid Responses function lifecycle")),
            }
        } else {
            if frame["content_index"].as_u64() != Some(0) {
                return Err(invalid("unsupported Responses content index"));
            }
            match kind {
                "response.content_part.added"
                    if item.value["content"] == serde_json::json!([])
                        && frame["part"]["type"] == "output_text"
                        && frame["part"]["text"] == "" =>
                {
                    item.value["content"] = serde_json::json!([frame["part"]]);
                }
                "response.output_text.delta"
                    if !item.text_done && item.value["content"][0]["type"] == "output_text" =>
                {
                    let mut text = string(&item.value["content"][0], "text")?.to_owned();
                    text.push_str(delta);
                    item.value["content"][0]["text"] = Value::String(text);
                    self.text.push_str(delta);
                    return Ok(delta.into());
                }
                "response.output_text.done"
                    if !item.text_done && frame["text"] == item.value["content"][0]["text"] =>
                {
                    item.text_done = true
                }
                "response.content_part.done"
                    if item.text_done
                        && !item.part_done
                        && frame["part"] == item.value["content"][0] =>
                {
                    item.part_done = true
                }
                _ => return Err(invalid("unsupported Responses message lifecycle")),
            }
        }
        Ok(String::new())
    }
}

#[cfg(test)]
#[path = "responses_tests.rs"]
mod tests;
