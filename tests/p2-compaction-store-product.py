"""Packaged P2-3c checkpoint CAS across separate native kernel processes."""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys

basic, runner, plugin, journal, storage, basic_defs, advanced_defs, destination = sys.argv[1:]
out = Path(destination)
out.mkdir(parents=True, exist_ok=True)
db = out / "sessions.sqlite"
env = dict(os.environ, PHENIX_BASIC_SQLITE_PATH=str(db))

seed = subprocess.run(
    [basic, "jsonl"],
    input=json.dumps({"type": "prompt", "session": "s1", "text": "remember:exact tool bytes"}) + "\n",
    text=True, capture_output=True, env=env, timeout=60,
)
assert seed.returncode == 0, (seed.stdout, seed.stderr)
assert any(item.get("kind") == "Answer" for item in map(json.loads, seed.stdout.splitlines()))


def run(mode):
    folder = out / mode
    result = subprocess.run(
        [runner, plugin, journal, storage, basic_defs, advanced_defs, mode, str(folder)],
        capture_output=True, text=True, env=env, timeout=60,
    )
    assert result.returncode == 0, (mode, result.stdout, result.stderr)
    return json.loads((folder / "compaction.json").read_text())


with sqlite3.connect(db) as conn:
    original = conn.execute(
        "SELECT session, name, revision, hex(data) FROM basic_records "
        "WHERE name = 'event-journal.v1' ORDER BY session"
    ).fetchall()
assert original, "Basic's source journal was not persisted"

committed = run("commit")
assert committed["first_revision"] == 1, committed
assert committed["second_revision"] == 2, committed
assert committed["stale_cas_denied"] is True, committed
assert committed["foreign_session_denied"] is True, committed

restored = run("restart")
assert restored["revision"] == 2, restored
assert restored["parent_checkpoint"] == "checkpoint-a", restored
assert restored["source_revalidated"] is True, restored

with sqlite3.connect(db) as conn:
    unchanged = conn.execute(
        "SELECT session, name, revision, hex(data) FROM basic_records "
        "WHERE name = 'event-journal.v1' ORDER BY session"
    ).fetchall()
assert unchanged == original, "checkpoint commit changed the authoritative journal"

# Simulate intentional removal of the underlying journal: persisted summaries
# must fail closed rather than impersonating an original source.
with sqlite3.connect(db) as conn:
    conn.execute(
        "DELETE FROM basic_records WHERE session = 's1' AND name = 'event-journal.v1'"
    )
deleted = run("missing-source")
assert deleted["hydration_denied"] is True, deleted

(out / "p2-compaction-store-product.json").write_text(json.dumps({
    "format": "phenix.p2.compaction-store-product.v1",
    "accepted": True,
    "cases": [
        "real-basic-session-journal",
        "native-checkpoint-cas-two-revisions",
        "stale-cas-denied",
        "foreign-session-denied",
        "independent-process-restart-retrieval",
        "authoritative-journal-unchanged",
        "source-removal-invalidates-derived-checkpoint",
    ],
}, indent=2))
