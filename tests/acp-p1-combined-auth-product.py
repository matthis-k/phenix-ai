"""P1 packaged product: independent API key and ChatGPT OAuth in one ACP process.

This is a deterministic non-login fixture. It proves exact connection routing,
separate backend accounts, a pending/cancellable OAuth browser challenge, and
absence of a fake authenticated model. It does not claim paid-plan inference.
"""
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading

(runtime, basic, api_management, oauth_management, api_credentials,
 api_resolver, oauth_resolver, oauth_logout, destination) = sys.argv[1:]
output = Path(destination)
output.mkdir(parents=True, exist_ok=True)

with tempfile.TemporaryDirectory(prefix="phenix-p1-combined-auth-") as private:
    base = Path(private)
    vault = base / "vault"
    vault.mkdir(mode=0o700)
    environment = dict(
        os.environ,
        PHENIX_CREDENTIAL_FILE_CONSENT="1",
        PHENIX_CREDENTIAL_FILE_DIRECTORY=str(vault),
        PHENIX_BASIC_SQLITE_PATH=str(base / "session.sqlite"),
        PHENIX_MODEL_HTTP_ENDPOINT="http://127.0.0.1:1/not-selected",
        PHENIX_MODEL_HTTP_MODEL="unselected",
    )
    environment.pop("PHENIX_MODEL_HTTP_API_KEY", None)
    environment.pop("PHENIX_CREDENTIAL_API_KEY_ENV_NAME", None)
    argv = [
        runtime, "acp", basic,
        "--provider-management", api_management,
        "--provider-management", oauth_management,
        "--provider-credentials", api_credentials,
        "--provider-credentials", oauth_logout,
        "--provider-model-resolver", api_resolver,
        "--provider-model-resolver", oauth_resolver,
    ]
    child = subprocess.Popen(
        argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, text=True, bufsize=1, env=environment,
    )
    replies = queue.Queue()

    def reader():
        for line in child.stdout:
            replies.put(line)
        replies.put(None)

    threading.Thread(target=reader, daemon=True).start()

    def call(request_id, method, params):
        child.stdin.write(json.dumps({
            "jsonrpc": "2.0", "id": request_id,
            "method": method, "params": params,
        }) + "\n")
        child.stdin.flush()
        for _ in range(40):
            raw = replies.get(timeout=25)
            if raw is None:
                raise AssertionError((method, child.poll(), child.stderr.read()[-1500:]))
            frame = json.loads(raw)
            if frame.get("method") == "session/update":
                continue
            assert frame.get("id") == request_id, frame
            return frame
        raise AssertionError("too many ACP notifications")

    try:
        init = call(1, "initialize", {"protocolVersion": 1, "clientCapabilities": {}})
        assert init["result"]["protocolVersion"] == 1
        catalog = call(2, "_phenix/providers/list", {"scope": "user"})
        assert "result" in catalog, catalog
        matching = [p for p in catalog["result"]["providers"] if p["id"] == "openai"]
        assert len(matching) == 1, matching
        connections = {c["id"]: c for c in matching[0]["connections"]}
        assert set(connections) == {"api", "chatgpt"}, connections
        assert connections["api"]["auth_methods"][0]["kind"] == "ApiKey"
        assert connections["chatgpt"]["auth_methods"][0]["kind"] == "OAuthAuthorizationCode"
        api = {"scope": "user", "provider_id": "openai", "connection_id": "api"}
        oauth = {"scope": "user", "provider_id": "openai", "connection_id": "chatgpt"}
        api_status = call(3, "_phenix/providers/auth/status", api)
        assert api_status["result"]["connection_id"] == "api", api_status
        oauth_status = call(4, "_phenix/providers/auth/status", oauth)
        assert oauth_status["result"]["profiles"] == [], oauth_status
        # These are per-root *granted* operations, not generic credential
        # method names. OAuth is provider-owned; only revoke is exposed.
        capabilities = call(39, "_phenix/credentials/capabilities", {})
        assert "result" in capabilities, capabilities
        profiles = {
            (p["provider_id"], p["connection_id"], p["profile_id"]): p
            for p in capabilities["result"]["profiles"]
        }
        assert set(profiles) == {
            ("openai", "api", "personal"),
            ("openai", "chatgpt", "personal"),
        }, capabilities
        assert profiles[("openai", "api", "personal")]["import"] is True
        assert profiles[("openai", "api", "personal")]["bind_environment"] is True
        assert profiles[("openai", "api", "personal")]["revoke"] is True
        assert profiles[("openai", "chatgpt", "personal")]["import"] is False
        assert profiles[("openai", "chatgpt", "personal")]["bind_environment"] is False
        assert profiles[("openai", "chatgpt", "personal")]["revoke"] is True
        # A logout-only OAuth account root cannot be converted into a key
        # importer or named environment credential by a forged ACP request.
        credential_account = {
            "provider_id": "openai",
            "connection_id": "chatgpt",
            "profile_id": "personal",
        }
        forged = call(40, "_phenix/credentials/import", {
            **credential_account, "secret": "not-an-oauth-token",
        })
        assert forged["error"]["code"] == -32602, forged
        forged_env = call(41, "_phenix/credentials/env-bind", {
            **credential_account, "environment_variable": "OPENAI_API_KEY",
        })
        assert forged_env["error"]["code"] == -32602, forged_env
        # No issued credentials yet: provider sign-out is idempotent and
        # cannot trigger an outbound request or invent authentication.
        missing_logout = call(42, "_phenix/credentials/revoke", credential_account)
        assert missing_logout.get("result", {}).get("ok") is True, missing_logout
        signed_out = call(43, "_phenix/providers/auth/status", oauth)
        assert signed_out["result"]["profiles"] == [], signed_out
        api_begin = call(5, "_phenix/providers/auth/begin", {**api, "method_id": "api-key"})
        api_attempt = api_begin["result"]["ApiKeyInput"]["attempt_id"]
        api_cancel = call(6, "_phenix/providers/auth/cancel", {
            **api, "attempt_id": api_attempt,
        })
        assert api_cancel["result"] == "Cancelled", api_cancel
        oauth_begin = call(7, "_phenix/providers/auth/begin", {
            **oauth, "method_id": "chatgpt-sign-in",
        })
        browser = oauth_begin["result"]["OpenBrowser"]
        assert browser["url"].startswith("https://auth.openai.com/"), browser
        assert all(x in browser["url"] for x in ["state=", "nonce=", "code_challenge="])
        attempt_id = browser["attempt_id"]
        pending = call(8, "_phenix/providers/auth/poll", {**oauth, "attempt_id": attempt_id})
        assert pending["result"] == "Pending", pending
        wrong = call(9, "_phenix/providers/auth/poll", {**api, "attempt_id": attempt_id})
        assert "error" in wrong, wrong
        cancelled = call(10, "_phenix/providers/auth/cancel", {
            **oauth, "attempt_id": attempt_id,
        })
        assert cancelled["result"] == "Cancelled", cancelled
        replay = call(11, "_phenix/providers/auth/poll", {
            **oauth, "attempt_id": attempt_id,
        })
        assert "error" in replay, replay
        session = call(12, "session/new", {"cwd": "/tmp", "mcpServers": []})
        assert "result" in session, session
        options = session["result"].get("configOptions", [])
        assert all("phenix.native-model@1" not in json.dumps(opt) for opt in options)
        child.stdin.close()
        code = child.wait(timeout=20)
        error = child.stderr.read()
        assert code == 0, error[-1200:]
    finally:
        if child.poll() is None:
            child.kill()
            child.wait(timeout=10)
        child.stdin.close() if not child.stdin.closed else None
        child.stdout.close()
        child.stderr.close()

(output / "p1-combined-auth.json").write_text(json.dumps({
    "installed_connections": ["openai/api", "openai/chatgpt"],
    "api_key_challenge_cancelled": True,
    "oauth_browser_pkce_challenge_cancelled": True,
    "cross_connection_attempt_denied": True,
    "oauth_only_provider_owned_logout_granted": True,
    "oauth_import_and_api_env_binding_denied": True,
    "unauthenticated_native_model_denied": True,
    "live_external_login_verified": False,
}, indent=2, sort_keys=True))
print("P1 combined API-key and ChatGPT OAuth ACP product verified")
