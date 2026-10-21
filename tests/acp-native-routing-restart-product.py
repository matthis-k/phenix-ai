"""Exercise durable model-routing fail-closed behavior over real ACP and SQLite.

An authentic provider-selected happy path is separately tracked: the installed
builtins currently expose registration, but not executable model discovery.
These tests verify that a durable *inconsistent* routing state cannot silently
restore the environment-configured HTTP preset after process restart.
"""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys

runtime, basic, http_basic, destination = sys.argv[1:]
output = Path(destination)
output.mkdir(parents=True, exist_ok=True)

SELECTED = "phenix.provider.selected-model.v1"
WITNESS = "phenix.provider.native-routing-witness.v1"


def launch(composition, database):
    env = dict(os.environ)
    env.update(
        PHENIX_BASIC_SQLITE_PATH=str(database),
        PHENIX_MODEL_HTTP_ENDPOINT="http://127.0.0.1:1/preset-must-not-run",
        PHENIX_MODEL_HTTP_MODEL="untrusted-environment-preset",
        PHENIX_MODEL_HTTP_API_KEY="environment-not-selected",
    )
    return subprocess.Popen(
        [runtime, "acp", composition], env=env, stdin=subprocess.PIPE,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1,
    )


def call(child, number, method, params):
    child.stdin.write(json.dumps({
        "jsonrpc": "2.0", "id": number, "method": method, "params": params,
    }) + "\n")
    child.stdin.flush()
    for _ in range(256):
        line = child.stdout.readline()
        assert line, (method, child.poll(), child.stderr.read())
        reply = json.loads(line)
        if reply.get("method") == "session/update":
            continue
        assert reply.get("id") == number, reply
        return reply
    raise AssertionError("too many ACP notifications")


def initialize(child):
    reply = call(child, 1, "initialize", {
        "protocolVersion": 1, "clientCapabilities": {},
    })
    assert reply["result"]["protocolVersion"] == 1, reply


def stop(child):
    child.stdin.close()
    status = child.wait(timeout=30)
    diagnostic = child.stderr.read()
    child.stdout.close()
    child.stderr.close()
    assert status == 0, (status, diagnostic[:1200])


def create_durable_session(database):
    child = launch(basic, database)
    initialize(child)
    created = call(child, 2, "session/new", {
        "cwd": "/tmp", "mcpServers": [],
    })
    session = created["result"]["sessionId"]
    result = call(child, 3, "session/prompt", {
        "sessionId": session,
        "prompt": [{"type": "text", "text": "durable-witness-baseline"}],
    })
    assert result["result"]["stopReason"] == "end_turn", result
    stop(child)
    with sqlite3.connect(database) as connection:
        stored = connection.execute(
            "SELECT COUNT(*) FROM basic_records WHERE session = ?",
            (session,),
        ).fetchone()[0]
        assert stored > 0, "scripted session did not checkpoint to SQLite"
    return session


def seed_disrupted_routing_record(database, session, case):
    if case == "untouched":
        return
    with sqlite3.connect(database) as connection:
        records = {
            "orphaned_witness": [(WITNESS, b"fixture-native-witness")],
            "orphaned_selection": [(SELECTED, b"fixture-selected-identity")],
            "corrupt_pair": [
                (SELECTED, b"fixture-selected-identity"),
                (WITNESS, b"corrupt-native-witness"),
            ],
        }[case]
        for key, data in records:
            connection.execute(
                "INSERT INTO basic_records (session, name, revision, data) "
                "VALUES (?, ?, 1, ?)", (session, key, data),
            )
        connection.commit()


evidence = {}
for label in ("untouched", "orphaned_witness", "orphaned_selection", "corrupt_pair"):
    database = output / ("witness-" + label + ".sqlite")
    session = create_durable_session(database)
    seed_disrupted_routing_record(database, session, label)

    # This is an entirely new native ACP process and kernel; the in-memory
    # selected_http_seen set from the previous process cannot help it.
    restarted = launch(http_basic, database)
    initialize(restarted)
    loaded = call(restarted, 2, "session/load", {
        "sessionId": session, "cwd": "/tmp", "mcpServers": [],
    })
    if label == "untouched":
        assert "error" not in loaded, loaded
        options = loaded["result"]["configOptions"]
        assert len(options) == 1 and options[0]["currentValue"] == "phenix.model.http"
        assert options[0]["options"] == [{
            "name": "phenix.model.http", "value": "phenix.model.http",
        }], options
    else:
        assert "error" in loaded, (label, loaded)
        # A denied load must not turn an already inserted session kernel
        # into an admitted preset prompt even within the new ACP process.
        attempted = call(restarted, 3, "session/prompt", {
            "sessionId": session,
            "prompt": [{"type": "text", "text": "should-never-hit-preset"}],
        })
        assert "error" in attempted, (label, attempted)
    stop(restarted)
    evidence[label] = {
        "restarted": True,
        "load_denied": label != "untouched",
        "prompt_denied": label != "untouched",
    }

(output / "native-routing-restart.json").write_text(json.dumps({
    "format": "phenix.native-routing-restart.v1",
    "test_isolation": "fresh ACP process and SQLite database for each case",
    "no_successful_provider_selection_claimed": True,
    "cases": evidence,
}, sort_keys=True, indent=2))
print("Restarted ACP denies orphaned/corrupt native-routing state")
