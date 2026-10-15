"""Deterministic common ACP interactions using the packaged verification preset."""
import json
import os
from pathlib import Path
import subprocess
import sys

binary, destination = sys.argv[1:]
output = Path(destination)
output.mkdir(parents=True, exist_ok=True)
env = dict(os.environ)
env["PHENIX_BASIC_SQLITE_PATH"] = str(output / "acp-history-verification.sqlite")


def start():
    return subprocess.Popen(
        [binary, "acp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, text=True, bufsize=1, env=env,
    )


updates = []


def request(proc, number, method, params, error_code=None):
    proc.stdin.write(json.dumps({
        "jsonrpc": "2.0", "id": number, "method": method, "params": params,
    }) + "\n")
    proc.stdin.flush()
    for _ in range(256):
        line = proc.stdout.readline()
        assert line, (method, proc.poll())
        frame = json.loads(line)
        if frame.get("method") == "session/update":
            updates.append(frame["params"])
            continue
        assert frame.get("id") == number, frame
        if error_code is not None:
            assert frame["error"]["code"] == error_code, frame
            return frame["error"]
        assert "error" not in frame, frame
        return frame["result"]
    raise AssertionError(("no ACP response", method))


def initialize(proc):
    init = request(proc, 1, "initialize", {
        "protocolVersion": 1, "clientCapabilities": {},
    })
    assert init["protocolVersion"] == 1, init
    assert init["agentCapabilities"]["loadSession"] is True, init


def new_session(proc, number):
    return request(proc, number, "session/new", {
        "cwd": "/tmp", "mcpServers": [],
    })["sessionId"]


def prompt(proc, number, session, text):
    offset = len(updates)
    result = request(proc, number, "session/prompt", {
        "sessionId": session,
        "prompt": [{"type": "text", "text": text}],
    })
    assert result["stopReason"] == "end_turn", result
    events = [item["update"] for item in updates[offset:]
              if item["sessionId"] == session]
    reply = "".join(item.get("content", {}).get("text", "")
                    for item in events
                    if item.get("sessionUpdate") == "agent_message_chunk")
    return reply, events


def close(proc):
    proc.stdin.close()
    code = proc.wait(timeout=30)
    stderr = proc.stderr.read()
    assert code == 0, (code, stderr)
    proc.stdout.close()
    proc.stderr.close()


first = start()
initialize(first)
s1 = new_session(first, 2)
assert prompt(first, 3, s1, "echo:acp")[0] == "echo:acp"
assert prompt(first, 4, s1, "remember:language=Rust")[0] == "stored:language=Rust"
assert prompt(first, 5, s1, "verify-assistant:language")[0] == "verified:language=Rust"
assert prompt(first, 6, s1, "recall:language")[0] == "recalled:language=Rust"

reply, events = prompt(first, 7, s1, "tools:hello")
assert reply == "tool-result:HELLO", (reply, events)
starts = [event for event in events if event.get("sessionUpdate") == "tool_call"]
ends = [event for event in events if event.get("sessionUpdate") == "tool_call_update"]
assert [event["toolCallId"] for event in starts] == ["history-tool-1"], events
assert [event["toolCallId"] for event in ends] == ["history-tool-1"], events
assert ends[0]["status"] == "completed", ends
assert prompt(first, 8, s1, "recall-tool")[0] == "tool-recalled:HELLO"

s2 = new_session(first, 9)
assert s2 != s1
assert prompt(first, 10, s2, "recall:language")[0] == "absent:language"
assert prompt(first, 11, s2, "remember:language=Python")[0] == "stored:language=Python"
assert prompt(first, 12, s1, "recall:language")[0] == "recalled:language=Rust"

# The text-only Basic frontend must reject unsupported content before
# admitting a Model invocation; subsequent valid prompts still work.
request(first, 13, "session/prompt", {
    "sessionId": s1,
    "prompt": [{"type": "image", "data": "not-supported"}],
}, error_code=-32602)
assert prompt(first, 14, s1, "echo:after-invalid")[0] == "echo:after-invalid"
close(first)

# Reload from SQLite in a new host and provider process, proving this cannot
# be simulated by a stateful mock model hidden inside the original process.
second = start()
initialize(second)
assert request(second, 15, "session/load", {
    "sessionId": s1, "cwd": "/tmp", "mcpServers": [],
}) == {}
assert prompt(second, 16, s1, "recall:language")[0] == "recalled:language=Rust"
assert prompt(second, 17, s1, "recall-tool")[0] == "tool-recalled:HELLO"
assert request(second, 18, "session/load", {
    "sessionId": s2, "cwd": "/tmp", "mcpServers": [],
}) == {}
assert prompt(second, 19, s2, "recall:language")[0] == "recalled:language=Python"
close(second)

(output / "acp-history-verification.json").write_text(json.dumps({
    "format": "phenix.basic.acp-history-verification.v1",
    "cases": [
        "acp-initialize-and-session-new",
        "acp-direct-response",
        "acp-user-and-assistant-recall",
        "acp-tool-call-result-stream",
        "acp-session-isolation",
        "acp-invalid-content-rejected",
        "acp-recovery-after-invalid-input",
        "acp-session-load-process-restart",
        "acp-tool-history-reloaded",
        "acp-multiple-session-loads",
    ],
    "accepted": True,
}, indent=2, sort_keys=True))
