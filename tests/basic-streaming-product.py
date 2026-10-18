"""Deterministic TextDelta journal projection through packaged Basic.

The history-test Model is synchronous: this fixture verifies event ordering,
text reconciliation and turn isolation, NOT incremental live transport progress.
Live mid-response streaming belongs to basic-pending-http-product.py.
Independent observer offsets/failed-delivery replay belong to observer-product.py.
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


# A synchronous direct reply yields one projected TextDelta matching Answer,
# ordered after ModelStarted and before the terminal event.
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

# A tool turn runs two synchronous rounds. The intermediate reply has no text
# and must not generate a TextDelta; only the final reply projects text.
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

# This checks session labels, not an independent observer subscription.
# Dedicated observer-product.py tests separate observer cursors.
second = jsonl(prompt("s2", "echo:other"))
assert streamed(second, "TextDelta")[0]["session"] == "s2", second
assert streamed(second, "TextDelta")[0]["text"] == "echo:other", second

# A model failure emits no TextDelta or Answer for that run; the streaming
# surface stays silent while the Failure event is journaled.
failed = jsonl(prompt("s1", "model-error"))
assert kinds(failed).count("TextDelta") == 0, failed
assert kinds(failed).count("Answer") == 0, failed
assert any(event["kind"] == "Failure" for event in failed), failed

# A new JSONL process with no input must emit no unsolicited output.
# Cross-observer acknowledged replay is established by observer-product.py.
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
        "textdelta-session-labels",
        "model-failure-no-streaming",
        "no-unsolicited-output-on-empty-restart",
    ],
    "accepted": True,
    "scope": "synchronous-journal-text-projection",
    "not_proven_here": [
        "incremental-live-model-progress",
        "independent-observer-cursor-replay",
        "active-midstream-cancellation",
    ],
}, sort_keys=True, indent=2))
