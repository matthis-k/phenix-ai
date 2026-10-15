"""P2-1 product acceptance: physical native journal source validation.

This deliberately drives the production Basic binary to persist a journal,
then independently invokes the P2 provider through a second kernel process.
No in-process mock Journal, canned Model answer, or private test shortcut.
"""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys

basic, evidence_runner, evidence_package, journal_package, storage_package, basic_defs, advanced_defs, destination = sys.argv[1:]
out = Path(destination)
out.mkdir(parents=True, exist_ok=True)
db = out / "source-evidence.sqlite"
env = dict(os.environ)
env["PHENIX_BASIC_SQLITE_PATH"] = str(db)

prompt = json.dumps({
    "type": "prompt",
    "session": "s1",
    "text": "echo:journal-reference-proof",
}) + "\n"
seed = subprocess.run([basic, "jsonl"], input=prompt, env=env,
                      text=True, capture_output=True, timeout=60)
assert seed.returncode == 0, (seed.stdout, seed.stderr)
events = [json.loads(line) for line in seed.stdout.splitlines()]
assert any(x["kind"] == "Answer" and x["text"] == "mock:echo:journal-reference-proof"
           for x in events), events
assert db.is_file(), "Basic did not persist the real event journal"

def records():
    with sqlite3.connect(db) as conn:
        return conn.execute(
            "SELECT session,name,revision,hex(data) FROM basic_records ORDER BY session,name"
        ).fetchall()

original = records()
assert any(row[1] == "event-journal.v1" for row in original), original

def verify(which):
    output_path = out / which
    output_path.mkdir()
    process = subprocess.run(
        [evidence_runner, evidence_package, journal_package, storage_package,
         basic_defs, advanced_defs, str(output_path)],
        env=env, text=True, capture_output=True, timeout=60,
    )
    assert process.returncode == 0, (which, process.stdout, process.stderr)
    report = json.loads((output_path / "source-evidence.json").read_text())
    assert report["accepted"] is True, report
    assert report["current"] is True, report
    assert report["tampered"] == "changed", report
    assert report["foreign_session"] == "denied", report
    assert report["missing_sequence"] == "absent", report
    assert report["journal_unchanged"] is True, report
    assert report["source_revision"], report
    return report

first = verify("first")
second = verify("after-restart")
assert first["source_digest_sha256"] == second["source_digest_sha256"]
assert first["source_revision"] == second["source_revision"]
assert records() == original, "source observation mutated authoritative journal or session"
(out / "p2-source-evidence-product.json").write_text(json.dumps({
    "format": "phenix.p2.source-evidence-product.v1",
    "cases": [
        "real-basic-journal-seed",
        "independent-native-provider",
        "authorized-source-observation",
        "tamper-revision-evidence",
        "denied-cross-session",
        "missing-sequence-fails-closed",
        "two-independent-host-processes",
        "no-observer-side-storage-effects",
    ],
    "accepted": True,
}, sort_keys=True, indent=2))
