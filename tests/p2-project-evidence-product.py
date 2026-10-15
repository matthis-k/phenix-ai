"""P2-2b packaged EvidenceStore: a shared project source survives sessions.

Original Basic session journals stay private; project records are separately
authorized sources. The same source is revalidated on every project lookup.
"""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys

(
    basic, project_runner, evidence_runner, store, source, journal, sqlite,
    project_storage, basic_defs, advanced_defs, destination,
) = sys.argv[1:]
out = Path(destination)
out.mkdir(parents=True, exist_ok=True)
env = dict(os.environ, PHENIX_BASIC_SQLITE_PATH=str(out / "project-evidence.sqlite"))
seed = subprocess.run(
    [basic, "jsonl"],
    input="".join(json.dumps({"type": "prompt", "session": session, "text": "echo:" + session}) + "\n"
                  for session in ("s1", "s2", "s3")),
    env=env, text=True, capture_output=True, timeout=60,
)
assert seed.returncode == 0, (seed.stdout, seed.stderr)


def run(cmd, mode):
    destination = out / mode
    result = subprocess.run(
        cmd + [mode, str(destination)],
        env=env, text=True, capture_output=True, timeout=90,
    )
    assert result.returncode == 0, (mode, result.stdout, result.stderr)
    name = "evidence-store.json" if cmd[0] == evidence_runner else "project-storage.json"
    return json.loads((destination / name).read_text())


def snapshot(table):
    """Inspect durable effects independently of the native denial replies."""
    with sqlite3.connect(out / "project-evidence.sqlite") as connection:
        if table == "project_records":
            return connection.execute(
                "SELECT project, name, revision, hex(data) FROM project_records "
                "ORDER BY project, name"
            ).fetchall()
        if table == "basic_records":
            return connection.execute(
                "SELECT session, name, revision, hex(data) FROM basic_records "
                "ORDER BY session, name"
            ).fetchall()
    raise ValueError(table)


original_basic = snapshot("basic_records")
project = [project_runner, project_storage, basic_defs, advanced_defs]
evidence = [
    evidence_runner, store, source, journal, sqlite, basic_defs, advanced_defs,
    project_storage,
]

assert run(project, "member-grant")["membership_created"] is True
written = run(evidence, "project-write")
assert written["source_verified"] is True
assert written["stored_by_session"] == "s1"
before_denied_recall = snapshot("project_records")
recalled = run(evidence, "project-recall")
assert recalled["reader"] == "s2"
assert recalled["statement"] == "The project color is blue"
assert recalled["unapproved_session_denied"] is True
assert recalled["unapproved_search_denied"] is True
assert recalled["cross_project_search_denied"] is True
assert recalled["unapproved_write_denied"] is True
assert recalled["cross_project_denied"] is True
assert recalled["cross_project_write_denied"] is True
assert snapshot("project_records") == before_denied_recall, (
    "unauthorized read/search/write changed project storage"
)
assert run(project, "source-changed")["stale_revision_changed"] is True
after_source_change = snapshot("project_records")
stale = run(evidence, "project-stale")
assert stale["source_changed"] is True
assert stale["stale_search_denied"] is True
assert stale["stale_write_denied"] is True
assert snapshot("project_records") == after_source_change, (
    "stale source denial had a durable side effect"
)
assert run(project, "member-revoke")["revoked"] is True
revoked = run(evidence, "project-revoked")
assert revoked["revoked_session_denied"] is True
assert revoked["revoked_search_denied"] is True
assert revoked["revoked_write_denied"] is True
assert revoked["retained_session_source_changed"] is True
assert snapshot("project_records") == after_source_change, (
    "revoked membership denial mutated durable project evidence"
)
assert snapshot("basic_records") == original_basic, (
    "project evidence modified authoritative Basic session records"
)

(out / "p2-project-evidence-product.json").write_text(json.dumps({
    "format": "phenix.p2.project-evidence-product.v1",
    "accepted": True,
    "cases": [
        "two-basic-sessions-one-project",
        "project-evidence-written-by-member",
        "project-evidence-retrieved-by-other-member",
        "source-digest-revalidated",
        "stale-revision-denial",
        "revocation-denies-active-projection",
        "foreign-and-nonmember-denial",
        "foreign-and-nonmember-write-denial",
        "foreign-and-nonmember-search-denial",
        "stale-source-search-and-write-denial",
        "revoked-member-write-denial",
        "revoked-member-search-denial",
        "denied-operations-have-no-storage-effects",
        "existing-session-journals-not-exposed",
    ],
}, indent=2))
