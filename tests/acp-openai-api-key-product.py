"""Installed real M1 API-key auth, credential scope and ACP HTTP selection.

All credentials, stream payloads and the temporary SQLite database remain
outside the Nix output. A dedicated Cargo feature substitutes literal 127.0.0.1
URLs only for the test fixture, not the distributed production provider.
"""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from model_protocol_fixture import ResponsesWire
import json
import os
import queue
import secrets
import subprocess
import sys
import tempfile
import threading
import time

runtime, http_basic, provider_artifact, credentials_artifact, definitions, destination = sys.argv[1:]
output = Path(destination)
output.mkdir(parents=True, exist_ok=True)
key = secrets.token_hex(32)
received = []
state = {"get": 0, "post": 0}
account = {"provider_id": "openai", "connection_id": "api", "profile_id": "personal"}
resource = "phenix.credentials." + ".".join(
    account[name].encode().hex()
    for name in ("provider_id", "connection_id", "profile_id")
)
credential_contract = "phenix.provider.credentials@1"
provider_contract = "phenix.provider.catalog@1"
auth_contract = "phenix.provider.auth@1"
scope = "phenix.providers.user"

class HTTP(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *_): pass
    def do_GET(self):
        assert self.path == "/models", self.path
        state["get"] += 1
        if self.headers.get("Authorization") != "Bearer " + key:
            code, body = 401, {"error": {"message": "invalid credential"}}
        else:
            code, body = 200, {"object": "list", "data": [
                {"id": "gpt-5"}, {"id": "text-embedding-3-large"},
            ]}
        payload = json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)
        self.wfile.flush()
    def do_POST(self):
        assert self.path == "/responses", self.path
        assert self.headers.get("Authorization") == "Bearer " + key
        packet = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        assert packet["model"] == "gpt-5", packet["model"]
        assert packet["stream"] is True
        received.append(packet["model"])
        state["post"] += 1
        parts = []
        writer = ResponsesWire(parts.append)
        writer.chunk({"choices": [{"index": 0, "delta": {
            "content": "verified-openai-api"}, "finish_reason": "stop"}]})
        writer.chunk({"choices": [], "usage": {"prompt_tokens": 5, "completion_tokens": 4}})
        writer.done()
        payload = b"".join(parts)
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)
        self.wfile.flush()

server = ThreadingHTTPServer(("127.0.0.1", 0), HTTP)
threading.Thread(target=server.serve_forever, daemon=True).start()
children = []
with tempfile.TemporaryDirectory(prefix="phenix-c11-api-key-") as temporary:
    root = Path(temporary)
    vault = root / "vault"
    vault.mkdir(mode=0o700)
    common = {
        "format": "phenix.composition.v3",
        "definitions": [definitions],
        "excluded": [], "substitutions": [],
    }
    credentials = dict(
        **common,
        entry={"interface": credential_contract, "operation": "import",
               "provider": "phenix.provider.credential-files"},
        artifacts=[credentials_artifact], slots=[],
        grants=[{"caller": "root", "interface": credential_contract,
                 "operation": operation, "resources": [resource]}
                for operation in ("import", "revoke")],
        authority={"operations": [credential_contract + "/" + op
                                  for op in ("import", "revoke")],
                   "resources": [resource]},
        resource=resource,
    )
    resolver = json.loads(json.dumps(credentials))
    resolver["entry"]["operation"] = "resolve"
    resolver["grants"] = [{"caller": "root", "interface": credential_contract,
                           "operation": "resolve", "resources": [resource]}]
    resolver["authority"]["operations"] = [credential_contract + "/resolve"]
    provider = dict(
        **common,
        entry={"interface": provider_contract, "operation": "list",
               "provider": "phenix.provider.openai-api"},
        artifacts=[provider_artifact, credentials_artifact],
        slots=[{"caller": "phenix.provider.openai-api", "slot": "Credentials",
                "provider": "phenix.provider.credential-files",
                "export": "Credentials", "index": 0}],
        grants=[{"caller": "root", "interface": interface,
                 "operation": operation, "resources": [scope]}
                for interface, operation in [
                    (provider_contract, "list"), (provider_contract, "describe"),
                    (provider_contract, "models"), (auth_contract, "status"),
                    (auth_contract, "begin"), (auth_contract, "poll"),
                    (auth_contract, "cancel"),
                ]] + [{"caller": "phenix.provider.openai-api",
                        "interface": credential_contract,
                        "operation": "resolve", "resources": [resource]}],
        authority={
            "operations": [interface + "/" + operation
                           for interface, operation in [
                               (provider_contract, "list"),
                               (provider_contract, "describe"),
                               (provider_contract, "models"),
                               (auth_contract, "status"),
                               (auth_contract, "begin"),
                               (auth_contract, "poll"),
                               (auth_contract, "cancel"),
                           ]] + [credential_contract + "/resolve"],
            "resources": [scope, resource],
        },
        resource=scope,
    )
    configs = {}
    for name, data in [("credential", credentials), ("resolver", resolver),
                       ("management", provider)]:
        path = root / (name + ".json")
        path.write_text(json.dumps(data))
        configs[name] = str(path)
    env = dict(os.environ, PHENIX_CREDENTIAL_FILE_CONSENT="1",
               PHENIX_CREDENTIAL_FILE_DIRECTORY=str(vault),
               PHENIX_OPENAI_API_PROFILE="personal",
               PHENIX_C11_TEST_BASE="http://127.0.0.1:" + str(server.server_port),
               PHENIX_BASIC_SQLITE_PATH=str(root / "selected.sqlite"),
               PHENIX_MODEL_HTTP_ENDPOINT="http://127.0.0.1:1/forged-preset",
               PHENIX_MODEL_HTTP_API_KEY="no-preset-token")

    def launch():
        p = subprocess.Popen(
            [runtime, "acp", http_basic,
             "--provider-management", configs["management"],
             "--provider-credentials", configs["credential"],
             "--provider-model-resolver", configs["resolver"]],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, bufsize=1, env=env,
        )
        children.append(p)
        p.responses = queue.Queue()
        def reader():
            for line in p.stdout: p.responses.put(line)
            p.responses.put(None)
        threading.Thread(target=reader, daemon=True).start()
        return p

    def call(p, request_id, method, params):
        p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": request_id,
                                   "method": method, "params": params}) + "\n")
        p.stdin.flush()
        for _ in range(256):
            raw = p.responses.get(timeout=40)
            assert raw is not None, (method, p.poll())
            assert key not in raw, "secret leaked in ACP response"
            data = json.loads(raw)
            if data.get("method") == "session/update": continue
            assert data.get("id") == request_id, (method, data)
            return data
        raise AssertionError("excessive ACP notifications")

    def init(p):
        value = call(p, 1, "initialize", {"protocolVersion": 1,
                                           "clientCapabilities": {}})
        assert value["result"]["protocolVersion"] == 1

    def close(p):
        p.stdin.close()
        assert p.wait(timeout=15) == 0, p.stderr.read()[-500:]
        err = p.stderr.read()
        assert key not in err
        p.stdout.close()
        p.stderr.close()

    p = launch()
    init(p)
    user = {"scope": "user", "provider_id": "openai", "connection_id": "api"}
    before = call(p, 2, "_phenix/providers/auth/status", user)
    assert before["result"]["profiles"][0]["status"]["Unavailable"]
    started = call(p, 3, "_phenix/providers/auth/begin",
                   {**user, "method_id": "api-key"})
    attempt = started["result"]["ApiKeyInput"]["attempt_id"]
    pending = call(p, 4, "_phenix/providers/auth/poll",
                   {**user, "attempt_id": attempt})
    assert pending["result"] == "Pending", pending
    forged = call(p, 5, "_phenix/credentials/import",
                  {**account, "profile_id": "work", "secret": key})
    assert forged["error"]["code"] == -32602
    invalid = call(p, 6, "_phenix/credentials/import",
                   {**account, "secret": "wrong-test-token"})
    assert invalid["result"]["ok"] is True
    after_wrong = call(p, 7, "_phenix/providers/auth/status", user)
    assert after_wrong["result"]["profiles"][0]["status"]["Unavailable"]
    checked = call(p, 8, "_phenix/credentials/import", {**account, "secret": key})
    assert checked["result"]["ok"] is True
    accepted = call(p, 9, "_phenix/providers/auth/poll",
                    {**user, "attempt_id": attempt})
    assert accepted["result"]["Completed"]["profile"]["id"] == "personal", accepted
    after_auth = call(p, 10, "_phenix/providers/auth/status", user)
    assert after_auth["result"]["profiles"][0]["status"] == "Authenticated"
    listed = call(p, 11, "_phenix/providers/models",
                  {**user, "profile_id": "personal", "cursor": None, "limit": 256})
    assert [m["id"] for m in listed["result"]["models"]] == ["gpt-5"], listed
    created = call(p, 12, "session/new", {"cwd": "/tmp", "mcpServers": []})
    session = created["result"]["sessionId"]
    config = created["result"]["configOptions"][0]
    choices = [x["value"] for x in config["options"]]
    selected = [x for x in choices if x.startswith("phenix.native-model@1:")]
    assert len(selected) == 1, created
    set_model = call(p, 13, "session/set_config_option",
                     {"sessionId": session, "configId": "model",
                      "value": selected[0]})
    assert "result" in set_model, set_model
    prompted = call(p, 14, "session/prompt",
                    {"sessionId": session, "prompt": [
                        {"type": "text", "text": "hello"}]})
    assert prompted.get("result", {}).get("stopReason") == "end_turn", prompted
    assert received == ["gpt-5"], received
    revoked = call(p, 15, "_phenix/credentials/revoke", account)
    assert revoked["result"]["ok"] is True
    unavailable = call(p, 16, "_phenix/providers/auth/status", user)
    assert unavailable["result"]["profiles"][0]["status"]["Unavailable"]
    close(p)

    assert not list(vault.iterdir()), "key not revoked from vault"
    assert state["get"] >= 5 and state["post"] == 1, state

server.shutdown()
(output / "openai-api-auth-product.json").write_text(json.dumps({
    "format": "phenix.c11.openai-api-product.v1",
    "missing_unverified": True, "invalid_key_unverified": True,
    "exact_profile_grant": True, "begin_poll_completed": True,
    "authorized_model_discovery": True, "streamed_selected_http": True,
    "revoke_invalidates_status": True, "secret_not_logged": True,
}, sort_keys=True, indent=2))
