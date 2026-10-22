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

if len(sys.argv) not in (7, 10):
    raise SystemExit("Expected provider product arguments plus optionally 3 packaged compositions")
runtime, http_basic, provider_artifact, credentials_artifact, definitions, destination = sys.argv[1:7]
packaged_paths = sys.argv[7:]
output = Path(destination)
output.mkdir(parents=True, exist_ok=True)
key = secrets.token_hex(32)
received = []
state = {"get": 0, "post": 0, "redirected": 0}
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
        if self.path != "/models":
            state["redirected"] += 1
            self.send_response(404)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        state["get"] += 1
        authorization = self.headers.get("Authorization")
        if authorization == "Bearer " + key:
            code, body = 200, {"object": "list", "data": [
                {"id": "gpt-5"}, {"id": "text-embedding-3-large"},
            ]}
        else:
            code = {
                "Bearer denied-test-token": 403,
                "Bearer limited-test-token": 429,
                "Bearer unavailable-test-token": 503,
                "Bearer malformed-test-token": 200,
                "Bearer redirect-test-token": 302,
            }.get(authorization, 401)
            body = {"error": {"message": "verification did not succeed"}}
        payload = (b"{not-json" if authorization == "Bearer malformed-test-token"
                   else json.dumps(body).encode())
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        if code == 302:
            self.send_header("Location",
                             f"http://127.0.0.1:{server.server_port}/redirected")
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
                 "operation": operation, "resources": [resource]}
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
            "resources": [resource],
        },
        resource=resource,
    )
    if packaged_paths:
        # Reuse the exact *installed* launcher authority specifications.
        # Replace only the production provider library with its independently
        # built loopback-test variant; no grants, slots, identities, or model
        # installation implementation are modified for this acceptance run.
        actual = {
            name: json.loads(Path(path).read_text())
            for name, path in zip(("management", "credential", "resolver"),
                                  packaged_paths, strict=True)
        }
        for name, expected in (("management", provider),
                               ("credential", credentials),
                               ("resolver", resolver)):
            installed = actual[name]
            assert installed["entry"] == expected["entry"], name
            assert installed["slots"] == expected["slots"], name
            assert installed["grants"] == expected["grants"], name
            assert installed["authority"] == expected["authority"], name
            assert installed["resource"] == expected["resource"], name
            assert installed["definitions"] == [definitions], name
            assert installed["excluded"] == installed["substitutions"] == [], name
        assert actual["management"]["artifacts"] == [
            actual["management"]["artifacts"][0], credentials_artifact
        ], actual["management"]["artifacts"]
        assert len(actual["management"]["artifacts"]) == 2
        assert actual["credential"]["artifacts"] == [credentials_artifact]
        assert actual["resolver"]["artifacts"] == [credentials_artifact]
        actual["management"]["artifacts"][0] = provider_artifact
        credentials, resolver, provider = (
            actual["credential"], actual["resolver"], actual["management"]
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

    def launch(configured_profile="personal"):
        launch_env = dict(env)
        if configured_profile is None:
            launch_env.pop("PHENIX_OPENAI_API_PROFILE", None)
        else:
            launch_env["PHENIX_OPENAI_API_PROFILE"] = configured_profile
        p = subprocess.Popen(
            [runtime, "acp", http_basic,
             "--provider-management", configs["management"],
             "--provider-credentials", configs["credential"],
             "--provider-model-resolver", configs["resolver"]],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, bufsize=1, env=launch_env,
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
    unsupported = call(p, 70, "_phenix/providers/auth/begin",
                       {**user, "method_id": "oauth-device"})
    assert "error" in unsupported, unsupported
    cancelled_start = call(p, 71, "_phenix/providers/auth/begin",
                           {**user, "method_id": "api-key"})
    cancelled_id = cancelled_start["result"]["ApiKeyInput"]["attempt_id"]
    cancelled = call(p, 72, "_phenix/providers/auth/cancel",
                     {**user, "attempt_id": cancelled_id})
    assert cancelled["result"] == "Cancelled", cancelled
    no_replay = call(p, 73, "_phenix/providers/auth/poll",
                     {**user, "attempt_id": cancelled_id})
    assert "error" in no_replay, no_replay
    forged = call(p, 5, "_phenix/credentials/import",
                  {**account, "profile_id": "work", "secret": key})
    assert forged["error"]["code"] == -32602
    invalid = call(p, 6, "_phenix/credentials/import",
                   {**account, "secret": "wrong-test-token"})
    assert invalid["result"]["ok"] is True
    after_wrong = call(p, 7, "_phenix/providers/auth/status", user)
    assert after_wrong["result"]["profiles"][0]["status"]["Unavailable"]
    # A stored credential is never equivalent to a provider-verified login.
    # Denied, throttled, unavailable, malformed and redirected probes fail closed.
    for index, token in enumerate((
            "denied-test-token", "limited-test-token",
            "unavailable-test-token", "malformed-test-token",
            "redirect-test-token"), start=30):
        imported = call(p, index * 2, "_phenix/credentials/import",
                        {**account, "secret": token})
        assert imported["result"]["ok"] is True, imported
        checked_status = call(p, index * 2 + 1,
                              "_phenix/providers/auth/status", user)
        assert checked_status["result"]["profiles"][0]["status"]["Unavailable"], checked_status
        assert token not in json.dumps(checked_status), "credential leaked"
    assert state["redirected"] == 0, "provider followed an unauthorized redirect"
    assert state["post"] == 0, "invalid credential reached inference"
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
    wrong_account = call(p, 74, "_phenix/providers/models",
                         {**user, "profile_id": "work", "cursor": None, "limit": 256})
    assert "error" in wrong_account, wrong_account
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
    # The authorized root, not a process-global profile default, must
    # select this account. Omitting the optional restriction still works,
    # while a conflicting deployment restriction fails before network I/O.
    root_selected = launch(configured_profile=None)
    init(root_selected)
    root_status = call(root_selected, 79, "_phenix/providers/auth/status", user)
    assert root_status["result"]["profiles"][0]["status"] == "Authenticated", root_status
    close(root_selected)
    before_denial = state["get"]
    restricted = launch(configured_profile="work")
    init(restricted)
    mismatch = call(restricted, 80, "_phenix/providers/auth/status", user)
    assert "error" in mismatch, mismatch
    assert state["get"] == before_denial, "mismatched account reached provider network"
    close(restricted)

    # Fresh ACP process revalidates the exact credential after session restart.
    restarted = launch()
    init(restarted)
    fresh_status = call(restarted, 20, "_phenix/providers/auth/status", user)
    assert fresh_status["result"]["profiles"][0]["status"] == "Authenticated"
    loaded = call(restarted, 21, "session/load",
                  {"sessionId": session, "cwd": "/tmp", "mcpServers": []})
    assert "result" in loaded, loaded
    close(restarted)
    revoked = call(p, 15, "_phenix/credentials/revoke", account)
    assert revoked["result"]["ok"] is True
    unavailable = call(p, 16, "_phenix/providers/auth/status", user)
    assert unavailable["result"]["profiles"][0]["status"]["Unavailable"]
    denied_prompt = call(p, 17, "session/prompt",
                         {"sessionId": session, "prompt": [
                             {"type": "text", "text": "must not execute"}]})
    assert "error" in denied_prompt, denied_prompt
    close(p)
    after_revoke = launch()
    init(after_revoke)
    denied_load = call(after_revoke, 22, "session/load",
                       {"sessionId": session, "cwd": "/tmp", "mcpServers": []})
    assert "error" in denied_load, denied_load
    close(after_revoke)

    assert not list(vault.iterdir()), "key not revoked from vault"
    assert state["redirected"] == 0, state
    assert state["get"] >= 10 and state["post"] == 1, state

server.shutdown()
(output / "openai-api-auth-product.json").write_text(json.dumps({
    "format": "phenix.c11.openai-api-product.v1",
    "missing_unverified": True, "invalid_key_unverified": True,
    "exact_profile_grant": True, "begin_poll_completed": True,
    "authorized_model_discovery": True, "streamed_selected_http": True,
    "revoke_invalidates_status": True, "secret_not_logged": True,
    "invalid_http_fail_closed": True, "redirect_not_followed": True,
    "restart_revalidates_profile": True, "revocation_denies_selected_prompt": True,
    "cross_profile_model_denied": True, "unsupported_oauth_denied": True,
    "root_selected_account_without_env": True,
    "deployment_mismatch_denied_before_network": True,
    "cancelled_auth_attempt_not_replayable": True,
    "shipping_launcher_authority_exercised": bool(packaged_paths),
}, sort_keys=True, indent=2))
