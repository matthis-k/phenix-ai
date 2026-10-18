"""Responses wire producer for the existing native and Basic HTTP scenarios."""
import json


def chat_view(body):
    assert body["store"] is False and body["stream"] is True
    assert "messages" not in body and "previous_response_id" not in body
    messages = []
    for item in body["input"]:
        if item.get("type") == "function_call":
            assert item["call_id"] and item["name"]
            if not messages or messages[-1]["role"] != "assistant":
                messages.append({"role": "assistant", "content": ""})
            messages[-1].setdefault("tool_calls", []).append({
                "id": item["call_id"], "type": "function",
                "function": {"name": item["name"], "arguments": item["arguments"]},
            })
        elif item.get("type") == "function_call_output":
            messages.append({"role": "tool", "tool_call_id": item["call_id"],
                             "content": item["output"]})
        else:
            messages.append({"role": item["role"], "content": item["content"]})
    return dict(body, messages=messages,
                tools=[{"type": "function", "function": {
                    key: value for key, value in tool.items() if key != "type"
                }} for tool in body.get("tools", [])],
                max_completion_tokens=body.get("max_output_tokens"),
                stream_options={"include_usage": True})


def anthropic_view(body):
    assert body["stream"] is True and body["model"]
    messages = []
    if "system" in body:
        messages.append({"role": "developer", "content": body["system"]})
    for entry in body["messages"]:
        role, content = entry["role"], entry["content"]
        if role == "assistant":
            text = ""
            tool_calls = []
            for block in content:
                if block["type"] == "text":
                    text = block["text"]
                else:
                    assert block["type"] == "tool_use"
                    tool_calls.append({
                        "id": block["id"], "type": "function",
                        "function": {"name": block["name"],
                                     "arguments": json.dumps(block["input"])},
                    })
            messages.append({"role": "assistant", "content": text,
                             "tool_calls": tool_calls})
        elif isinstance(content, list):
            for block in content:
                assert block["type"] == "tool_result"
                messages.append({"role": "tool", "tool_call_id": block["tool_use_id"],
                                 "content": block["content"]})
        else:
            messages.append({"role": "user", "content": content})
    return dict(body, messages=messages,
                tools=[{"type": "function", "function": {
                    "name": tool["name"], "description": tool["description"],
                    "parameters": tool["input_schema"],
                }} for tool in body.get("tools", [])],
                max_completion_tokens=body.get("max_tokens"),
                stream_options={"include_usage": True})


class AnthropicWire:
    """Translate Chat-shaped fixture frames into an Anthropic event stream."""

    def __init__(self, write):
        self.write = write
        self.started = False
        self.blocks = []
        self.closed = set()
        self.text_index = None
        self.calls = {}
        self.usage = None
        self.stop_reason = None

    def event(self, kind, **fields):
        value = dict(type=kind, **fields)
        self.write(f"event: {kind}\r\ndata: {json.dumps(value, ensure_ascii=False)}\r\n\r\n".encode())

    def start(self):
        self.event("message_start", message={
            "id": "msg_fixture", "type": "message", "role": "assistant",
            "model": "claude-fixture", "content": [], "stop_reason": None,
            "stop_sequence": None,
            "usage": {"input_tokens": 27, "output_tokens": 1},
        })
        self.started = True

    def open_text(self):
        if self.text_index is None:
            block = {"type": "text", "text": ""}
            index = len(self.blocks)
            self.blocks.append(block)
            self.event("content_block_start", index=index, content_block=block)
            self.text_index = index
        return self.text_index

    def close_text(self):
        if self.text_index is not None:
            self.event("content_block_stop", index=self.text_index)
            self.closed.add(self.text_index)
            self.text_index = None

    def tool_call(self, index, call):
        self.close_text()
        if index not in self.calls:
            function = call.get("function", {})
            block = {"type": "tool_use", "id": call["id"], "name": function.get("name", ""),
                     "input": {}}
            block_index = len(self.blocks)
            self.blocks.append(block)
            self.event("content_block_start", index=block_index, content_block=block)
            self.calls[index] = block_index
        block_index = self.calls[index]
        self.event("content_block_delta", index=block_index,
                   delta={"type": "input_json_delta",
                          "partial_json": call.get("function", {}).get("arguments", "")})

    def chunk(self, frame):
        if not self.started:
            self.start()
        if "usage" in frame:
            self.usage = (frame["usage"]["prompt_tokens"], frame["usage"]["completion_tokens"])
        for choice in frame.get("choices", []):
            delta = choice["delta"]
            if delta.get("content"):
                index = self.open_text()
                self.event("content_block_delta", index=index,
                           delta={"type": "text_delta", "text": delta["content"]})
            for call in sorted(delta.get("tool_calls", []), key=lambda c: c["index"]):
                self.tool_call(call["index"], call)
            if choice.get("finish_reason"):
                self.stop_reason = "tool_use" if choice["finish_reason"] == "tool_calls" else "end_turn"

    def done(self):
        self.close_text()
        for index, block in enumerate(self.blocks):
            if index not in self.closed:
                self.event("content_block_stop", index=index)
                self.closed.add(index)
        reason = self.stop_reason or ("tool_use" if self.calls else "end_turn")
        delta = {"stop_reason": reason, "stop_sequence": None}
        fields = {"delta": delta}
        if self.usage is not None:
            fields["usage"] = {"output_tokens": self.usage[1]}
        self.event("message_delta", **fields)
        self.event("message_stop")


class ResponsesWire:
    def __init__(self, write):
        self.write = write
        self.started = False
        self.items = []
        self.calls = {}
        self.message = None
        self.usage = None

    def event(self, kind, **fields):
        value = dict(type=kind, **fields)
        self.write(f"event: {kind}\r\ndata: {json.dumps(value, ensure_ascii=False)}\r\n\r\n".encode())

    def chunk(self, frame):
        if not self.started:
            self.event("response.created", response={"id": "resp_fixture", "status": "in_progress"})
            self.started = True
        if "usage" in frame:
            usage = frame["usage"]
            self.usage = {"input_tokens": usage["prompt_tokens"],
                          "output_tokens": usage["completion_tokens"]}
        for choice in frame.get("choices", []):
            delta = choice["delta"]
            if "content" in delta:
                if self.message is None:
                    self.message = len(self.items)
                    self.items.append({"id": "msg_fixture", "type": "message", "role": "assistant",
                                       "status": "in_progress", "content": []})
                    self.event("response.output_item.added", output_index=self.message, item=self.items[-1])
                    self.event("response.content_part.added", output_index=self.message,
                               item_id="msg_fixture", content_index=0,
                               part={"type": "output_text", "text": "", "annotations": []})
                    self.items[-1]["content"] = [{"type": "output_text", "text": "", "annotations": []}]
                self.event("response.output_text.delta", output_index=self.message,
                           item_id="msg_fixture", content_index=0, delta=delta["content"])
                self.items[self.message]["content"][0]["text"] += delta["content"]
            for call in sorted(delta.get("tool_calls", []), key=lambda c: c["index"]):
                index = call["index"]
                function = call["function"]
                if index not in self.calls:
                    output_index = len(self.items)
                    item = {"type": "function_call", "id": f"fc_{index}", "call_id": call["id"],
                            "name": function["name"], "arguments": "", "status": "in_progress"}
                    self.calls[index] = output_index
                    self.items.append(item)
                    self.event("response.output_item.added", output_index=output_index, item=item)
                output_index = self.calls[index]
                item = self.items[output_index]
                self.event("response.function_call_arguments.delta", output_index=output_index,
                           item_id=item["id"], delta=function.get("arguments", ""))
                item["arguments"] += function.get("arguments", "")

    def done(self):
        for index, item in enumerate(self.items):
            if item["type"] == "message":
                part = item["content"][0]
                self.event("response.output_text.done", output_index=index, item_id=item["id"],
                           content_index=0, text=part["text"])
                self.event("response.content_part.done", output_index=index, item_id=item["id"],
                           content_index=0, part=part)
            else:
                self.event("response.function_call_arguments.done", output_index=index,
                           item_id=item["id"], arguments=item["arguments"])
            item["status"] = "completed"
            self.event("response.output_item.done", output_index=index, item=item)
        response = {"id": "resp_fixture", "status": "completed", "output": self.items}
        if self.usage is not None:
            response["usage"] = self.usage
        self.event("response.completed", response=response)
