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

# A malformed idle frame must get a JSON-RPC error, not terminate ACP.
first.stdin.write("{bad-json\n")
first.stdin.flush()
parse_error = json.loads(first.stdout.readline())
assert parse_error["error"]["code"] == -32700, parse_error
assert parse_error["id"] is None

# Invalid JSON-RPC framing and missing/zero protocol version cannot
# permanently consume the initialize slot.
first.stdin.write(json.dumps({
    "jsonrpc": "1.0", "id": 91, "method": "initialize",
    "params": {"protocolVersion": 1, "clientCapabilities": {}},
}) + "\n")
first.stdin.flush()
invalid_request = json.loads(first.stdout.readline())
assert invalid_request["error"]["code"] == -32600, invalid_request

first.stdin.write(json.dumps({
    "jsonrpc": "2.0", "id": 92, "method": "initialize",
    "params": {"protocolVersion": 0, "clientCapabilities": {}},
}) + "\n")
first.stdin.flush()
invalid_version = json.loads(first.stdout.readline())
assert invalid_version["id"] == 92, invalid_version
assert invalid_version["error"]["code"] == -32602, invalid_version

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

# A v1-only agent negotiates down to protocol version 1 when the
# client advertises a newer version. The client may continue at v1.
version_probe = start()
downgraded = request(version_probe, 90, "initialize", {
    "protocolVersion": 2,
    "clientCapabilities": {},
})
assert downgraded["protocolVersion"] == 1, downgraded
assert request(version_probe, 91, "session/new", {
    "cwd": "/tmp", "mcpServers": [],
})["sessionId"]
exit_cleanly(version_probe)

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
            "malformed-frame-recovers", "invalid-jsonrpc-rejected",
            "invalid-protocol-version-rejected",
            "newer-client-negotiates-down-to-v1",
        ],
        "active_cancellation_verified": False,
    }, indent=2, sort_keys=True)
)
