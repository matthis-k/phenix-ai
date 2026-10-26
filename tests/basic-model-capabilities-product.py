"""Credential-free, black-box ModelRequest capability and tool wiring check.

This is independent of provider authentication, provider model discovery, and
history persistence acceptance. The Model fixture reports the exact metadata
Basic supplied, rather than an expected hard-coded fake catalog.
"""
import json
import os
from pathlib import Path
import subprocess
import sys

binary, destination = sys.argv[1:]
output = Path(destination)
output.mkdir(parents=True, exist_ok=True)
env = dict(os.environ)
for name in (
    "OPENAI_API_KEY", "ANTHROPIC_API_KEY", "OPENROUTER_API_KEY",
    "PHENIX_CREDENTIAL_API_KEY_ENV_NAME", "PHENIX_CREDENTIAL_FILE_CONSENT",
):
    env.pop(name, None)
env["PHENIX_BASIC_SQLITE_PATH"] = str(output / "model-capabilities.sqlite")


def prompt(session, content):
    return {"type": "prompt", "session": session, "text": content}


def run(*commands):
    result = subprocess.run(
        [binary, "jsonl"],
        input="".join(json.dumps(command) + "\n" for command in commands),
        capture_output=True, text=True, env=env, timeout=45,
    )
    assert result.returncode == 0, (result.returncode, result.stderr)
    return [json.loads(line) for line in result.stdout.splitlines()]


def answers(events):
    return [(event["session"], event["text"])
            for event in events if event["kind"] == "Answer"]


# The test model sees only the authoritative ModelRequest projection. No
# fixture-owned service registry, HTTP endpoint or credential can supply this.
events = run(
    prompt("probe-a", "inspect:capabilities"),
    prompt("probe-b", "inspect:capabilities"),
)
observed = answers(events)
assert len(observed) == 2, events
assert [session for session, _ in observed] == ["probe-a", "probe-b"]
a, b = (json.loads(text) for _, text in observed)
assert a == b, (a, b)
assert a["format"] == "phenix.test.model-capabilities.v1"
assert [tool["id"] for tool in a["tools"]] == [
    "fail", "length", "uppercase"
], a
assert all(isinstance(tool["description"], str) for tool in a["tools"])
assert all(isinstance(tool["idempotent"], bool) for tool in a["tools"])
assert all(isinstance(json.loads(tool["arguments_schema"]), dict)
           for tool in a["tools"]), a
assert isinstance(a["skills"], list)
assert isinstance(a["context"], list)
assert all(isinstance(item, str) for item in a["skills"] + a["context"])
assert a["skills"] == sorted(a["skills"]), a
assert a["context"] == sorted(a["context"]), a

# A capability is not just a displayed label: verify the same composition
# can execute the advertised uppercase tool, continue the Model turn, and
# complete without a provider login.
tool_events = run(prompt("probe-a", "tools:hello"))
assert answers(tool_events) == [("probe-a", "tool-result:HELLO")], tool_events
assert [event["kind"] for event in tool_events
        if event["kind"] in ("ToolStarted", "ToolFinished")] == [
            "ToolStarted", "ToolFinished"
        ], tool_events

(output / "accepted.json").write_text(json.dumps({
    "format": "phenix.test.model-capabilities.v1",
    "cases": [
        "actual-model-request-tool-catalog",
        "tool-id-and-schema-metadata",
        "skill-and-context-identifiers",
        "consistent-cross-session-catalog",
        "credential-free-execution",
        "advertised-tool-can-be-executed",
    ],
    "accepted": True,
}, sort_keys=True, indent=2))
