"""Provider catalog/discovery admission without auth or model invocation.

The fixture is a native Provider implementation used behind ordinary ACP
provider management. Its endpoint is an unreachable loopback sentinel;
this gate must make zero HTTP requests to it.
"""
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import threading

runtime, basic, management, destination = sys.argv[1:]
out = Path(destination)
out.mkdir(parents=True, exist_ok=True)


def probe(*, revoked):
    env = dict(os.environ)
    for name in ("OPENAI_API_KEY", "ANTHROPIC_API_KEY",
                 "OPENROUTER_API_KEY", "PHENIX_MODEL_HTTP_API_KEY"):
        env.pop(name, None)
    env.update(
        PHENIX_BASIC_SQLITE_PATH=str(out / ("revoked.sqlite" if revoked else "catalog.sqlite")),
        PHENIX_C9_TEST_HTTP_ENDPOINT="http://127.0.0.1:1/completions",
        PHENIX_C9_TEST_REVOKED="1" if revoked else "0",
        PHENIX_MODEL_HTTP_ENDPOINT="http://127.0.0.1:1/not-executable",
    )
    child = subprocess.Popen(
        [runtime, "acp", basic, "--provider-management", management],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, bufsize=1, env=env,
    )
    frames = queue.Queue()

    def reader():
        for line in child.stdout:
            frames.put(line)
        frames.put(None)

    threading.Thread(target=reader, daemon=True).start()
    serial = 0

    def call(method, params):
        nonlocal serial
        assert method.startswith("_phenix/providers/"), method
        serial += 1
        child.stdin.write(json.dumps({
            "jsonrpc": "2.0", "id": serial, "method": method,
            "params": params,
        }) + "\n")
        child.stdin.flush()
        for _ in range(64):
            try:
                raw = frames.get(timeout=25)
            except queue.Empty as cause:
                raise AssertionError("ACP provider query timeout") from cause
            assert raw is not None, (method, child.poll())
            frame = json.loads(raw)
            assert frame.get("id") == serial, frame
            return frame
        raise AssertionError("too many ACP responses")

    try:
        child.stdin.write(json.dumps({
            "jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {"protocolVersion": 1, "clientCapabilities": {}},
        }) + "\n")
        child.stdin.flush()
        first = json.loads(frames.get(timeout=25))
        assert first.get("result", {}).get("protocolVersion") == 1, first

        page = call("_phenix/providers/list", {"scope": "user"})
        assert "result" in page, page
        assert [x["id"] for x in page["result"]["providers"]] == [
            "c9-test-loopback"
        ], page
        model_query = {
            "scope": "user", "provider_id": "c9-test-loopback",
            "connection_id": "local", "profile_id": None,
            "cursor": None, "limit": 20,
        }
        models = call("_phenix/providers/models", model_query)
        if revoked:
            assert models["error"]["data"]["code"] == "Denied", models
        else:
            assert "result" in models, models
            page = models["result"]
            assert page["revision"] == 7 and page["next_cursor"] is None, page
            available = page["models"]
            assert [x["id"] for x in available] == ["fixture-a", "fixture-b"], available
            assert all(
                x["connection_id"] == "local"
                and x["protocol_id"] == "openai-chat"
                and x["selectable"] is True
                and not x["id"].startswith("phenix.")
                for x in available
            ), available

            invalid = call("_phenix/providers/models", {
                **model_query, "profile_id": "forged",
            })
            assert invalid["error"]["data"]["code"] == "InvalidInput", invalid
            wrong_scope = call("_phenix/providers/models", {
                **model_query, "scope": "other",
            })
            assert "error" in wrong_scope, wrong_scope
        return True
    finally:
        child.stdin.close()
        code = child.wait(timeout=30)
        stderr = child.stderr.read()
        child.stdout.close()
        child.stderr.close()
        assert code == 0, (code, stderr[-1400:])


assert probe(revoked=False)
assert probe(revoked=True)
(out / "accepted.json").write_text(json.dumps({
    "format": "phenix.p1.model-discovery.v1",
    "cases": [
        "native-provider-registration",
        "two-concrete-models",
        "model-interface-is-not-selectable-id",
        "no-credentials-or-model-execution",
        "foreign-profile-denied",
        "foreign-provider-scope-denied",
        "revocation-denies-catalog",
    ],
    "accepted": True,
}, sort_keys=True, indent=2))
