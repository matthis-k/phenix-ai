"""Exercise ACP v1 over real stdio and two distinct product processes."""
import json
import os
from pathlib import Path
import subprocess
import sys

binary, composition, output_dir = sys.argv[1:]
output = Path(output_dir)
output.mkdir(parents=True, exist_ok=True)
updates = []


def start():
    return subprocess.Popen(
        [binary, "acp", composition],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
        env=os.environ.copy(),
    )


def request(process, number, method, params):
    frame = {
        "jsonrpc": "2.0",
        "id": number,
        "method": method,
        "params": params,
    }
    process.stdin.write(json.dumps(frame) + "\n")
    process.stdin.flush()
    for _ in range(256):
        line = process.stdout.readline()
        assert line, f"ACP process exited before reply to {method}"
        response = json.loads(line)
        assert response["jsonrpc"] == "2.0"
        if response.get("method") == "session/update":
            updates.append(response)
            continue
        assert response["id"] == number, response
        assert "error" not in response, response
        return response["result"]
    raise AssertionError("no ACP response after 256 messages")


def initialize(process):
    result = request(
        process, 1, "initialize",
        {"protocolVersion": 1, "clientCapabilities": {}},
    )
    assert result["protocolVersion"] == 1
    assert result["agentCapabilities"]["loadSession"] is True


def exit_cleanly(process):
    process.stdin.close()
    code = process.wait(timeout=30)
    error = process.stderr.read()
    assert code == 0, (code, error)
    process.stdout.close()
    process.stderr.close()


first = start()
initialize(first)
session = request(first, 2, "session/new", {
    "cwd": "/tmp", "mcpServers": [],
})["sessionId"]
assert session
done = request(first, 3, "session/prompt", {
    "sessionId": session,
    "prompt": [{"type": "text", "text": "first"}],
})
assert done["stopReason"] == "end_turn"
assert any(
    entry["params"]["sessionId"] == session
    and entry["params"]["update"].get("content", {}).get("text") == "mock:first"
    for entry in updates
)
exit_cleanly(first)

# A new provider process reopens the same SQLite file through Storage.
second = start()
initialize(second)
result = request(second, 4, "session/load", {
    "sessionId": session,
    "cwd": "/tmp",
    "mcpServers": [],
})
assert result == {}
assert any(
    entry["params"]["sessionId"] == session
    and entry["params"]["update"].get("content", {}).get("text") == "mock:first"
    for entry in updates
)
continued = request(second, 5, "session/prompt", {
    "sessionId": session,
    "prompt": [{"type": "text", "text": "continued"}],
})
assert continued["stopReason"] == "end_turn"
second_session = request(second, 6, "session/new", {
    "cwd": "/tmp", "mcpServers": [],
})["sessionId"]
assert second_session != session
another = request(second, 7, "session/prompt", {
    "sessionId": second_session,
    "prompt": [{"type": "text", "text": "isolated"}],
})
assert another["stopReason"] == "end_turn"
assert any(
    entry["params"]["sessionId"] == second_session
    and entry["params"]["update"].get("content", {}).get("text") == "mock:isolated"
    for entry in updates
)
exit_cleanly(second)

(output / "acp-product.json").write_text(
    json.dumps({
        "format": "phenix.acp.host.v1",
        "cases": [
            "initialize", "session-new", "prompt", "stream-notifications",
            "process-restart-load", "session-continuation",
            "two-session-isolation",
        ],
        "active_cancellation_verified": False,
    }, indent=2, sort_keys=True)
)
