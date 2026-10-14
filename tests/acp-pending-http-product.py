"""ACP live streaming and cancellation against an independent HTTP Model."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import threading
import time

binary, base_file, model_plugin, destination = sys.argv[1:]
output = Path(destination)
output.mkdir(parents=True, exist_ok=True)
composition = json.loads(Path(base_file).read_text())
composition["artifacts"] = [
    path for path in composition["artifacts"]
    if "phenix-plugin-model-scripted" not in path
] + [model_plugin]
for binding in composition["slots"]:
    if binding["caller"] == "phenix.basic.agent" and binding["slot"] == "Model":
        binding["provider"] = "phenix.model.http"
composition["slots"].append({
    "caller": "phenix.model.http", "slot": "Journal", "index": 0,
    "provider": "phenix.basic.event-journal", "export": "Journal",
})
composition["grants"].append({
    "caller": "phenix.model.http", "interface": "phenix.basic.journal@1",
    "operation": "append", "resources": ["phenix.session.s1"],
})
configuration = output / "acp-pending-composition.json"
configuration.write_text(json.dumps(composition))

requests = []
mutex = threading.Lock()


class ModelServer(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_):
        pass

    def send_event(self, text, finished=False):
        data = {"choices": [{"index": 0, "delta": {"content": text},
                             "finish_reason": "stop" if finished else None}]}
        frame = ("data: " + json.dumps(data) + "\r\n\r\n").encode()
        for pos in range(0, len(frame), 3):
            chunk = frame[pos:pos + 3]
            self.wfile.write(f"{len(chunk):x}\r\n".encode() + chunk + b"\r\n")
        self.wfile.flush()

    def do_POST(self):
        assert self.path == "/completions"
        assert self.headers["Authorization"] == "Bearer acp-fixture"
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        assert request["stream"] is True
        with mutex:
            requests.append(request)
            ordinal = len(requests)
        assert ordinal in (1, 2, 3), ordinal
        messages = request["messages"]
        assert messages[-1]["role"] == "user"
        assert messages[-1]["content"] == {
            1: "cancel me", 2: "after cancel", 3: "client eof"
        }[ordinal]
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Transfer-Encoding", "chunked")
        self.end_headers()
        if ordinal in (1, 3):
            self.send_event("acp-live" if ordinal == 1 else "acp-eof")
            # Give the host time to cancel the active graph while the
            # independent HTTP server deliberately withholds completion.
            time.sleep(3.0 if ordinal == 1 else 8.0)
            try:
                self.send_event("unwanted", finished=True)
            except (BrokenPipeError, ConnectionResetError):
                return
        else:
            self.send_event("acp-resumed", finished=True)
        usage = b'data: {"choices":[],"usage":{"prompt_tokens":15,"completion_tokens":3}}\r\n\r\n'
        final = b"data: [DONE]\r\n\r\n"
        try:
            self.wfile.write(f"{len(usage):x}\r\n".encode() + usage + b"\r\n")
            self.wfile.write(f"{len(final):x}\r\n".encode() + final + b"\r\n")
            self.wfile.write(b"0\r\n\r\n")
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass


server = ThreadingHTTPServer(("127.0.0.1", 0), ModelServer)
thread = threading.Thread(target=server.serve_forever, daemon=True)
thread.start()
env = dict(os.environ)
env.update(
    PHENIX_MODEL_HTTP_ENDPOINT=f"http://127.0.0.1:{server.server_port}/completions",
    PHENIX_MODEL_HTTP_MODEL="acp-fixture",
    PHENIX_MODEL_HTTP_API_KEY="acp-fixture",
    PHENIX_BASIC_SQLITE_PATH=str(output / "acp-pending.db"),
)
proc = subprocess.Popen(
    [binary, "acp", str(configuration)],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    env=env, text=True, bufsize=1,
)
frames = queue.Queue()


def read_output():
    for line in proc.stdout:
        frames.put(json.loads(line))
    frames.put(None)


reader = threading.Thread(target=read_output, daemon=True)
reader.start()


def send(method, params, identifier=None):
    frame = {"jsonrpc": "2.0", "method": method, "params": params}
    if identifier is not None:
        frame["id"] = identifier
    proc.stdin.write(json.dumps(frame) + "\n")
    proc.stdin.flush()


def until(predicate, label):
    deadline = time.monotonic() + 25
    while time.monotonic() < deadline:
        try:
            frame = frames.get(timeout=min(2.0, max(0.01, deadline - time.monotonic())))
        except queue.Empty:
            continue
        assert frame is not None, f"ACP exited before {label}, status {proc.poll()}"
        assert frame.get("jsonrpc") == "2.0", frame
        if frame.get("error"):
            raise AssertionError((label, frame))
        if predicate(frame):
            return frame
    raise AssertionError(f"timeout waiting for {label}")


def delta(frame, value, session):
    if frame.get("method") != "session/update":
        return False
    params = frame.get("params", {})
    update = params.get("update", {})
    return (
        params.get("sessionId") == session
        and update.get("sessionUpdate") == "agent_message_chunk"
        and update.get("content", {}).get("text") == value
    )


try:
    send("initialize", {"protocolVersion": 1, "clientCapabilities": {}}, 1)
    assert until(lambda x: x.get("id") == 1, "initialize")["result"]["protocolVersion"] == 1
    send("session/new", {"cwd": "/tmp", "mcpServers": []}, 2)
    session = until(lambda x: x.get("id") == 2, "session/new")["result"]["sessionId"]
    send("session/prompt", {
        "sessionId": session, "prompt": [{"type": "text", "text": "cancel me"}],
    }, 3)
    until(lambda x: delta(x, "acp-live", session), "first live delta")
    # Invalid input must yield a protocol error without abandoning the root.
    proc.stdin.write("{malformed-json\n")
    proc.stdin.flush()
    invalid = frames.get(timeout=5)
    assert invalid is not None and invalid.get("error", {}).get("code") == -32700, invalid
    # The same JSON-RPC envelope validation must run during an active
    # prompt. An invalid cancellation is rejected immediately, rather
    # than being deferred until the model finishes or reaching the graph.
    proc.stdin.write(json.dumps({
        "jsonrpc": "1.0", "id": 73, "method": "session/cancel",
        "params": {"sessionId": session},
    }) + "\n")
    proc.stdin.flush()
    invalid_envelope = frames.get(timeout=5)
    assert invalid_envelope is not None, "missing pending-frame rejection"
    assert invalid_envelope.get("id") is None, invalid_envelope
    assert invalid_envelope.get("error", {}).get("code") == -32600, invalid_envelope
    proc.stdin.write(json.dumps({
        "jsonrpc": "2.0", "id": [], "method": "session/cancel",
        "params": {"sessionId": session},
    }) + "\n")
    proc.stdin.flush()
    invalid_id = frames.get(timeout=5)
    assert invalid_id is not None and invalid_id.get("id") is None, invalid_id
    assert invalid_id.get("error", {}).get("code") == -32600, invalid_id
    # A cancel sent as a request has no request handler in the official ACP
    # SDK. Its JSON-RPC method-not-found reply must preserve the ID, arrive
    # immediately, and leave the active graph untouched.
    send("session/cancel", {"sessionId": session}, 74)
    bad_cancel = frames.get(timeout=2)
    assert bad_cancel is not None and bad_cancel.get("id") == 74, bad_cancel
    assert bad_cancel.get("error", {}).get("code") == -32601, bad_cancel
    proc.stdin.write(json.dumps({
        "jsonrpc": "2.0", "id": None, "method": "session/cancel",
        "params": {"sessionId": session},
    }) + "\n")
    proc.stdin.flush()
    null_cancel = frames.get(timeout=2)
    assert null_cancel is not None and "id" in null_cancel, null_cancel
    assert null_cancel["id"] is None, null_cancel
    assert null_cancel.get("error", {}).get("code") == -32601, null_cancel
    # Unsupported notifications must not occupy the controller request queue.
    # Queue a valid session/new while the Model graph is running: the SDK
    # rejects unknown *methods* immediately, so those cannot test deferred
    # controller admission or the bounded FIFO.
    for _ in range(80):
        send("session/new", {"cwd": "/tmp", "mcpServers": []})
    send("session/new", {"cwd": "/tmp", "mcpServers": []}, 73)
    # A notification for another session cannot cancel this prompt.
    send("session/cancel", {"sessionId": "unrelated-session"})
    try:
        unexpected = frames.get(timeout=0.1)
        assert False, f"wrong-session cancel produced output: {unexpected}"
    except queue.Empty:
        pass
    send("session/cancel", {"sessionId": session})
    cancelled = until(lambda x: x.get("id") == 3, "cancelled prompt response")
    assert cancelled["result"]["stopReason"] == "cancelled", cancelled
    deferred_reply = until(lambda x: x.get("id") == 73, "deferred session/new")
    assert isinstance(deferred_reply.get("result", {}).get("sessionId"), str), deferred_reply

    send("session/prompt", {
        "sessionId": session, "prompt": [{"type": "text", "text": "after cancel"}],
    }, 4)
    until(lambda x: delta(x, "acp-resumed", session), "resumed delta")
    done = until(lambda x: x.get("id") == 4, "resumed completion")
    assert done["result"]["stopReason"] == "end_turn", done
    proc.stdin.close()
    assert proc.wait(timeout=20) == 0, proc.stderr.read()
    assert len(requests) == 2, len(requests)
    # Closing ACP stdin while a request is still live must cancel the
    # graph and release the owner marker, not wait for the HTTP reply.
    proc.stdout.close()
    proc.stderr.close()
    reader.join(timeout=5)
    proc = subprocess.Popen(
        [binary, "acp", str(configuration)],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        env=env, text=True, bufsize=1,
    )
    frames = queue.Queue()
    reader = threading.Thread(target=read_output, daemon=True)
    reader.start()
    send("initialize", {"protocolVersion": 1, "clientCapabilities": {}}, 51)
    until(lambda x: x.get("id") == 51, "EOF probe initialize")
    send("session/new", {"cwd": "/tmp", "mcpServers": []}, 52)
    eof_session = until(lambda x: x.get("id") == 52, "EOF probe session")["result"]["sessionId"]
    send("session/prompt", {
        "sessionId": eof_session,
        "prompt": [{"type": "text", "text": "client eof"}],
    }, 53)
    until(lambda x: delta(x, "acp-eof", eof_session), "EOF probe live model delta")
    proc.stdin.close()
    # The server withholds completion for eight seconds. Five seconds is
    # enough for normal root cancellation and kernel shutdown, but not
    # for a disconnected host that incorrectly waits for that completion.
    assert proc.wait(timeout=5) == 0, proc.stderr.read()
    assert len(requests) == 3, len(requests)
    assert (output / "acp-pending.db").exists()
    (output / "acp-pending-http.json").write_text(json.dumps({
        "format": "phenix.acp.pending-http-product.v1",
        "live_delta": True,
        "active_cancellation": True,
        "same_session_continuation": True,
        "malformed_frame_isolated": True,
        "invalid_active_request_rejected": True,
        "wrong_session_cancel_ignored": True,
        "unsupported_notifications_do_not_fill_queue": True,
        "active_cancel_requests_rejected_immediately": True,
        "cancel_as_request_uses_official_sdk_method_not_found": True,
        "client_eof_cancels_active_graph": True,
        "model_requests": len(requests),
    }, indent=2, sort_keys=True))
finally:
    if proc.poll() is None:
        proc.kill()
        proc.wait(timeout=10)
    if proc.stdin and not proc.stdin.closed:
        proc.stdin.close()
    proc.stdout.close()
    proc.stderr.close()
    reader.join(timeout=5)
    server.shutdown()
    server.server_close()
