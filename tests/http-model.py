"""Independent HTTP/HTTPS server drives the real native Model and its tickets."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import ssl
import subprocess
import sys
import tempfile
import threading
import time
from model_protocol_fixture import AnthropicWire, ResponsesWire, anthropic_view, chat_view

binary, model, journal, storage, definitions, provider_definitions, output_dir = sys.argv[1:]
output = Path(output_dir)
output.mkdir(parents=True, exist_ok=True)
requests = []
lock = threading.Lock()
selected_received = set()
selected_output = None
protocol = os.environ.get("PHENIX_MODEL_HTTP_PROTOCOL", "openai-chat")
responses = protocol == "openai-responses"
anthropic = protocol == "anthropic-messages"


def frame(delta, finish=None):
    return {"choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}


class Server(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def write_chunk(self, data):
        self.wfile.write(f"{len(data):x}\r\n".encode() + data + b"\r\n")

    def end_chunks(self):
        self.wfile.write(b"0\r\n\r\n")
        self.wfile.flush()

    def log_message(self, *_):
        pass

    def chunk(self, value):
        if responses or anthropic:
            self.wire.chunk(value)
            return
        data = f"data: {json.dumps(value, ensure_ascii=False)}\r\n\r\n".encode()
        # Split even multibyte UTF-8 into distinct writes.
        for offset in range(0, len(data), 3):
            self.write_chunk(data[offset:offset + 3])
        self.wfile.flush()

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if responses:
            body = chat_view(body)
            self.wire = ResponsesWire(self.fragment)
        elif anthropic:
            body = anthropic_view(body)
            self.wire = AnthropicWire(self.fragment)
        mode = body["messages"][-1]["content"]
        selected = mode.startswith("selected-")
        name = mode.rsplit("-", 1)[-1] if selected else None
        with lock:
            requests.append(mode)
            if selected:
                selected_received.add(name)
                if selected_received == {"a", "b"}:
                    (selected_output / "both-selected-received").write_text("received")
        if anthropic:
            assert self.headers.get("x-api-key") == (f"selected-secret-{name}" if selected else "fixture-token")
            assert self.headers.get("anthropic-version") == "2023-06-01"
        else:
            assert self.headers.get("Authorization") == (f"Bearer selected-secret-{name}" if selected else "Bearer fixture-token")
        assert body["model"] == (f"selected-{name}" if selected else "fixture-model") and body["stream"] is True
        if selected:
            assert self.path == f"/completions/{name}"
        assert body["stream_options"]["include_usage"] is True
        assert body["max_completion_tokens"] == (1 if mode == "limit" else 32)
        assert "Use Rust" in body["messages"][0]["content"]
        assert "Phenix" in body["messages"][0]["content"]
        assert json.loads(body["messages"][2]["tool_calls"][0]["function"]["arguments"]) == {"value": "old"}
        assert body["messages"][3]["tool_call_id"] == "prior-1"
        assert "prior tool failed" in body["messages"][4]["content"]
        assert body["tools"][0]["function"]["parameters"]["properties"]["value"] == {"type": "string"}
        try:
            if mode == "status":
                self.send_response(429); self.send_header("Content-Length", "0"); self.end_headers(); return
            if mode == "redirect":
                self.send_response(307); self.send_header("Location", "/redirect-target"); self.send_header("Content-Length", "0"); self.end_headers(); return
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream; charset=utf-8")
            self.send_header("Transfer-Encoding", "chunked")
            self.end_headers()
            if mode == "timeout":
                time.sleep(0.3); return
            if mode == "malformed":
                self.write_chunk(b"data: invalid-json\n\n"); self.end_chunks(); return
            if mode == "provider-error":
                if anthropic:
                    self.write_chunk(b'event: error\ndata: {"type":"error","error":{"type":"overloaded_error","message":"secret upstream details"}}\n\n')
                elif responses:
                    self.write_chunk(b'event: error\ndata: {"type":"error","message":"secret upstream details"}\n\n')
                else:
                    self.chunk({"error": {"message": "secret upstream details"}})
                self.end_chunks(); return
            if mode == "multi":
                self.chunk(frame({"tool_calls": [
                    {"index": 1, "id": "c2", "type": "function", "function": {"name": "length", "arguments": '{"value":'}},
                    {"index": 0, "id": "c1", "type": "function", "function": {"name": "uppercase", "arguments": '{"value":'}},
                ]}))
                self.chunk(frame({"tool_calls": [
                    {"index": 0, "function": {"arguments": '"hello"}'}},
                    {"index": 1, "function": {"arguments": '"hello"}'}},
                ]}, "tool_calls"))
            else:
                self.chunk(frame({"content": "first-"}))
                if selected:
                    marker = selected_output / f"release-selected-{name}"
                    deadline = time.monotonic() + 15
                    while not marker.exists():
                        assert time.monotonic() < deadline, "selected release deadline"
                        # Polling only transports the explicit caller barrier.
                        threading.Event().wait(0.005)
                if mode in ("stream", "cancel"):
                    marker = output / ("live-progress" if mode == "stream" else "cancel-requested")
                    deadline = time.monotonic() + 15
                    while not marker.exists() and time.monotonic() < deadline:
                        time.sleep(0.005)
                    assert marker.exists(), f"native caller did not observe {mode}"
                self.chunk(frame({"content": "aß"}, "stop"))
            if mode == "truncated":
                self.end_chunks()
                return
            if mode != "no-usage":
                self.chunk({"choices": [], "usage": {"prompt_tokens": 27, "completion_tokens": 7}})
            if responses or anthropic:
                self.wire.done()
            else:
                self.write_chunk(b"data: [DONE]\r\n\r\n")
            self.end_chunks()
        except (BrokenPipeError, ConnectionResetError, ssl.SSLError):
            pass

    def fragment(self, data):
        for offset in range(0, len(data), 3):
            self.write_chunk(data[offset:offset + 3])
        self.wfile.flush()


http = ThreadingHTTPServer(("127.0.0.1", 0), Server)
threading.Thread(target=http.serve_forever, daemon=True).start()


def run(mode, endpoint, ca=None):
    env = os.environ.copy()
    env.update(PHENIX_MODEL_HTTP_ENDPOINT=endpoint, PHENIX_MODEL_HTTP_MODEL="fixture-model",
               PHENIX_MODEL_HTTP_API_KEY="fixture-token", PHENIX_BASIC_SQLITE_PATH=str(output / f"{mode}.db"))
    env.pop("PHENIX_MODEL_HTTP_CA_FILE", None)
    if ca:
        env["PHENIX_MODEL_HTTP_CA_FILE"] = str(ca)
    before = len(requests)
    subprocess.run([binary, mode, model, journal, storage, definitions, provider_definitions, output], env=env, check=True, timeout=30)
    if mode in ("denied", "zero", "bad-config", "tls-untrusted"):
        assert len(requests) == before, (mode, requests[before:])
    else:
        assert requests[before:] == ([mode, "multi"] if mode == "cancel" else [mode]), (mode, requests[before:])


endpoint = f"http://127.0.0.1:{http.server_port}/completions"
for mode in ["denied", "zero", "stream", "multi", "cancel", "status", "redirect", "provider-error", "malformed", "truncated", "no-usage", "limit", "timeout"]:
    run(mode, endpoint)
for mode in ("selected-complete", "selected-cancel"):
    selected_output = output / mode
    selected_output.mkdir()
    selected_received.clear()
    env = dict(os.environ, PHENIX_MODEL_HTTP_ENDPOINT=endpoint,
               PHENIX_MODEL_HTTP_MODEL="wrong-preset-model",
               PHENIX_MODEL_HTTP_API_KEY="wrong-preset-secret",
               PHENIX_BASIC_SQLITE_PATH=str(selected_output / "journal.db"))
    before = len(requests)
    completed = subprocess.run([binary, mode, model, journal, storage, definitions,
                               provider_definitions, selected_output], env=env,
                               capture_output=True, text=True, timeout=30)
    assert completed.returncode == 0, (mode, completed.stdout, completed.stderr)
    assert "selected-secret-" not in completed.stdout + completed.stderr
    retirement = json.loads((selected_output / f"{mode}.json").read_text())
    assert retirement["all_physical_leases_released_before_retirement"] is True
    assert retirement["all_roots_released_before_successful_retirement"] is True
    assert retirement["final_kernel_reference_destructor_observed"] is True
    identities = retirement["destroyed_kernel_generations"]
    assert set(identities) == {"s-a", "s-b"}
    assert identities["s-a"] != identities["s-b"]
    for session, identity in identities.items():
        assert completed.stderr.splitlines().count(
            f"test-http-fixture: kernel-destroyed {session} {identity}") == 1
    if os.environ.get("PHENIX_TEST_HTTP_LIFECYCLE") == "1":
        events = [line for line in completed.stderr.splitlines()
                  if line.startswith(("test-http-lifecycle:", "test-http-fixture:"))]
        def position(event):
            assert events.count(event) == 1, (mode, event, events)
            return events.index(event)
        held = position("test-http-fixture: both-held")
        a_drained = position("test-http-fixture: a-drained-b-held")
        released = position("test-http-fixture: both-released")
        roots_dropped = position("test-http-fixture: roots-dropped")
        retired = position("test-http-fixture: a-retired")
        destroyed = position("test-http-fixture: generations-dropped")
        assert held < a_drained < released < roots_dropped < retired < destroyed
        for name, lower, upper in (("a", held, a_drained), ("b", a_drained, released)):
            assert lower < position(f"test-http-lifecycle: worker-joined s-{name}") < upper
            assert lower < position(f"test-http-lifecycle: config-dropped selected-{name}") < upper
        assert roots_dropped < position("test-http-lifecycle: state-dropped s-a") < retired
        assert retired < position("test-http-lifecycle: state-dropped s-b") < destroyed
        a_kernel = position(f"test-http-fixture: kernel-destroyed s-a {identities['s-a']}")
        b_kernel = position(f"test-http-fixture: kernel-destroyed s-b {identities['s-b']}")
        assert retired < a_kernel < position("test-http-lifecycle: state-dropped s-b")
        assert destroyed < b_kernel
        (selected_output / "lifecycle-evidence.json").write_text(json.dumps({
            "events": events,
            "actual_join_return_observed": True,
            "config_destructor_observed": True,
            "native_state_destructor_observed": True,
            "final_kernel_reference_destructor_observed": True,
            "destroyed_kernel_generations": identities,
        }, sort_keys=True))
    assert sorted(requests[before:]) == [f"{mode}-a", f"{mode}-b"]
run("bad-config", "file:///tmp/forbidden-model")
# A credentialed Model must reject non-loopback plaintext before transport.
run("bad-config", "http://192.0.2.42/completions")
run("bad-config", "http://localhost:8800/completions")

with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)
    cert, key = root / "cert.pem", root / "key.pem"
    subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
                    "-subj", "/CN=localhost", "-addext", "subjectAltName=IP:127.0.0.1",
                    "-keyout", key, "-out", cert], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    https = ThreadingHTTPServer(("127.0.0.1", 0), Server)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(cert, key)
    https.socket = context.wrap_socket(https.socket, server_side=True)
    threading.Thread(target=https.serve_forever, daemon=True).start()
    endpoint = f"https://127.0.0.1:{https.server_port}/completions"
    run("tls-untrusted", endpoint)
    run("tls", endpoint, cert)
    https.shutdown()
http.shutdown()
(output / "http-model.json").write_text(json.dumps({
    "format": "phenix.basic.http-model.v1", "cases": requests,
    "negative_zero_request_cases": ["denied", "zero", "bad-config", "tls-untrusted"],
    "live_journal_before_completion": True, "cancelled_cleanup_writes": False,
    "caller_selected_limits": True, "https_verified": True,
    "basic_agent_integration": False,
    "independent_instances_overlap": True,
    "same_instance_busy_rejected": True,
}, indent=2, sort_keys=True))
