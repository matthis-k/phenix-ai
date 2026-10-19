"""Read-only R-N4a via the official ACP SDK and an independent native kernel grant."""
import json
import os
from pathlib import Path
import subprocess
import sys

runtime, basic, management, destination = sys.argv[1:]
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
    assert all("secret" not in json.dumps(p).lower() for p in providers.values())

    # No ACP-supplied scope or operation can widen this launcher-owned grant.
    for number, params in enumerate([dict(scope="other"),
                                      dict(scope="user", token="forged")], start=3):
        rejection = call(number, "_phenix/providers/list", params)
        assert rejection["error"]["code"] == -32602, rejection
    unknown = call(5, "_phenix/providers/write", query)
    assert unknown["error"]["code"] == -32601, unknown

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
(output / "acp-provider-management.json").write_text(json.dumps(dict(
    format="phenix.acp.provider-management.product.v1",
    read_only=True, managed=True, unmodified_basic_denied=True,
    scope_denied=True, unknown_mutation_denied=True,
    simultaneous_basic_session=True, **report,
), sort_keys=True, indent=2))
print("Authorized read-only provider catalog via official ACP SDK verified")
