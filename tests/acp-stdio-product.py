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


def request_error(process, number, method, params, code):
    frame = {
        "jsonrpc": "2.0",
        "id": number,
        "method": method,
        "params": params,
    }
    process.stdin.write(json.dumps(frame) + "\n")
    process.stdin.flush()
    response = json.loads(process.stdout.readline())
    assert response["id"] == number, response
    assert response["error"]["code"] == code, response


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


def assert_model_options(response):
    options = response["configOptions"]
    assert len(options) == 1, options
    model = options[0]
    assert model["id"] == "model", model
    assert model["category"] == "model", model
    assert model["type"] == "select", model
    assert model["currentValue"] == "phenix.model.scripted", model
    assert model["options"] == [{
        "value": "phenix.model.scripted",
        "name": "phenix.model.scripted",
    }], model


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
# The SDK rejects pre-release v0 during protocol compatibility admission,
# before the Phenix InitializeRequest handler can return invalid params.
assert invalid_version["error"]["code"] == -32600, invalid_version

# A notification cannot initialize the connection or admit a new session.
first.stdin.write(json.dumps({
    "jsonrpc": "2.0", "method": "initialize",
    "params": {"protocolVersion": 1, "clientCapabilities": {}},
}) + "\n")
first.stdin.flush()

# A Boolean request ID is neither a string nor a JSON-RPC numeric ID.
first.stdin.write(json.dumps({
    "jsonrpc": "2.0", "id": True, "method": "initialize",
    "params": {"protocolVersion": 1, "clientCapabilities": {}},
}) + "\n")
first.stdin.flush()
bad_id = json.loads(first.stdout.readline())
assert bad_id["error"]["code"] == -32600, bad_id
assert bad_id["id"] is None, bad_id

initialize(first)
new_session = request(first, 2, "session/new", {
    "cwd": "/tmp", "mcpServers": [],
})
assert_model_options(new_session)
session = new_session["sessionId"]
assert session

# ACP v1 model settings use the standard session configuration method.
# The only installed binding may be reselected; an uninstalled provider
# must fail without altering the running graph or journal.
assert_model_options(request(first, 105, "session/set_config_option", {
    "sessionId": session, "configId": "model", "value": "phenix.model.scripted",
}))
for number, params in enumerate([
    {"sessionId": session, "configId": "model", "value": "phenix.model.http"},
    {"sessionId": session, "configId": "unknown", "value": "phenix.model.scripted"},
    {"sessionId": "unknown", "configId": "model", "value": "phenix.model.scripted"},
], start=106):
    request_error(first, number, "session/set_config_option", params, -32602)
# Invalid prompt content is a client parameter error. These cases must
# leave the session idle, with no output or Model admission.
for number, params in enumerate([
    {"sessionId": session},
    {"sessionId": session, "prompt": []},
    {"sessionId": session, "prompt": [{"type": "image", "data": "unsupported"}]},
    {"sessionId": session, "prompt": [{"type": "text", "text": 42}]},
], start=110):
    previous_updates = list(updates)
    request_error(first, number, "session/prompt", params, -32602)
    assert updates == previous_updates, "invalid prompt caused session updates"
# An arbitrary well-formed ID is not a persisted session. Reject it
# without replacing or corrupting the client's existing live session.
request_error(first, 101, "session/load", {
    "sessionId": "s_nonexistent", "cwd": "/tmp", "mcpServers": [],
}, -32602)
# The ID returned by session/new is still live even before its first
# durable checkpoint. Reloading it in the same connection remains valid.
assert_model_options(request(first, 102, "session/load", {
    "sessionId": session, "cwd": "/tmp", "mcpServers": [],
}))
done = request(first, 3, "session/prompt", {
    "sessionId": session,
    "prompt": [{"type": "text", "text": "first"}],
})
assert done["stopReason"] == "end_turn"
assert any(
    entry["params"]["sessionId"] == session
    and entry["params"]["update"].get("sessionUpdate") == "user_message_chunk"
    and entry["params"]["update"].get("content", {}).get("text") == "first"
    for entry in updates
), "ACP must project accepted user input from the durable journal"
assert any(
    entry["params"]["sessionId"] == session
    and entry["params"]["update"].get("content", {}).get("text") == "mock:first"
    for entry in updates
)
# Tool events must preserve the call identity and order through the
# EventJournal and the ACP frontend, not merely appear as extra lines.
start_updates = len(updates)
tools = request(first, 8, "session/prompt", {
    "sessionId": session,
    "prompt": [{"type": "text", "text": "tools:hello"}],
})
assert tools["stopReason"] == "end_turn", tools
tool_updates = [
    entry["params"]["update"]
    for entry in updates[start_updates:]
    if entry["params"]["sessionId"] == session
]
started = [
    (i, update) for i, update in enumerate(tool_updates)
    if update.get("sessionUpdate") == "tool_call"
]
finished = [
    (i, update) for i, update in enumerate(tool_updates)
    if update.get("sessionUpdate") == "tool_call_update"
]
assert [entry["toolCallId"] for _, entry in started] == ["call-1", "call-2"], started
assert [entry["toolCallId"] for _, entry in finished] == ["call-1", "call-2"], finished
for call_id in ("call-1", "call-2"):
    assert next(i for i, x in started if x["toolCallId"] == call_id) < next(
        i for i, x in finished if x["toolCallId"] == call_id
    )
# The official v1 schema omits Pending because it is the default status.
assert all(entry.get("status", "pending") == "pending" for _, entry in started)
assert all(entry["status"] == "completed" for _, entry in finished)
assert "".join(
    event.get("content", {}).get("text", "")
    for event in tool_updates
    if event.get("sessionUpdate") == "agent_message_chunk"
) == "mock:call-1:HELLO|call-2:5", tool_updates
# An invalid session identity is a client parameter error, not a host
# failure. Reject a forged well-formed ID after durable work without
# replacing the current kernel, losing its journal or admitting a new run.
original_updates = list(updates)
request_error(first, 103, "session/load", {
    "sessionId": "../../escape", "cwd": "/tmp", "mcpServers": [],
}, -32602)
request_error(first, 104, "session/load", {
    "sessionId": "s_nonexistent", "cwd": "/tmp", "mcpServers": [],
}, -32602)
assert updates == original_updates, "invalid session loads emitted output"
assert_model_options(request(first, 105, "session/load", {
    "sessionId": session, "cwd": "/tmp", "mcpServers": [],
}))
assert updates[len(original_updates):] == original_updates, (
    "live session journal did not survive refused load"
)
exit_cleanly(first)

# A v1-only official SDK Agent.builder() endpoint rejects the draft-v2
# wire version, and the rejected initialize must not consume its slot.
# Dual-version selection belongs to the separate SDK v1/v2 router.
version_probe = start()
request_error(version_probe, 90, "initialize", {
    "protocolVersion": 2,
    "clientCapabilities": {},
}, -32600)
initialize(version_probe)
resource_session = request(version_probe, 91, "session/new", {
    "cwd": "/tmp", "mcpServers": [],
})["sessionId"]
# V1 resource_link is mandatory, but Basic never dereferences its URI.
# Preserve the reference for the Model/Tool layer rather than adding
# filesystem operations to the Frontend or kernel.
before_resource = len(updates)
resource_reply = request(version_probe, 92, "session/prompt", {
    "sessionId": resource_session,
    "prompt": [
        {"type": "text", "text": "reference"},
        {"type": "resource_link", "name": "notes", "uri": "file:///tmp/notes.md"},
    ],
})
assert resource_reply["stopReason"] == "end_turn", resource_reply
assert any(
    u["params"]["sessionId"] == resource_session
    and u["params"]["update"].get("content", {}).get("text")
        == "mock:reference\nResource link: notes (file:///tmp/notes.md)"
    for u in updates[before_resource:]
), "required v1 resource link was dropped"
exit_cleanly(version_probe)

# An explicit null ID is a request, not a notification. Preserve the
# supplied ID in its reply without suppressing protocol initialization.
null_id_probe = start()
null_id_probe.stdin.write(json.dumps({
    "jsonrpc": "2.0", "id": None, "method": "initialize",
    "params": {"protocolVersion": 1, "clientCapabilities": {}},
}) + "\n")
null_id_probe.stdin.flush()
null_id_reply = json.loads(null_id_probe.stdout.readline())
assert null_id_reply["id"] is None, null_id_reply
assert null_id_reply["result"]["protocolVersion"] == 1, null_id_reply
exit_cleanly(null_id_probe)

# A new provider process reopens the same SQLite file through Storage.
second = start()
initialize(second)
result = request(second, 4, "session/load", {
    "sessionId": session,
    "cwd": "/tmp",
    "mcpServers": [],
})
assert_model_options(result)
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

# One input frame without a newline can otherwise grow beyond the bounded
# reader channel's 64 messages. The host rejects it before JSON decoding,
# with a bounded allocation and no active graph or Model dispatch.
oversized = start()
stdout, stderr = oversized.communicate("x" * (1024 * 1024 + 8) + "\n", timeout=30)
assert oversized.returncode != 0, stdout
assert "stdin frame exceeds 1 MiB" in stderr, stderr

(output / "acp-product.json").write_text(
    json.dumps({
        "format": "phenix.acp.host.v1",
        "cases": [
            "initialize", "session-new", "prompt", "stream-notifications",
            "composition-model-catalog-advertised-on-new-and-load",
            "accepted-user-prompts-replay-as-acp-user-message-chunks",
            "process-restart-load", "session-continuation",
            "two-session-isolation",
            "malformed-frame-recovers", "invalid-jsonrpc-rejected",
            "invalid-protocol-version-rejected",
            "draft-v2-rejected-by-stable-v1-only-sdk",
            "v1-retry-after-rejected-v2-initialize",
            "v1-resource-link-preserved-without-dereference",
            "notifications-do-not-initialize",
            "invalid-request-id-rejected",
            "explicit-null-request-id-replied",
            "two-tool-acp-lifecycle-ordered",
            "tool-call-identity-preserved",
            "tool-result-and-final-text-streamed",
            "unknown-durable-session-rejected",
            "live-uncheckpointed-session-reload-allowed",
            "invalid-session-identity-rejected",
            "failed-load-preserves-durable-live-journal",
            "oversized-stdio-frame-rejected",
            "invalid-prompt-params-rejected-before-admission",
        ],
        "active_cancellation_verified": False,
    }, indent=2, sort_keys=True)
)
