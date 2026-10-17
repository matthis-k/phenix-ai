"""Packaged Basic HTTP preset uses the real provider, not the scripted Model."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import sys
import threading

binary, composition_path, out_dir = sys.argv[1:]
out = Path(out_dir)
out.mkdir(parents=True, exist_ok=True)
composition = json.loads(Path(composition_path).read_text())
assert not any("phenix-plugin-model-scripted" in p for p in composition["artifacts"])
assert any("phenix-plugin-model-http" in p for p in composition["artifacts"])
assert len([x for x in composition["slots"] if x["caller"] == "phenix.basic.agent"
            and x["slot"] == "Model" and x["provider"] == "phenix.model.http"]) == 1
assert any(x["caller"] == "phenix.model.http" and x["slot"] == "Journal"
           and x["provider"] == "phenix.basic.event-journal" for x in composition["slots"])
assert any(x["caller"] == "phenix.model.http"
           and x["interface"] == "phenix.basic.journal@1"
           and x["operation"] == "append" for x in composition["grants"])

requests = []
class Provider(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *args):
        pass
    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        requests.append(request)
        assert self.headers["Authorization"] == "Bearer live-fixture"
        assert request["model"] == "live-fixture-model"
        assert request["messages"][-1] == {"role": "user", "content": "hello from Neovim"}
        assert request["stream"] is True
        payload = [
            {"choices": [{"index": 0, "delta": {"content": "real-http-"}, "finish_reason": None}]},
            {"choices": [{"index": 0, "delta": {"content": "provider"}, "finish_reason": "stop"}]},
            {"choices": [], "usage": {"prompt_tokens": 8, "completion_tokens": 4}},
        ]
        body = "".join("data: " + json.dumps(item) + "\n\n" for item in payload)
        body += "data: [DONE]\n\n"
        encoded = body.encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)
        self.wfile.flush()

server = ThreadingHTTPServer(("127.0.0.1", 0), Provider)
threading.Thread(target=server.serve_forever, daemon=True).start()
try:
    env = os.environ.copy()
    env.update(
        PHENIX_MODEL_HTTP_ENDPOINT=f"http://127.0.0.1:{server.server_port}/completions",
        PHENIX_MODEL_HTTP_MODEL="live-fixture-model",
        PHENIX_MODEL_HTTP_API_KEY="live-fixture",
        PHENIX_BASIC_SQLITE_PATH=str(out / "live-fixture.sqlite"),
    )
    message = {"type": "prompt", "session": "s1", "text": "hello from Neovim"}
    result = subprocess.run(
        [binary, "jsonl"], input=json.dumps(message) + "\n",
        text=True, capture_output=True, env=env, timeout=40
    )
    assert result.returncode == 0, (result.stdout, result.stderr)
    events = [json.loads(line) for line in result.stdout.splitlines()]
    assert len(requests) == 1, requests
    assert [event["text"] for event in events if event["kind"] == "Answer"] == [
        "real-http-provider"
    ], events
    assert (out / "live-fixture.sqlite").exists()
    (out / "passed.json").write_text(json.dumps({
        "selected_model": "phenix.model.http",
        "http_requests": len(requests),
        "answer": "real-http-provider",
        "live_credentials_required_in_ci": False
    }))
finally:
    server.shutdown()
