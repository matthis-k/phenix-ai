"""Read-only R-N4a via the official ACP SDK and an independent native kernel grant."""
import json
import os
from pathlib import Path
import subprocess
import sys

runtime, basic, management, http_basic, destination = sys.argv[1:]
output = Path(destination)
output.mkdir(parents=True, exist_ok=True)


def probe(managed: bool):
    argv = [runtime, "acp", basic]
    if managed:
        argv.extend(["--provider-management", management])
    child = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE, text=True, bufsize=1, env=os.environ.copy())

    def call(request_id, method, params):
        child.stdin.write(json.dumps(dict(jsonrpc="2.0", id=request_id,
                                          method=method, params=params)) + "\n")
        child.stdin.flush()
        for _ in range(20):
            raw = child.stdout.readline()
            assert raw, (method, child.poll(), child.stderr.read())
            frame = json.loads(raw)
            if frame.get("method") == "session/update":
                continue
            assert frame.get("id") == request_id, frame
            return frame
        raise AssertionError("too many ACP notifications")

    initialization = call(1, "initialize",
                          dict(protocolVersion=1, clientCapabilities={}))
    assert initialization["result"]["protocolVersion"] == 1

    query = dict(scope="user")
    result = call(2, "_phenix/providers/list", query)
    if not managed:
        assert result["error"]["code"] == -32601, result
        unavailable = call(7, "_phenix/providers/auth/status", dict(
            scope="user", provider_id="openai", connection_id="api"))
        assert unavailable["error"]["code"] == -32601
        unavailable = call(8, "_phenix/providers/models", dict(
            scope="user", provider_id="openai", connection_id="api",
            profile_id=None, cursor=None, limit=20))
        assert unavailable["error"]["code"] == -32601
        for n, (method, fields) in enumerate([
            ("begin", {"method_id": "api-key"}),
            ("poll", {"attempt_id": "unknown"}),
            ("cancel", {"attempt_id": "unknown"}),
        ], start=30):
            attempt = call(n, "_phenix/providers/auth/" + method, {
                "scope": "user", "provider_id": "openai",
                "connection_id": "api", **fields,
            })
            assert attempt["error"]["code"] == -32601, attempt
        child.stdin.close()
        assert child.wait(timeout=15) == 0
        child.stdout.close()
        child.stderr.close()
        return

    page = result["result"]
    assert page["revision"] > 0, page
    providers = {p["id"]: p for p in page["providers"]}
    assert set(providers) == {
        "openai", "openrouter", "anthropic", "opencode-zen", "opencode-go", "zai"
    }, providers
    openai = {x["id"]: x for x in providers["openai"]["connections"]}
    assert set(openai) == {"api", "chatgpt"}, openai
    assert openai["api"]["auth_methods"][0]["kind"] == "ApiKey"
    assert openai["chatgpt"]["auth_methods"][0]["kind"] == "OAuthAuthorizationCode"
    assert openai["api"]["auth_methods"][0]["adapter"]["id"] == "api-key"
    assert openai["chatgpt"]["auth_methods"][0]["adapter"]["id"] == "chatgpt.oauth"
    assert openai["api"]["model_sources"][0]["adapter"]["id"] == "openai.models"
    assert openai["chatgpt"]["model_sources"][0]["adapter"]["id"] == "chatgpt.models"
    assert [entry["id"] for entry in openai["api"]["protocols"]] == [
        "openai-responses", "openai-chat",
    ]
    assert [entry["id"] for entry in openai["chatgpt"]["protocols"]] == [
        "openai-responses",
    ]
    # ACP serializes the typed provider response through Serde (Option<String>
    # becomes a scalar); the native kernel Portable wire instead uses a
    # one-element array for Some(endpoint), as tested separately.
    # The installed provider plugin and ACP query preserve exact, documented
    # targets. They are metadata only, not a ready account or inference grant.
    assert {p["id"]: p["endpoint"] for p in openai["api"]["protocols"]} == {
        "openai-responses": "https://api.openai.com/v1/responses",
        "openai-chat": "https://api.openai.com/v1/chat/completions",
    }
    assert openai["chatgpt"]["protocols"][0]["endpoint"] == "https://api.openai.com/v1/responses"
    router = providers["openrouter"]["connections"][0]
    assert {p["id"]: p["endpoint"] for p in router["protocols"]} == {
        "openai-chat": "https://openrouter.ai/api/v1/chat/completions",
        "openai-responses": "https://openrouter.ai/api/v1/responses",
    }
    assert providers["anthropic"]["connections"][0]["protocols"][0]["endpoint"] == (
        "https://api.anthropic.com/v1/messages"
    )
    assert all("secret" not in json.dumps(p).lower() for p in providers.values())

    # No ACP-supplied scope or operation can widen this launcher-owned grant.
    for number, params in enumerate([dict(scope="other"),
                                      dict(scope="user", token="forged")], start=3):
        rejection = call(number, "_phenix/providers/list", params)
        assert rejection["error"]["code"] == -32602, rejection
    unknown = call(5, "_phenix/providers/write", query)
    assert unknown["error"]["code"] == -32601, unknown

    status = call(7, "_phenix/providers/auth/status", dict(
        scope="user", provider_id="openai", connection_id="api"))
    assert status["result"] == {
        "provider_id": "openai", "connection_id": "api", "profiles": [],
    }, status
    # Credential storage or provider registration alone cannot claim readiness.
    subscription = call(8, "_phenix/providers/auth/status", dict(
        scope="user", provider_id="openai", connection_id="chatgpt"))
    assert subscription["result"]["profiles"] == []

    # Actual model discovery is not installed: no invented models from
    # provider metadata, even if a caller supplies a forged profile.
    models = call(9, "_phenix/providers/models", dict(
        scope="user", provider_id="openai", connection_id="api",
        profile_id="forged", cursor=None, limit=20))
    assert models["error"]["data"]["code"] == "AuthenticationRequired", models
    wrong_scope = call(10, "_phenix/providers/auth/status", dict(
        scope="foreign", provider_id="openai", connection_id="api"))
    assert wrong_scope["error"]["code"] == -32602
    invalid = call(11, "_phenix/providers/models", dict(
        scope="user", provider_id="openai", connection_id="api",
        profile_id=None, cursor=None, limit=1000))
    assert invalid["error"]["code"] == -32602
    missing = call(12, "_phenix/providers/auth/status", dict(
        scope="user", provider_id="openai", connection_id="missing"))
    assert missing["error"]["data"]["code"] == "NotFound", missing
    # The contract's auth lifecycle is now routed by the ACP SDK. The
    # shipping builtins remain registration-only: no prompt or successful
    # login may be invented before an authorized backend is installed.
    start = call(13, "_phenix/providers/auth/begin", dict(
        scope="user", provider_id="openai", connection_id="api", method_id="api-key"))
    assert start["error"]["data"]["code"] == "Unavailable", start
    wrong_method = call(14, "_phenix/providers/auth/begin", dict(
        scope="user", provider_id="openai", connection_id="api", method_id="forged"))
    assert wrong_method["error"]["data"]["code"] == "InvalidInput", wrong_method
    for n, op in enumerate(["poll", "cancel"], start=15):
        unknown = call(n, "_phenix/providers/auth/" + op, dict(
            scope="user", provider_id="openai", connection_id="api",
            attempt_id="not-issued"))
        assert unknown["error"]["data"]["code"] == "NotFound", unknown
        missing = call(n + 3, "_phenix/providers/auth/" + op, dict(
            scope="user", provider_id="openai", connection_id="api",
            attempt_id=""))
        assert missing["error"]["code"] == -32602, missing
    forged_scope = call(21, "_phenix/providers/auth/begin", dict(
        scope="another", provider_id="openai", connection_id="api", method_id="api-key"))
    assert forged_scope["error"]["code"] == -32602, forged_scope

    new_session = call(6, "session/new", dict(cwd="/tmp", mcpServers=[]))
    assert "sessionId" in new_session["result"], new_session
    assert new_session["result"]["configOptions"][0]["currentValue"] == "phenix.model.scripted"

    child.stdin.close()
    exit_code = child.wait(timeout=15)
    stderr = child.stderr.read()
    assert exit_code == 0, (exit_code, stderr[:300])
    child.stdout.close()
    child.stderr.close()
    return dict(providers=sorted(providers), revision=page["revision"])


def probe_undiscovered_http_choices():
    """An opt-in HTTP composition must not invent a selectable provider model.

    Registration is not authentication or discovery. The only valid model
    selection is the already-installed static HTTP binding until the plugin
    contributes executable, authenticated choices.
    """
    child = subprocess.Popen(
        [runtime, "acp", http_basic, "--provider-management", management],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, bufsize=1, env=os.environ.copy(),
    )

    def request(number, method, params):
        child.stdin.write(json.dumps(dict(
            jsonrpc="2.0", id=number, method=method, params=params
        )) + "\n")
        child.stdin.flush()
        for _ in range(20):
            line = child.stdout.readline()
            assert line, (method, child.poll(), child.stderr.read())
            frame = json.loads(line)
            if frame.get("method") == "session/update":
                continue
            assert frame.get("id") == number, frame
            return frame
        raise AssertionError("unexpected ACP notification loop")

    initialized = request(1, "initialize", dict(
        protocolVersion=1, clientCapabilities={}
    ))
    assert initialized["result"]["protocolVersion"] == 1
    created = request(2, "session/new", dict(cwd="/tmp", mcpServers=[]))
    assert "result" in created, created
    session = created["result"]["sessionId"]
    options = created["result"]["configOptions"]
    assert len(options) == 1 and options[0]["id"] == "model", options
    assert options[0]["currentValue"] == "phenix.model.http", options
    assert options[0]["options"] == [{
        "name": "phenix.model.http", "value": "phenix.model.http"
    }], options
    repeat = request(3, "session/set_config_option", {
        "sessionId": session, "configId": "model", "value": "phenix.model.http"
    })
    assert repeat["result"]["configOptions"] == options, repeat
    forged = request(4, "session/set_config_option", {
        "sessionId": session, "configId": "model",
        "value": 'phenix.native-model@1:{"provider_id":"forged"}'
    })
    assert forged["error"]["code"] == -32602, forged
    child.stdin.close()
    result = child.wait(timeout=20)
    stderr = child.stderr.read()
    child.stdout.close()
    child.stderr.close()
    assert result == 0, (result, stderr[:500])
    return True


# The installed ACP management process cannot be pointed at a different
# contract/operation by an untrusted launcher option. The controller admits
# only this one read-only typed provider catalog entry.
invalid = json.loads(Path(management).read_text())
invalid["entry"]["operation"] = "begin"
invalid_path = output / "invalid-provider-management.json"
invalid_path.write_text(json.dumps(invalid))
denied = subprocess.run(
    [runtime, "acp", basic, "--provider-management", str(invalid_path)],
    capture_output=True, text=True, timeout=15, env=os.environ.copy(),
)
assert denied.returncode != 0, denied
assert "must select provider catalog list" in denied.stderr, denied.stderr

probe(False)
report = probe(True)
undiscovered_native_choices_denied = probe_undiscovered_http_choices()
(output / "acp-provider-management.json").write_text(json.dumps(dict(
    format="phenix.acp.provider-management.product.v1",
    read_only=True, managed=True, unmodified_basic_denied=True,
    scope_denied=True, unknown_mutation_denied=True,
    simultaneous_basic_session=True, auth_status=True,
    missing_discovery_not_selectable=True, scoped_read_only_model_query=True,
    no_auth_begin=True,
    undiscovered_native_choices_denied=undiscovered_native_choices_denied,
    **report,
), sort_keys=True, indent=2))
print("Authorized read-only provider catalog via official ACP SDK verified")
