"""P2-2b membership admission, revocation and restart through native kernel calls.

This slice proves membership only. Project evidence retrieval needs its own
source-verification integration and is not accepted by this test.
"""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys

basic, runner, project_storage, basic_definitions, advanced_definitions, destination = sys.argv[1:]
out = Path(destination)
out.mkdir(parents=True, exist_ok=True)
db = out / "membership.sqlite"
env = dict(os.environ, PHENIX_BASIC_SQLITE_PATH=str(db))

# Real Basic sessions exist before project membership is administered.
seed = subprocess.run(
    [basic, "jsonl"],
    input="".join(json.dumps({"type": "prompt", "session": session, "text": "echo:" + session}) + "\n"
                  for session in ("s1", "s2", "s3")),
    text=True, capture_output=True, env=env, timeout=60,
)
assert seed.returncode == 0, (seed.stdout, seed.stderr)
answers = [(item["session"], item["text"]) for item in map(json.loads, seed.stdout.splitlines())
           if item.get("kind") == "Answer"]
assert answers == [(session, "mock:echo:" + session) for session in ("s1", "s2", "s3")], answers


def snapshot():
    with sqlite3.connect(db) as conn:
        return conn.execute(
            "SELECT session, name, revision, hex(data) FROM basic_records ORDER BY session, name"
        ).fetchall()


before = snapshot()


def run(mode):
    folder = out / mode
    result = subprocess.run(
        [runner, project_storage, basic_definitions, advanced_definitions, mode, str(folder)],
        capture_output=True, text=True, env=env, timeout=60,
    )
    assert result.returncode == 0, (mode, result.stdout, result.stderr)
    assert snapshot() == before, "project membership mutated Basic storage"
    return json.loads((folder / "project-storage.json").read_text())


granted = run("member-grant")
assert granted["membership_created"] is True
assert granted["forged_admin_denied"] is True
checked = run("member-check")
assert checked["two_sessions_authorized"] is True
assert checked["impersonation_denied"] is True
assert checked["nonmember_denied"] is True
assert checked["foreign_project_denied"] is True
revoked = run("member-revoke")
assert revoked["revision"] == 2
after = run("member-after-revoke")
assert after["revoked_member_denied"] == "s2"
with sqlite3.connect(db) as conn:
    rows = conn.execute(
        "SELECT project, session, revision, active FROM project_membership ORDER BY project, session"
    ).fetchall()
assert rows == [("alpha", "s1", 1, 1), ("alpha", "s2", 2, 0)], rows
(out / "p2-project-membership-product.json").write_text(json.dumps({
    "format": "phenix.p2.project-membership-product.v1",
    "accepted": True,
    "checks": [
        "real-independent-basic-session-records",
        "project-root-only-membership-administration",
        "session-root-identity-check",
        "two-sessions-one-stable-project",
        "cross-project-denial",
        "persistent-revocation-across-processes",
        "membership-cas-conflict",
        "unchanged-basic-storage",
    ],
}, indent=2))
