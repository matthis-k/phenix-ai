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

# P2-3y RED: isolate a fresh native checkpoint writer so that CAS
# publication, readback and an independent restart prove idempotence.
publish_db = out / "checkpoint-publication.sqlite"
with sqlite3.connect(db) as source, sqlite3.connect(publish_db) as target:
    source.backup(target)
publish_env = dict(env, PHENIX_BASIC_SQLITE_PATH=str(publish_db))

def run(mode, runtime_env=None):
    folder = out / mode
    target = [
        runner, checkpoint, journal, storage, basic_defs, advanced_defs, mode, str(folder),
    ]
    result = subprocess.run(
        target, capture_output=True, text=True, env=runtime_env or env, timeout=90,
    )
    assert result.returncode == 0, (mode, result.returncode, result.stdout, result.stderr)
    return json.loads((folder / "compaction-product.json").read_text())

# Native CAS publication through the selected store: the new process
# must read back the authenticated checkpoint before returning acceptance.
published = run("publish", publish_env)
assert published["created"] is True, published
assert published["revision"] == 1, published
assert published["source_bound"] is True, published
reused = run("publish", publish_env)
assert reused["created"] is False, reused
assert reused["checkpoint_id"] == published["checkpoint_id"], reused
with sqlite3.connect(publish_db) as connection:
    assert connection.execute(
        "SELECT revision FROM basic_records "
        "WHERE session = 's1' AND name = 'compaction-checkpoints.v1'"
    ).fetchone() == (1,)
    assert connection.execute(
        "SELECT session, name, revision, hex(data) FROM basic_records "
        "WHERE name != 'compaction-checkpoints.v1' ORDER BY session, name"
    ).fetchall() == original

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
assert restarted["latest_checkpoint_id"] == "cp2", restarted
assert restarted["view_original_source_verified"] is True, restarted
assert restarted["view_summary_cited"] is True, restarted
assert restarted["view_uncompacted_events_exact"] is True, restarted
assert restarted["view_visible_payload_bytes"] > 0, restarted
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
        "selected-store-cas-publication-with-authenticated-readback",
        "publication-restart-reuses-exact-checkpoint-without-write",
        "immutable-linear-parent-checkpoint-history",
        "read-after-independent-process-restart",
        "latest-linear-checkpoint-after-restart",
        "latest-checkpoint-fails-closed-after-source-erasure",
        "reversible-source-cited-context-view-after-restart",
        "stale-write-conflict",
        "tampered-journal-hash-rejected",
        "unauthorized-cross-session-rejected",
        "basic-journal-not-mutated",
        "missing-source-denies-checkpoint-rehydration",
    ],
}, indent=2, sort_keys=True))
