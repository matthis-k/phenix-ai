"""Deterministic assistant-text streaming through the packaged Basic runtime.

The history-test Model has no hidden state. A streaming answer must surface as
an ordered TextDelta journal event before the terminal Answer, and a tool turn
must not emit a stray TextDelta for its empty intermediate reply.
"""
import json
import os
from pathlib import Path
import subprocess
import sys

binary, destination = sys.argv[1:]
output = Path(destination)
output.mkdir(parents=True, exist_ok=True)
environment = dict(os.environ)
environment["PHENIX_BASIC_SQLITE_PATH"] = str(output / "streaming.sqlite")


def jsonl(*messages):
    payload = "".join(json.dumps(message) + "\n" for message in messages)
    result = subprocess.run(
        [binary, "jsonl"],
        input=payload, text=True, capture_output=True, env=environment, timeout=45,
    )
    assert result.returncode == 0, (result.stdout, result.stderr)
    return [json.loads(line) for line in result.stdout.splitlines()]


def prompt(session, content):
    return {"type": "prompt", "session": session, "text": content}


def kinds(events):
    return [event["kind"] for event in events]


def streamed(events, kind):
    return [event for event in events if event["kind"] == kind]


# A direct reply streams exactly one TextDelta that matches the terminal
# Answer, and it is ordered after ModelStarted and before Answer.
direct = jsonl(prompt("s1", "echo:streaming"))
assert kinds(direct).count("ModelStarted") == 1, direct
assert kinds(direct).count("TextDelta") == 1, direct
assert kinds(direct).count("Answer") == 1, direct
deltas = streamed(direct, "TextDelta")
answers = streamed(direct, "Answer")
assert deltas[0]["text"] == "echo:streaming", deltas
assert answers[0]["text"] == "echo:streaming", answers
assert deltas[0]["session"] == "s1", deltas
assert deltas[0]["sequence"] < answers[0]["sequence"], direct
assert kinds(direct).index("ModelStarted") < kinds(direct).index("TextDelta") < kinds(direct).index("Answer"), direct

# A tool turn runs two model rounds. The intermediate reply has no text and
# must contribute no TextDelta; only the final continuation streams.
tool = jsonl(prompt("s1", "tools:hello"))
assert kinds(tool).count("ModelStarted") == 2, tool
assert kinds(tool).count("TextDelta") == 1, tool
assert kinds(tool).count("Answer") == 1, tool
assert kinds(tool).count("ToolStarted") == 1, tool
assert kinds(tool).count("ToolFinished") == 1, tool
started = streamed(tool, "ToolStarted")[0]
finished = streamed(tool, "ToolFinished")[0]
assert started["call_id"] == "history-tool-1", started
assert finished["call_id"] == "history-tool-1", finished
assert started["sequence"] < finished["sequence"], tool
assert streamed(tool, "TextDelta")[0]["text"] == "tool-result:HELLO"
assert streamed(tool, "Answer")[0]["text"] == "tool-result:HELLO"
assert streamed(tool, "TextDelta")[0]["sequence"] < streamed(tool, "Answer")[0]["sequence"]

# Streaming events are scoped to the submitting session and never leak to a
# second observer cursor. A second session streams independently.
second = jsonl(prompt("s2", "echo:other"))
assert streamed(second, "TextDelta")[0]["session"] == "s2", second
assert streamed(second, "TextDelta")[0]["text"] == "echo:other", second

# A model failure emits no TextDelta or Answer for that run; the streaming
# surface stays silent while the Failure event is journaled.
failed = jsonl(prompt("s1", "model-error"))
assert kinds(failed).count("TextDelta") == 0, failed
assert kinds(failed).count("Answer") == 0, failed
assert any(event["kind"] == "Failure" for event in failed), failed

# Restart without input replays no acknowledged TextDelta event twice.
assert jsonl() == [], "acknowledged streaming output replayed on restart"

(output / "streaming.json").write_text(json.dumps({
    "format": "phenix.basic.streaming.v1",
    "cases": [
        "direct-reply-textdelta-before-answer",
        "single-model-round-one-textdelta",
        "tool-turn-two-model-rounds",
        "intermediate-tool-reply-no-textdelta",
        "final-tool-continuation-streams",
        "tool-call-identity-preserved",
        "streaming-session-isolation",
        "model-failure-no-streaming",
        "restart-no-textdelta-replay",
    ],
    "accepted": True,
}, sort_keys=True, indent=2))
