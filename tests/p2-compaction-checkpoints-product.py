"""Native P2-3c checkpoint persistence with real Basic journal + SQLite.

Three independent native processes verify CAS, immutable source bytes,
restart lineage and fail-closed retrieval when the journal disappears.
"""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys

basic, runner, checkpoint, journal, storage, basic_defs, advanced_defs, destination = sys.argv[1:]
out = Path(destination)
out.mkdir(parents=True, exist_ok=True)
db = out / "checkpoint.sqlite"
env = dict(os.environ, PHENIX_BASIC_SQLITE_PATH=str(db))

seed = subprocess.run(
    [basic, "jsonl"],
    input="".join(
        json.dumps({"type": "prompt", "session": "s1", "text": "echo:" + text}) + "\n"
        for text in ("first source", "second source")
    ),
    capture_output=True, text=True, env=env, timeout=60,
)
assert seed.returncode == 0, (seed.stdout, seed.stderr)
answers = [
    item["text"] for item in map(json.loads, seed.stdout.splitlines())
    if item.get("kind") == "Answer"
]
assert answers == ["mock:echo:first source", "mock:echo:second source"], answers

def snapshot_original():
    with sqlite3.connect(db) as connection:
        return connection.execute(
            "SELECT session, name, revision, hex(data) FROM basic_records "
            "WHERE name != 'compaction-checkpoints.v1' ORDER BY session, name"
        ).fetchall()

original = snapshot_original()
assert any(row[1] == "event-journal.v1" for row in original), original

def run(mode):
    folder = out / mode
    target = [
        runner, checkpoint, journal, storage, basic_defs, advanced_defs, mode, str(folder),
    ]
    result = subprocess.run(target, capture_output=True, text=True, env=env, timeout=90)
    assert result.returncode == 0, (mode, result.returncode, result.stdout, result.stderr)
    return json.loads((folder / "compaction-product.json").read_text())

write = run("write")
assert write["written"] == 2, write
assert write["verified_journal_events"] >= 4, write
assert write["stale_conflict"] is True
assert write["tampered_source_denied"] is True
assert write["foreign_session_denied"] is True
assert snapshot_original() == original, "checkpoint writes mutated Basic source journal"

restarted = run("restart")
assert restarted["restored"] is True
assert restarted["revisions"] == [1, 2]
assert snapshot_original() == original, "checkpoint read mutated Basic source journal"

with sqlite3.connect(db) as connection:
    rows = connection.execute(
        "SELECT session, revision FROM basic_records "
        "WHERE name = 'compaction-checkpoints.v1'"
    ).fetchall()
    assert rows == [("s1", 2)], rows
    connection.execute(
        "DELETE FROM basic_records WHERE session = 's1' AND name = 'event-journal.v1'"
    )
    connection.commit()

missing = run("source-missing")
assert missing["stale_checkpoint_denied"] is True
(out / "p2-compaction-checkpoints-product.json").write_text(json.dumps({
    "format": "phenix.p2.compaction-checkpoints-product.v1",
    "accepted": True,
    "cases": [
        "independent-basic-journal-source",
        "native-checkpoint-journal-sqlite-binding",
        "checkpoint-revision-cas",
        "immutable-linear-parent-checkpoint-history",
        "read-after-independent-process-restart",
        "stale-write-conflict",
        "tampered-journal-hash-rejected",
        "unauthorized-cross-session-rejected",
        "basic-journal-not-mutated",
        "missing-source-denies-checkpoint-rehydration",
    ],
}, indent=2, sort_keys=True))
