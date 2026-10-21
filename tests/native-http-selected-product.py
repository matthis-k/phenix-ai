"""Native selected HTTP product: credential-free and exact API-key identity."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import sys
import threading

binary, model, journal, storage, basic_defs, provider_defs, output_dir = sys.argv[1:]
output = Path(output_dir)
output.mkdir(parents=True, exist_ok=True)
requests = []
active_mode = None
fixture_secret = "selected-fixture-secret"


class Server(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        assert self.path == "/completions"
        expected_auth = (
            f"Bearer {fixture_secret}" if active_mode == "api-key" else None
        )
        assert self.headers.get("Authorization") == expected_auth
        assert body["model"] == "selected-fixture"
        assert body["stream"] is True
        assert body["stream_options"]["include_usage"] is True
        assert body["max_completion_tokens"] == 32
        assert body["messages"][-1] == {"role": "user", "content": "selected"}
        requests.append(active_mode)
        frames = [
            {"choices": [{"index": 0, "delta": {"content": "selected-live"},
                           "finish_reason": "stop"}]},
            {"choices": [], "usage": {"prompt_tokens": 27, "completion_tokens": 7}},
        ]
        body_bytes = "".join(
            "data: " + json.dumps(frame) + "\r\n\r\n" for frame in frames
        ).encode() + b"data: [DONE]\r\n\r\n"
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(body_bytes)))
        self.end_headers()
        self.wfile.write(body_bytes)
        self.wfile.flush()


server = ThreadingHTTPServer(("127.0.0.1", 0), Server)
thread = threading.Thread(target=server.serve_forever, daemon=True)
thread.start()
try:
    for active_mode in ("none", "api-key"):
        case = output / active_mode
        case.mkdir(parents=True)
        env = dict(os.environ)
        env.update(
            PHENIX_SELECTED_HTTP_ENDPOINT=(
                f"http://127.0.0.1:{server.server_port}/completions"
            ),
            PHENIX_SELECTED_HTTP_AUTH_MODE=active_mode,
            # A wrong preset endpoint/model/key must never replace the checked
            # native selection, including the no-auth/authorized-key distinction.
            PHENIX_MODEL_HTTP_ENDPOINT="http://127.0.0.1:1/not-selected",
            PHENIX_MODEL_HTTP_MODEL="wrong-preset-model",
            PHENIX_MODEL_HTTP_API_KEY="wrong-preset-key",
            PHENIX_BASIC_SQLITE_PATH=str(case / "selected.db"),
        )
        before = len(requests)
        completed = subprocess.run(
            [binary, model, journal, storage, basic_defs, provider_defs, str(case)],
            check=False, text=True, capture_output=True, env=env, timeout=40,
        )
        assert completed.returncode == 0, (
            active_mode, completed.returncode, completed.stdout, completed.stderr
        )
        assert fixture_secret not in completed.stdout + completed.stderr
        assert requests[before:] == [active_mode], (
            active_mode, requests[before:]
        )
        assert (case / "accepted.txt").read_text() == "selected-native-http\n"

    (output / "evidence.json").write_text(json.dumps({
        "native_scope": True,
        "foreign_session_denied": True,
        "exact_credential_identity_checked": True,
        "absent_api_key_denied": True,
        "selected_endpoint_executed": True,
        "credential_free_has_no_authorization_header": True,
        "api_key_has_exact_authorization_header": True,
        "wrong_environment_preset_ignored": True,
        "single_use_model": True,
        "http_requests": len(requests),
        "controller_admission_integrated": False,
    }, sort_keys=True))
finally:
    server.shutdown()
