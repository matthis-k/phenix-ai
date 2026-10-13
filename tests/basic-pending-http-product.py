"""End-to-end Basic graph with a real pending HTTP Model and two tool calls."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
import queue
from pathlib import Path
import subprocess
import sys
import threading
import time

binary, base_composition, http_model, destination = sys.argv[1:]
output = Path(destination)
output.mkdir(parents=True, exist_ok=True)

composition = json.loads(Path(base_composition).read_text())
composition["artifacts"] = [
    item for item in composition["artifacts"] if "phenix-plugin-model-scripted" not in item
] + [http_model]
model_slots = [
    item for item in composition["slots"]
    if item["caller"] == "phenix.basic.agent" and item["slot"] == "Model"
]
assert len(model_slots) == 1
model_slots[0]["provider"] = "phenix.model.http"
composition["slots"].append({
    "caller": "phenix.model.http", "slot": "Journal", "index": 0,
    "provider": "phenix.basic.event-journal", "export": "Journal",
})
composition["grants"].append({
    "caller": "phenix.model.http", "interface": "phenix.basic.journal@1",
    "operation": "append", "resources": ["phenix.session.s1"],
})
composition_file = output / "http-basic.json"
composition_file.write_text(json.dumps(composition, indent=2))

calls = []
lock = threading.Lock()


class Server(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_):
        pass

    def chunk(self, content):
        payload = f"data: {json.dumps(content, ensure_ascii=False)}\r\n\r\n".encode()
        # Deliberately split SSE frames and UTF-8 across HTTP chunks.
        for offset in range(0, len(payload), 2):
            data = payload[offset:offset + 2]
            self.wfile.write(f"{len(data):x}\r\n".encode() + data + b"\r\n")
        self.wfile.flush()

    def do_POST(self):
        assert self.path == "/completions"
        assert self.headers["Authorization"] == "Bearer pending-fixture"
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        assert request["stream"] is True
        assert request["stream_options"]["include_usage"] is True
        assert request["model"] == "pending-fixture"
        messages = request["messages"]
        with lock:
            calls.append(request)
            turn = len(calls)
        assert turn in (1, 2, 3, 4), "unexpected model continuation"
        if turn == 1:
            assert messages[-1] == {"role": "user", "content": "two tools"}
        elif turn == 2:
            assert messages[-1]["role"] == "tool"
            assert len([x for x in messages if x["role"] == "tool"]) == 2
            assert {x["tool_call_id"] for x in messages if x["role"] == "tool"} == {"c1", "c2"}
            assert any(x["role"] == "assistant" and len(x.get("tool_calls", [])) == 2 for x in messages)
        elif turn == 3:
            assert messages[-1] == {"role": "user", "content": "cancel during stream"}
        else:
            assert messages[-1] == {"role": "user", "content": "after cancel"}
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Transfer-Encoding", "chunked")
        self.end_headers()
        if turn == 1:
            self.chunk({"choices": [{"index": 0, "delta": {"content": "phase1-"}, "finish_reason": None}]})
            time.sleep(0.03)
            self.chunk({"choices": [{"index": 0, "delta": {"tool_calls": [
                {"index": 0, "id": "c1", "type": "function",
                 "function": {"name": "uppercase", "arguments": "{\"value\":\"hi\"}"}},
                {"index": 1, "id": "c2", "type": "function",
                 "function": {"name": "length", "arguments": "{\"value\":\"hi\"}"}},
            ]}, "finish_reason": "tool_calls"}]})
        elif turn == 2:
            self.chunk({"choices": [{"index": 0, "delta": {"content": "final-"}, "finish_reason": None}]})
            time.sleep(0.03)
            self.chunk({"choices": [{"index": 0, "delta": {"content": "ä"}, "finish_reason": "stop"}]})
        elif turn == 3:
            self.chunk({"choices": [{"index": 0, "delta": {"content": "cancel-start"}, "finish_reason": None}]})
            time.sleep(3.0)
            # The client should have cancelled the transport before the next chunk.
            try:
                self.chunk({"choices": [{"index": 0, "delta": {"content": "too-late"}, "finish_reason": "stop"}]})
            except (BrokenPipeError, ConnectionResetError):
                return
        else:
            self.chunk({"choices": [{"index": 0, "delta": {"content": "resumed"}, "finish_reason": "stop"}]})
        self.chunk({"choices": [], "usage": {"prompt_tokens": 15, "completion_tokens": 3}})
        done = b"data: [DONE]\r\n\r\n"
        self.wfile.write(f"{len(done):x}\r\n".encode() + done + b"\r\n")
        self.wfile.write(b"0\r\n\r\n")
        self.wfile.flush()


http = ThreadingHTTPServer(("127.0.0.1", 0), Server)
server = threading.Thread(target=http.serve_forever, daemon=True)
server.start()

try:
    env = dict(os.environ)
    env.update(
        PHENIX_MODEL_HTTP_ENDPOINT=f"http://127.0.0.1:{http.server_port}/completions",
        PHENIX_MODEL_HTTP_MODEL="pending-fixture",
        PHENIX_MODEL_HTTP_API_KEY="pending-fixture",
        PHENIX_BASIC_SQLITE_PATH=str(output / "basic-http.db"),
    )
    packet = {"type": "prompt", "session": "s1", "text": "two tools",
              "limits": {"model_turns": 2, "tool_calls": 2, "model_tokens": 20}}
    result = subprocess.run(
        [binary, "jsonl", str(composition_file)],
        input=json.dumps(packet) + "\n", text=True, capture_output=True,
        env=env, timeout=35,
    )
    assert result.returncode == 0, (result.returncode, result.stdout, result.stderr)
    events = [json.loads(line) for line in result.stdout.splitlines()]
    assert len(calls) == 2, f"expected one tool continuation: {len(calls)}"
    assert "".join(e["text"] for e in events if e["kind"] == "TextDelta") == "phase1-final-ä", events
    assert [e["kind"] for e in events].count("ToolStarted") == 2
    assert [e["kind"] for e in events].count("ToolFinished") == 2
    assert [e["text"] for e in events if e["kind"] == "Answer"] == ["final-ä"]
    assert (output / "basic-http.db").exists()

    # Keep stdin open while the first delta arrives, then cancel the live
    # GraphExecution. The next prompt must use the same durable session.
    proc = subprocess.Popen(
        [binary, "jsonl", str(composition_file)],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, bufsize=1, env=env,
    )
    observed = queue.Queue()

    def read_events():
        for item in proc.stdout:
            observed.put(json.loads(item))
        observed.put(None)

    reader = threading.Thread(target=read_events, daemon=True)
    reader.start()

    def write_packet(kind, text=None):
        packet = {"type": kind, "session": "s1"}
        if text is not None:
            packet["text"] = text
        proc.stdin.write(json.dumps(packet) + "\n")
        proc.stdin.flush()

    def until(predicate, label):
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            try:
                event = observed.get(timeout=min(2, max(0.01, deadline - time.monotonic())))
            except queue.Empty:
                continue
            assert event is not None, f"JSONL host exited before {label}: {proc.poll()}"
            if predicate(event):
                return event
        raise AssertionError(f"timed out waiting for {label}")

    try:
        write_packet("prompt", "cancel during stream")
        until(lambda e: e["kind"] == "TextDelta" and e["text"] == "cancel-start",
              "live streamed text")
        write_packet("cancel")
        until(lambda e: e["kind"] == "Cancelled", "durable cancellation")
        write_packet("prompt", "after cancel")
        until(lambda e: e["kind"] == "Answer" and e["text"] == "resumed",
              "continuation after cancellation")
        proc.stdin.close()
        assert proc.wait(timeout=25) == 0, proc.stderr.read()
        assert len(calls) == 4, f"unexpected model calls after cancellation: {len(calls)}"
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait(timeout=10)
        proc.stdin.close() if proc.stdin and not proc.stdin.closed else None
        proc.stdout.close()
        proc.stderr.close()
        reader.join(timeout=5)

    (output / "pending-basic-http.json").write_text(json.dumps({
        "format": "phenix.basic.pending-http-product.v1",
        "model_requests": len(calls),
        "tool_calls": 2,
        "streamed_text_exactly_once": True,
        "pending_basic_agent": True,
        "active_cancellation_tested": True,
        "durable_resume_after_cancel": True,
        "live_jsonl_delta": True,
    }, indent=2, sort_keys=True))
finally:
    http.shutdown()
    http.server_close()
