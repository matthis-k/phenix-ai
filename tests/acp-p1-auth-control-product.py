"""Black-box P1 authentication *control plane*, without any Model invocation.

Invoke once per connection to keep API-key and OAuth failures distinguishable.
Both scenarios run against native provider composition and real ACP routing.
No session, model catalog, HTTP request, or external login is required.
"""
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading

runtime, basic, management, connection, destination = sys.argv[1:]
assert connection in ("api", "chatgpt"), connection
output = Path(destination)
output.mkdir(parents=True, exist_ok=True)


class Acp:
    def __init__(self, env):
        self.process = subprocess.Popen(
            [runtime, "acp", basic, "--provider-management", management],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, bufsize=1, env=env,
        )
        self.frames = queue.Queue()

        def reader():
            for line in self.process.stdout:
                self.frames.put(line)
            self.frames.put(None)

        threading.Thread(target=reader, daemon=True).start()
        self.serial = 0

    def call(self, method, params):
        assert not method.startswith("session/"), "auth tests must not use a session"
        self.serial += 1
        self.process.stdin.write(json.dumps({
            "jsonrpc": "2.0", "id": self.serial, "method": method,
            "params": params,
        }) + "\n")
        self.process.stdin.flush()
        for _ in range(64):
            try:
                raw = self.frames.get(timeout=30)
            except queue.Empty as cause:
                raise AssertionError(f"no ACP reply to {method}") from cause
            assert raw is not None, (method, self.process.poll())
            reply = json.loads(raw)
            if reply.get("method") == "session/update":
                continue
            assert reply.get("id") == self.serial, reply
            return reply
        raise AssertionError(f"excessive notifications for {method}")

    def close(self):
        self.process.stdin.close()
        assert self.process.wait(timeout=30) == 0, self.process.stderr.read()[-1500:]
        self.process.stdout.close()
        self.process.stderr.close()

    def abort(self):
        if self.process.poll() is None:
            self.process.kill()
            self.process.wait(timeout=10)


with tempfile.TemporaryDirectory(prefix="phenix-auth-control-") as private:
    root = Path(private)
    vault = root / "vault"
    vault.mkdir(mode=0o700)
    env = dict(os.environ)
    for name in (
        "OPENAI_API_KEY", "ANTHROPIC_API_KEY", "OPENROUTER_API_KEY",
        "PHENIX_CREDENTIAL_API_KEY_ENV_NAME", "PHENIX_MODEL_HTTP_API_KEY",
    ):
        env.pop(name, None)
    env.update(
        PHENIX_CREDENTIAL_FILE_CONSENT="1",
        PHENIX_CREDENTIAL_FILE_DIRECTORY=str(vault),
        PHENIX_BASIC_SQLITE_PATH=str(root / "session.sqlite"),
        PHENIX_MODEL_HTTP_ENDPOINT="http://127.0.0.1:1/never-invoke",
        PHENIX_MODEL_HTTP_MODEL="not-selected",
    )
    client = Acp(env)
    try:
        init = client.call("initialize", {
            "protocolVersion": 1, "clientCapabilities": {},
        })
        assert init.get("result", {}).get("protocolVersion") == 1, init

        catalog = client.call("_phenix/providers/list", {"scope": "user"})
        assert "result" in catalog, catalog
        providers = [item for item in catalog["result"]["providers"]
                     if item["id"] == "openai"]
        assert len(providers) == 1, providers
        connections = {item["id"]: item
                       for item in providers[0]["connections"]}
        assert connection in connections, connections
        expected_kind = "ApiKey" if connection == "api" else "OAuthAuthorizationCode"
        assert connections[connection]["auth_methods"][0]["kind"] == expected_kind
        scope = {"scope": "user", "provider_id": "openai",
                 "connection_id": connection}
        status = client.call("_phenix/providers/auth/status", scope)
        assert status["result"]["connection_id"] == connection, status
        initial_profiles = status["result"]["profiles"]
        if connection == "api":
            assert [(item["id"], item["method_id"]) for item in initial_profiles] == [
                ("personal", "api-key")
            ], status
            assert all("Unavailable" in item["status"] for item in initial_profiles), status
        else:
            assert initial_profiles == [], status

        method = "api-key" if connection == "api" else "chatgpt-sign-in"
        started = client.call("_phenix/providers/auth/begin", {
            **scope, "method_id": method,
        })
        assert "result" in started, started
        if connection == "api":
            attempt = started["result"]["ApiKeyInput"]["attempt_id"]
        else:
            browser = started["result"]["OpenBrowser"]
            url = browser["url"]
            assert url.startswith("https://auth.openai.com/"), browser
            assert all(marker in url for marker in (
                "state=", "nonce=", "code_challenge=",
            )), browser
            attempt = browser["attempt_id"]
            pending = client.call("_phenix/providers/auth/poll", {
                **scope, "attempt_id": attempt,
            })
            assert pending["result"] == "Pending", pending

        stopped = client.call("_phenix/providers/auth/cancel", {
            **scope, "attempt_id": attempt,
        })
        assert stopped["result"] == "Cancelled", stopped
        replay = client.call("_phenix/providers/auth/poll", {
            **scope, "attempt_id": attempt,
        })
        assert "error" in replay, replay

        final_status = client.call("_phenix/providers/auth/status", scope)
        final_profiles = final_status["result"]["profiles"]
        if connection == "api":
            assert [(item["id"], item["method_id"]) for item in final_profiles] == [
                ("personal", "api-key")
            ], final_status
            assert all("Unavailable" in item["status"] for item in final_profiles), final_status
        else:
            assert final_profiles == [], final_status
        client.close()
    finally:
        client.abort()

(output / "accepted.json").write_text(json.dumps({
    "format": "phenix.p1.auth-control.v1",
    "connection": connection,
    "cases": [
        "provider-registration",
        "unauthenticated-status",
        "challenge-begin",
        "oauth-pending" if connection == "chatgpt" else "api-key-input",
        "attempt-cancellation",
        "cancelled-attempt-replay-denied",
        "no-fake-authenticated-profile",
        "no-model-or-session-invocation",
    ],
    "accepted": True,
}, sort_keys=True, indent=2))
