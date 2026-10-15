"""Black-box in-session history proof using the packaged Basic runtime.

The test model has no process-local memory. Every recall and transcript audit
must be reconstructed from ModelRequest.history supplied by the real agent.
"""
import json
import os
from pathlib import Path
import subprocess
import sys

binary, basic_path, verification_path, destination = sys.argv[1:]
output = Path(destination)
output.mkdir(parents=True, exist_ok=True)
environment = dict(os.environ)
environment["PHENIX_BASIC_SQLITE_PATH"] = str(output / "history-verification.sqlite")

basic = json.loads(Path(basic_path).read_text())
verification = json.loads(Path(verification_path).read_text())
basic_model = next(slot for slot in basic["slots"] if slot["caller"] == "phenix.basic.agent" and slot["slot"] == "Model")
verify_model = next(slot for slot in verification["slots"] if slot["caller"] == "phenix.basic.agent" and slot["slot"] == "Model")
assert basic_model["provider"] == "phenix.model.scripted", basic_model
assert verify_model["provider"] == "phenix.model.history-test", verify_model
assert len(verification["artifacts"]) == len(basic["artifacts"]) + 1
assert verification["artifacts"][:len(basic["artifacts"])] == basic["artifacts"]
assert verification["definitions"] == basic["definitions"]
for field in ("entry", "grants", "authority", "resource", "excluded", "substitutions"):
    assert verification[field] == basic[field], field
assert [slot for slot in verification["slots"] if slot["slot"] != "Model"] == [
    slot for slot in basic["slots"] if slot["slot"] != "Model"
]


def run(mode, lines, session=None):
    command = [binary, mode]
    if session:
        command.extend(["--session", session])
    result = subprocess.run(
        command,
        input=lines, text=True, capture_output=True, env=environment, timeout=45,
    )
    assert result.returncode == 0, (command, result.stdout, result.stderr)
    return result.stdout


def prompt(session, content):
    return {"type": "prompt", "session": session, "text": content}


def jsonl(*messages):
    payload = "".join(json.dumps(message) + "\n" for message in messages)
    return [json.loads(line) for line in run("jsonl", payload).splitlines()]


def answers(events):
    return [
        (event["session"], event["text"])
        for event in events if event["kind"] == "Answer"
    ]


# Interleave two sessions in one process, then verify assistant history as
# well as the earlier user message. A latest-message-only model must fail.
first = answers(jsonl(
    prompt("s1", "remember:color=violet"),
    prompt("s2", "remember:color=amber"),
    prompt("s1", "verify-assistant:color"),
    prompt("s2", "recall:color"),
    prompt("s1", "recall:color"),
))
assert first == [
    ("s1", "stored:color=violet"),
    ("s2", "stored:color=amber"),
    ("s1", "verified:color=violet"),
    ("s2", "recalled:color=amber"),
    ("s1", "recalled:color=violet"),
], first

# A new process must reload the full transcript. No hidden plugin state can
# survive; a previously unseen third session must not inherit either fact.
second = answers(jsonl(
    prompt("s1", "recall:color"),
    prompt("s2", "recall:color"),
    prompt("s3", "recall:color"),
))
assert second == [
    ("s1", "recalled:color=violet"),
    ("s2", "recalled:color=amber"),
    ("s3", "absent:color"),
], second

# Run a real Basic tool. The fake model checks tool-call identity and output,
# then a later prompt checks that those messages were checkpointed.
tool = jsonl(prompt("s1", "tools:hello"))
assert answers(tool) == [("s1", "tool-result:HELLO")], tool
assert [event["kind"] for event in tool if event["kind"] in ("ToolStarted", "ToolFinished")] == [
    "ToolStarted", "ToolFinished"
], tool
assert answers(jsonl(prompt("s1", "recall-tool"))) == [
    ("s1", "tool-recalled:HELLO")
]
assert answers(jsonl(prompt("s1", "recall-tool"))) == [
    ("s1", "tool-recalled:HELLO")
], "restart lost tool-call/result transcript"

# Run the identical agent/composition with the Terminal frontend. The fake
# model's answer still depends on earlier JSONL turns persisted by Session.
terminal = run("terminal", "recall:color\n", "s1")
assert "answer: recalled:color=violet" in terminal, terminal
assert run("terminal", "", "s1") == "", "terminal replayed acknowledged output"
# JSONL has an independent observer cursor; it must deliver the Terminal turn
# once rather than silently skip it or replay it again.
assert answers(jsonl()) == [("s1", "recalled:color=violet")]
assert jsonl() == [], "JSONL replayed the other frontend's acknowledged event"

# Direct text, non-ASCII input, multiple turns, updated facts and missing
# facts must have fixed responses and not leak between sessions.
direct = answers(jsonl(
    prompt("s1", "echo:Grüße"),
    prompt("s2", "echo:independent"),
    prompt("s1", "remember:color=indigo"),
    prompt("s1", "recall:color"),
    prompt("s2", "recall:unknown"),
))
assert direct == [
    ("s1", "echo:Grüße"),
    ("s2", "echo:independent"),
    ("s1", "stored:color=indigo"),
    ("s1", "recalled:color=indigo"),
    ("s2", "absent:unknown"),
], direct

# A tool may fail as a domain result without killing the Basic turn. The
# second tool must execute, and the model must see both results in order.
failed_tools = jsonl(prompt("s2", "tools-fail:hello"))
assert answers(failed_tools) == [
    ("s2", "tools-recovered:scripted failure|5")
], failed_tools
assert len([event for event in failed_tools if event["kind"] == "ToolStarted"]) == 2, failed_tools
assert len([event for event in failed_tools if event["kind"] == "ToolFinished"]) == 2, failed_tools
assert answers(jsonl(prompt("s2", "recall:color"))) == [
    ("s2", "recalled:color=amber")
], "a failed tool corrupted the prior session transcript"


def failures(events):
    return [(event["session"], event["text"]) for event in events
            if event["kind"] == "Failure"]


# Failures are scoped to the submitting run. In the same stdin connection,
# subsequent requests must continue and must produce exactly one answer.
recover = jsonl(
    prompt("s1", "model-error"),
    prompt("s2", "echo:other-session-survived"),
    prompt("s1", "echo:same-session-recovered"),
)
assert failures(recover) == [
    ("s1", "verification model failure"),
], recover
assert answers(recover) == [
    ("s2", "echo:other-session-survived"),
    ("s1", "echo:same-session-recovered"),
], recover

# Each user limit is exercised against the actual Basic graph, not against a
# direct call to the test model. No denied tool call may execute.
token_cap = prompt("s1", "echo:limited")
token_cap["limits"] = {"model_tokens": 0}
tool_cap = prompt("s1", "tools-fail:hello")
tool_cap["limits"] = {"tool_calls": 1}
turn_cap = prompt("s1", "tools:hello")
turn_cap["limits"] = {"model_turns": 1, "tool_calls": 1}
limited = jsonl(
    token_cap,
    tool_cap,
    turn_cap,
    prompt("s1", "echo:after-limits"),
)
assert failures(limited) == [
    ("s1", "verification model token limit"),
    ("s1", "tool call limit reached"),
    ("s1", "model turn limit reached"),
], limited
assert answers(limited) == [("s1", "echo:after-limits")], limited
assert len([event for event in limited if event["kind"] == "ToolStarted"]) == 1, limited
assert len([event for event in limited if event["kind"] == "ToolFinished"]) == 1, limited

# An idle cancellation must not poison a session or duplicate journal output.
assert jsonl({"type": "cancel", "session": "s1"}) == []
assert answers(jsonl(prompt("s1", "recall:color"))) == [
    ("s1", "recalled:color=indigo")
]
assert jsonl() == [], "acknowledged JSONL output replayed on process restart"
invalid = subprocess.run(
    [binary, "terminal", "--session", "../escape"],
    input="", text=True, capture_output=True, env=environment, timeout=20,
)
assert invalid.returncode != 0, invalid
assert "answer:" not in invalid.stdout, invalid.stdout

# The original Basic configuration must remain usable on its own. In
# particular, the verification executable must not replace the default model.
assert Path(environment["PHENIX_BASIC_SQLITE_PATH"]).is_file()
(output / "history-verification.json").write_text(json.dumps({
    "format": "phenix.basic.history-verification.v1",
    "cases": [
        "existing-basic-composition-unchanged",
        "only-selected-model-overridden",
        "previous-user-message",
        "previous-assistant-message",
        "interleaved-multi-session",
        "cross-session-isolation",
        "restart-continues-history",
        "tool-call-and-result-history",
        "tool-history-after-restart",
        "terminal-reuses-jsonl-session",
        "independent-observer-delivery",
        "direct-text-and-unicode",
        "interleaved-text-replies",
        "latest-fact-wins",
        "missing-fact-is-absent",
        "two-tools-one-domain-failure",
        "model-failure-recovery",
        "per-session-model-error-isolation",
        "model-token-limit",
        "tool-call-limit-denies-all",
        "model-turn-limit-after-tool",
        "failure-does-not-poison-followup",
        "idle-cancellation",
        "observer-checkpoint-no-replay",
        "invalid-session-resource-refused",
    ],
    "accepted": True,
}, sort_keys=True, indent=2))
