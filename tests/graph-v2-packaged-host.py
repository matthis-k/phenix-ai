"""Drive the production host with immutable, canonically packaged V2 metadata."""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys

runtime, composition, output = sys.argv[1:]
output = Path(output)
database = output / "session.sqlite"
env = dict(os.environ, PHENIX_BASIC_SQLITE_PATH=str(database.resolve()))
assert not database.exists()


def prompt(text, *, storage_failure=False):
    result = subprocess.run(
        [runtime, "terminal", composition],
        input=text + "\n", text=True, capture_output=True, env=env, timeout=60,
    )
    if storage_failure:
        assert result.returncode != 0 and "StorageFailed" in result.stderr, (result.stdout, result.stderr)
    else:
        assert result.returncode == 0, (result.stdout, result.stderr)
    return result.stdout


# No Session::open, auth setup, metadata rewrite or alternate dispatcher.
first = prompt("hello-packaged-v2")
assert "mock:hello-packaged-v2" in first, first
assert database.is_file()
with sqlite3.connect(database) as db:
    sessions = db.execute("SELECT DISTINCT session FROM basic_records").fetchall()
assert len(sessions) == 1, sessions
session = sessions[0][0]


def journal_counts():
    with sqlite3.connect(database) as db:
        sessions = db.execute("SELECT DISTINCT session FROM basic_records").fetchall()
        assert sessions == [(session,)], sessions
        (data,) = db.execute(
            "SELECT data FROM basic_records WHERE session = ? AND name = 'event-journal.v1'",
            (session,),
        ).fetchone()
    return {name: data.count(name.encode()) for name in (
        "PromptAccepted", "ModelStarted", "ToolStarted", "ToolFinished", "Answer", "Failure",
    )}


assert journal_counts() == dict(
    PromptAccepted=1, ModelStarted=1, ToolStarted=0, ToolFinished=0, Answer=1, Failure=0,
), journal_counts()
second = prompt("hello-after-restart")
assert "mock:hello-after-restart" in second, second
assert "mock:hello-packaged-v2" not in second, second
failure = prompt("model-error")
assert "scripted model failure" in failure, failure
recovery = prompt("hello-after-failure")
assert "mock:hello-after-failure" in recovery, recovery
tool = prompt("tool:packaged-proof")
assert "mock:call-1:PACKAGED-PROOF" in tool, tool
assert tool.count("tool: uppercase") == 1 and tool.count("tool result: PACKAGED-PROOF") == 1, tool
tool_failure = prompt("tools-fail:packaged-proof")
assert "call-1:error:" in tool_failure and "call-2:14" in tool_failure, tool_failure
after_tool = prompt("hello-after-tool-restart")
assert "mock:hello-after-tool-restart" in after_tool, after_tool
same_host = prompt("hello-same-host\nmodel-error\nhello-same-host-recovered")
assert "mock:hello-same-host" in same_host, same_host
assert "scripted model failure" in same_host, same_host
assert "mock:hello-same-host-recovered" in same_host, same_host
counts = journal_counts()
assert counts == dict(
    PromptAccepted=10, ModelStarted=12, ToolStarted=3, ToolFinished=3, Answer=8, Failure=2,
), counts

# Fail the continuation's ModelStarted append after all tool effects have
# committed. Cleanup must terminate Running, without replaying the tool.
with sqlite3.connect(database) as db:
    model_started = "hex(CAST('ModelStarted' AS BLOB))"
    occurrences = f"(length(hex(NEW.data)) - length(replace(hex(NEW.data), {model_started}, ''))) / length({model_started})"
    db.execute(f"""CREATE TRIGGER reject_continuation BEFORE UPDATE ON basic_records
        WHEN NEW.name = 'event-journal.v1'
        AND {occurrences} > {counts['ModelStarted'] + 1}
        BEGIN SELECT RAISE(ABORT, 'injected continuation journal failure'); END""")
continuation_failure = prompt("tool:continuation-fault", storage_failure=True)
assert "mock:call-1:CONTINUATION-FAULT" not in continuation_failure, continuation_failure
fault_counts = journal_counts()
assert fault_counts == {**counts, "PromptAccepted": 11, "ModelStarted": 13,
                       "ToolStarted": 4, "ToolFinished": 4, "Failure": 3}, fault_counts
with sqlite3.connect(database) as db:
    db.execute("DROP TRIGGER reject_continuation")
recovered = prompt("hello-after-continuation-fault")
assert "mock:hello-after-continuation-fault" in recovered, (continuation_failure, recovered)

# Policy never substitutes for the composition's operation grants. Reject
# before loading plugins or creating durable state, without touching metadata.
denied = json.loads(Path(composition).read_text())
denied["grants"] = [grant for grant in denied["grants"] if not (
    grant["caller"] == "phenix.basic.agent"
    and grant["interface"] == "phenix.basic.graph-turn@1"
    and grant["operation"] == "tool"
)]
denied_path = output / "denied-composition.json"
denied_path.write_text(json.dumps(denied))
denied_database = output / "denied.sqlite"
rejected = subprocess.run(
    [runtime, "terminal", str(denied_path)],
    input="must-not-execute\n", text=True, capture_output=True, timeout=60,
    env=dict(env, PHENIX_BASIC_SQLITE_PATH=str(denied_database.resolve())),
)
assert rejected.returncode != 0, rejected.stdout
assert "graph-v2 operation lacks prepared grant" in rejected.stderr, rejected.stderr
assert not denied_database.exists()
(output / "acceptance.json").write_text(json.dumps({
    "session": session, "first_prompt": first, "restart": second,
    "domain_failure": failure, "recovery": recovery,
    "tool": tool, "tool_failure": tool_failure, "tool_restart": after_tool,
    "same_host_failure_recovery": same_host,
    "journal_counts": counts, "session_source": "SQLite basic_records after first default terminal prompt",
    "denied_before_load": rejected.stderr,
    "composition": composition, "runtime": runtime,
}, indent=2) + "\n")
