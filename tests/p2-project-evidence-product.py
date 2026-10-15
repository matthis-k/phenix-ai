"""P2-2b packaged EvidenceStore: a shared project source survives sessions.

Original Basic session journals stay private; project records are separately
authorized sources. The same source is revalidated on every project lookup.
"""
import json
import os
from pathlib import Path
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


project = [project_runner, project_storage, basic_defs, advanced_defs]
evidence = [
    evidence_runner, store, source, journal, sqlite, basic_defs, advanced_defs,
    project_storage,
]

assert run(project, "member-grant")["membership_created"] is True
written = run(evidence, "project-write")
assert written["source_verified"] is True
assert written["stored_by_session"] == "s1"
recalled = run(evidence, "project-recall")
assert recalled["reader"] == "s2"
assert recalled["statement"] == "The project color is blue"
assert recalled["unapproved_session_denied"] is True
assert recalled["cross_project_denied"] is True
assert run(project, "source-changed")["stale_revision_changed"] is True
stale = run(evidence, "project-stale")
assert stale["source_changed"] is True
assert run(project, "member-revoke")["revoked"] is True
revoked = run(evidence, "project-revoked")
assert revoked["revoked_session_denied"] is True
assert revoked["retained_session_source_changed"] is True

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
        "existing-session-journals-not-exposed",
    ],
}, indent=2))
