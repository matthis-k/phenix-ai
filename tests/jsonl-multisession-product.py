"""Verify independent JSONL sessions and durable continuation across restarts."""
import json
import os
from pathlib import Path
import subprocess
import sys

binary, composition, destination = sys.argv[1:]
output = Path(destination)
output.mkdir(parents=True, exist_ok=True)
env = dict(os.environ)
env["PHENIX_BASIC_SQLITE_PATH"] = str(output / "jsonl-multisession.db")


def run(packets):
    result = subprocess.run(
        [binary, "jsonl", composition],
        input="".join(json.dumps(item) + "\n" for item in packets),
        text=True, capture_output=True, env=env, timeout=45,
    )
    assert result.returncode == 0, (result.stdout, result.stderr)
    return [json.loads(line) for line in result.stdout.splitlines()]


def prompt(session, text):
    return {"type": "prompt", "session": session, "text": text}


def answers(events):
    return [(item["session"], item["text"]) for item in events if item["kind"] == "Answer"]


first = run([
    prompt("s1", "first-one"),
    prompt("s2", "first-two"),
    prompt("s1", "second-one"),
    prompt("s2", "second-two"),
])
assert answers(first) == [
    ("s1", "mock:first-one"),
    ("s2", "mock:first-two"),
    ("s1", "mock:second-one"),
    ("s2", "mock:second-two"),
], answers(first)
assert {event["session"] for event in first} == {"s1", "s2"}
# A restart without input replays no acknowledged s1 events.
assert run([]) == [], "acknowledged s1 journal was replayed"
second = run([prompt("s2", "third-two"), prompt("s1", "third-one")])
assert answers(second) == [
    ("s2", "mock:third-two"),
    ("s1", "mock:third-one"),
], answers(second)
assert {event["session"] for event in second} == {"s1", "s2"}

# An idle cancellation for another session must not alter durable state.
assert run([{"type": "cancel", "session": "s1"}]) == []
third = run([prompt("s2", "fourth-two")])
assert answers(third) == [("s2", "mock:fourth-two")], answers(third)
assert (output / "jsonl-multisession.db").is_file()

(output / "jsonl-multisession.json").write_text(json.dumps({
    "format": "phenix.basic.jsonl-multisession.v1",
    "cases": [
        "interleaved-session-prompts",
        "scoped-output-isolation",
        "independent-observer-checkpoints",
        "process-restart-continuation",
        "idle-cancel-isolation",
    ],
    "accepted": True,
}, indent=2, sort_keys=True))
