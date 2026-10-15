"""Packaged project record source verification across independent Basic sessions."""
import json
import os
from pathlib import Path
import subprocess
import sys

basic, runner, project_storage, basic_defs, advanced_defs, destination = sys.argv[1:]
out = Path(destination)
out.mkdir(parents=True, exist_ok=True)
env = dict(os.environ, PHENIX_BASIC_SQLITE_PATH=str(out / "project-source.sqlite"))

seed = subprocess.run(
    [basic, "jsonl"],
    input="".join(json.dumps({"type": "prompt", "session": sid, "text": "echo:" + sid}) + "\n"
                  for sid in ("s1", "s2", "s3")),
    text=True, capture_output=True, env=env, timeout=60,
)
assert seed.returncode == 0, (seed.stdout, seed.stderr)


def run(mode):
    target = out / mode
    result = subprocess.run(
        [runner, project_storage, basic_defs, advanced_defs, mode, str(target)],
        env=env, capture_output=True, text=True, timeout=60,
    )
    assert result.returncode == 0, (mode, result.stdout, result.stderr)
    return json.loads((target / "project-storage.json").read_text())


assert run("member-grant")["membership_created"] is True
current = run("source-current")
assert current["both_members_current"] is True
assert current["unapproved_session_denied"] is True
assert current["malformed_source_denied"] is True
assert current["forged_digest_changed"] is True
changed = run("source-changed")
assert changed["stale_revision_changed"] is True
assert changed["new_revision_current"] is True
assert run("member-revoke")["revoked"] is True
revoked = run("source-after-revoke")
assert revoked["revoked_source_denied"] is True
assert revoked["remaining_member_current"] is True
(out / "p2-project-source-product.json").write_text(json.dumps({
    "accepted": True,
    "format": "phenix.p2.project-source-product.v1",
    "checks": [
        "two-real-basic-sessions",
        "exact-project-source-digest",
        "independent-kernel-process-read",
        "missing-project-membership-denial",
        "source-revision-revalidation",
        "forged-source-rejection",
        "restart-and-revocation",
    ],
}, indent=2))
