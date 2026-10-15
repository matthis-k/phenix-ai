"""P2-2a packaged product: authenticated project storage through real SQLite and M1 kernel.

This test is committed before any new project contract, plugin or runner.
The database also contains Basic session/journal data, which must remain unchanged.
"""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys

basic, runner, project_storage, basic_definitions, advanced_definitions, output = sys.argv[1:]
out = Path(output)
out.mkdir(parents=True, exist_ok=True)
db = out / "project-memory.sqlite"
env = dict(os.environ, PHENIX_BASIC_SQLITE_PATH=str(db))

seed = subprocess.run(
    [basic, "jsonl"],
    input=json.dumps({"type": "prompt", "session": "s1", "text": "echo:basic-unchanged"}) + "\n",
    env=env, text=True, capture_output=True, timeout=60,
)
assert seed.returncode == 0, (seed.stdout, seed.stderr)
assert any(
    e.get("kind") == "Answer" and e.get("text") == "mock:echo:basic-unchanged"
    for e in map(json.loads, seed.stdout.splitlines())
), seed.stdout

def basic_snapshot():
    with sqlite3.connect(db) as connection:
        return connection.execute(
            "SELECT session, name, revision, hex(data) "
            "FROM basic_records ORDER BY session, name"
        ).fetchall()

baseline = basic_snapshot()
assert any(name == "event-journal.v1" for _, name, _, _ in baseline)

def verify(mode):
    dest = out / mode
    dest.mkdir()
    process = subprocess.run(
        [
            runner, project_storage, basic_definitions, advanced_definitions,
            mode, str(dest),
        ],
        env=env, text=True, capture_output=True, timeout=90,
    )
    assert process.returncode == 0, (
        mode, process.returncode, process.stdout, process.stderr
    )
    report = json.loads((dest / "project-storage.json").read_text())
    assert report["accepted"] is True, report
    assert basic_snapshot() == baseline, "project storage mutated Basic records"
    return report

first = verify("write")
assert first["initial_missing"] == "not_found", first
assert first["created_revision"] == 1, first
assert first["updated_revision"] == 2, first
assert first["stale_cas"] == "conflict", first
assert first["atomic_rollback"] is True, first
assert first["foreign_project_read"] == "denied", first
assert first["foreign_project_write"] == "denied", first
assert first["invalid_key"] == "rejected", first

second = verify("restart")
assert second["shared_project_revision"] == 2, second
assert second["shared_project_data"] == "blue", second
assert second["other_project_missing"] == "not_found", second
assert second["foreign_project_read"] == "denied", second

with sqlite3.connect(db) as conn:
    rows = conn.execute(
        "SELECT project, name, revision FROM project_records ORDER BY project, name"
    ).fetchall()
    version = conn.execute("PRAGMA user_version").fetchone()[0]
assert ("alpha", "theme", 2) in rows, rows
assert version == 1, "project provider changed the Basic schema version"
assert basic_snapshot() == baseline

(out / "p2-project-storage-product.json").write_text(json.dumps({
    "format": "phenix.p2.project-storage-product.v1",
    "accepted": True,
    "cases": [
        "real-basic-session-journal-unchanged",
        "actual-native-project-sqlite-provider",
        "first-create-and-revision-cas",
        "multi-key-atomic-rollback",
        "two-independent-kernel-processes",
        "cross-session-project-sharing",
        "cross-project-identity-denial",
        "other-project-storage-isolation",
        "invalid-key-rejected",
        "basic-database-schema-version-retained",
    ],
}, indent=2, sort_keys=True))
