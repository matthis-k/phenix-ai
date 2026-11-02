"""Verify independent JSONL sessions and durable continuation across restarts."""
import json
import fcntl
import os
from pathlib import Path
import subprocess
import sqlite3
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
# Failed tools are domain outcomes, not terminal agent failures. Two
# sessions must keep separate journal and observer cursors after recovery.
tool_recovery = run([
    prompt("s2", "tools-fail:hello"),
    prompt("s1", "after-tool-failure"),
])
assert answers(tool_recovery) == [
    ("s2", "mock:call-1:error:scripted failure|call-2:5"),
    ("s1", "mock:after-tool-failure"),
], answers(tool_recovery)
assert run([]) == [], "tool-recovery journal was delivered twice"

# A Model domain error ends one graph, not the JSONL connection. The
# controller must deliver the durable Failure event, clear its owner marker,
# and service queued prompts in both the same and another session.
recovered_live = run([
    prompt("s2", "model-error"),
    prompt("s1", "jsonl-other-session"),
    prompt("s2", "jsonl-after-model-error"),
])
failure_events = [
    event for event in recovered_live
    if event["kind"] == "Failure" and event["session"] == "s2"
]
assert len(failure_events) == 1, recovered_live
assert failure_events[0]["text"] == "scripted model failure", failure_events
assert answers(recovered_live) == [
    ("s1", "mock:jsonl-other-session"),
    ("s2", "mock:jsonl-after-model-error"),
], answers(recovered_live)
assert run([]) == [], "failed JSONL turn was replayed after acknowledgement"
restarted = run([prompt("s2", "jsonl-after-process-restart")])
assert answers(restarted) == [
    ("s2", "mock:jsonl-after-process-restart"),
], answers(restarted)
assert run([]) == [], "restart repeated a previously acknowledged JSONL event"

# Caller-selected limits stop their individual Basic run but must never
# kill the JSONL controller, strand an owner marker or lose queued inputs.
# The tool-call cap rejects both proposed calls before either executes.
tool_cap = prompt("s2", "tools:hello")
tool_cap["limits"] = {"tool_calls": 1}
# The model-turn cap allows two tools, then stops before model continuation.
turn_cap = prompt("s2", "tools:hello")
turn_cap["limits"] = {"model_turns": 1, "tool_calls": 2}
limited = run([
    tool_cap,
    prompt("s1", "independent-after-limit"),
    turn_cap,
    prompt("s2", "same-session-after-limit"),
])
limited_failures = [
    event for event in limited
    if event["kind"] == "Failure" and event["session"] == "s2"
]
assert [event["text"] for event in limited_failures] == [
    "tool call limit reached",
    "model turn limit reached",
], limited_failures
# No tool side effect from the denied first run; exactly two from the
# allowed tool phase of the second, which then hits its model-turn cap.
started = [event for event in limited if event["kind"] == "ToolStarted"]
finished = [event for event in limited if event["kind"] == "ToolFinished"]
assert len(started) == len(finished) == 2, limited
assert answers(limited) == [
    ("s1", "mock:independent-after-limit"),
    ("s2", "mock:same-session-after-limit"),
], answers(limited)
assert run([]) == [], "limit failure duplicated acknowledged JSONL output"
assert answers(run([prompt("s2", "after-limits-restart")])) == [
    ("s2", "mock:after-limits-restart"),
]
assert run([]) == [], "limit-recovery restart duplicated JSONL output"

assert (output / "jsonl-multisession.db").is_file()

# Seed history using another authorized observer so JSONL has pending replay.
env["PHENIX_BASIC_SQLITE_PATH"] = str(output / "jsonl-observation-lock.db")
for session in ("s1", "s2"):
    seeded = subprocess.run(
        [binary, "terminal", composition, "--session", session],
        input=f"pending-{session}\n", text=True, capture_output=True,
        env=env, timeout=45,
    )
    assert seeded.returncode == 0, seeded.stderr


def records(session):
    with sqlite3.connect(env["PHENIX_BASIC_SQLITE_PATH"]) as db:
        return dict((name, (revision, data)) for name, revision, data in db.execute(
            "SELECT name, revision, data FROM basic_records WHERE session=?", (session,)))


before = records("s1")
lock_path = env["PHENIX_BASIC_SQLITE_PATH"] + ".phenix-session-7331.lock"
with open(lock_path, "r+b") as owner:
    fcntl.flock(owner, fcntl.LOCK_EX | fcntl.LOCK_NB)
    assert run([]) == [], "busy startup observation should be skipped"
    progressed = run([prompt("s2", "while-s1-owned")])
    assert answers(progressed) == [
        ("s2", "mock:pending-s2"),
        ("s2", "mock:while-s1-owned"),
    ], progressed
    assert all(event["session"] == "s2" for event in progressed), progressed
    for packet in (prompt("s1", "must-not-run"), {"type": "cancel", "session": "s1"}):
        rejected = subprocess.run(
            [binary, "jsonl", composition], input=json.dumps(packet) + "\n", text=True,
            capture_output=True, env=env, timeout=45,
        )
        assert rejected.returncode == 0, rejected
        failures = [json.loads(line) for line in rejected.stdout.splitlines()]
        assert len(failures) == 1, failures
        assert failures[0]["kind"] == "Failure" and failures[0]["sequence"] == 0, failures
        assert failures[0]["run"] == failures[0]["call_id"] == "", failures
        assert "session already owned" in failures[0]["text"], failures
        assert failures[0]["data"], failures
    assert records("s1") == before, "startup replay mutated a locked session"
assert answers(run([])) == [("s1", "mock:pending-s1")]
assert run([]) == [], "resumed startup replay duplicated events"

# Seed fresh nondefault history after the independent-session prompt acknowledged it.
seeded = subprocess.run(
    [binary, "terminal", composition, "--session", "s2"],
    input="pending-s2-again\n", text=True, capture_output=True, env=env, timeout=45,
)
assert seeded.returncode == 0, seeded.stderr
before = records("s2")
replayed = run([{"type": "cancel", "session": "s2"}])
assert answers(replayed) == [("s2", "mock:pending-s2-again")], replayed
after = records("s2")
changed = {name for name in before.keys() | after.keys() if before.get(name) != after.get(name)}
assert changed and all("observer" in name for name in changed), changed
assert run([{"type": "cancel", "session": "s2"}]) == []

(output / "jsonl-multisession.json").write_text(json.dumps({
    "format": "phenix.basic.jsonl-multisession.v1",
    "cases": [
        "interleaved-session-prompts",
        "scoped-output-isolation",
        "independent-observer-checkpoints",
        "process-restart-continuation",
        "idle-cancel-isolation",
        "startup-pending-replay-lock-exclusion",
        "busy-default-session-independent-progress",
        "busy-default-session-explicit-request-rejection",
        "startup-replay-resumes-after-unlock",
        "nondefault-idle-cancel-observation-only",
        "failed-tool-model-continuation",
        "model-failure-process-restart",
        "same-process-model-failure-continuation",
        "caller-tool-limit-no-effect",
        "caller-model-turn-limit-continuation",
        "limit-recovery-session-isolation",
    ],
    "accepted": True,
}, indent=2, sort_keys=True))
