"""ACP frontend ModelRequest projection, independent of JSONL, auth, and history tests."""
import json
import os
from pathlib import Path
import subprocess
import sys

binary, destination = sys.argv[1:]
output = Path(destination)
output.mkdir(parents=True, exist_ok=True)
env = dict(os.environ)
for key in ("OPENAI_API_KEY", "ANTHROPIC_API_KEY", "OPENROUTER_API_KEY"):
    env.pop(key, None)
env["PHENIX_BASIC_SQLITE_PATH"] = str(output / "acp-capabilities.sqlite")
process = subprocess.Popen(
    [binary, "acp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
    stderr=subprocess.PIPE, text=True, bufsize=1, env=env,
)
updates = []


def rpc(number, method, params):
    process.stdin.write(json.dumps({
        "jsonrpc": "2.0", "id": number, "method": method, "params": params,
    }) + "\n")
    process.stdin.flush()
    for _ in range(128):
        raw = process.stdout.readline()
        assert raw, (method, process.poll())
        frame = json.loads(raw)
        if frame.get("method") == "session/update":
            updates.append(frame["params"])
            continue
        assert frame.get("id") == number, (method, frame)
        assert "error" not in frame, (method, frame)
        return frame["result"]
    raise AssertionError(("too many ACP notifications", method))


try:
    init = rpc(1, "initialize", {
        "protocolVersion": 1, "clientCapabilities": {},
    })
    assert init["protocolVersion"] == 1, init
    session = rpc(2, "session/new", {
        "cwd": str(output), "mcpServers": [],
    })["sessionId"]
    prompt_result = rpc(3, "session/prompt", {
        "sessionId": session,
        "prompt": [{"type": "text", "text": "inspect:capabilities"}],
    })
    assert prompt_result["stopReason"] == "end_turn", prompt_result
    chunks = [
        event["update"]["content"]["text"]
        for event in updates
        if event["sessionId"] == session
        and event["update"].get("sessionUpdate") == "agent_message_chunk"
    ]
    observed = json.loads("".join(chunks))
    assert observed["format"] == "phenix.test.model-capabilities.v1"
    assert [tool["id"] for tool in observed["tools"]] == [
        "fail", "length", "uppercase"
    ], observed
    assert all(isinstance(json.loads(tool["arguments_schema"]), dict)
               for tool in observed["tools"]), observed
    assert isinstance(observed["skills"], list)
    assert isinstance(observed["context"], list)
finally:
    process.stdin.close()
    code = process.wait(timeout=30)
    stderr = process.stderr.read()
    assert code == 0, (code, stderr[-1500:])
    process.stdout.close()
    process.stderr.close()

(output / "accepted.json").write_text(json.dumps({
    "format": "phenix.test.acp-model-capabilities.v1",
    "cases": [
        "acp-initialize-and-session",
        "acp-model-request-tool-projection",
        "acp-schema-metadata",
        "credential-free-acp-prompt",
    ],
    "accepted": True,
}, sort_keys=True, indent=2))
