"""Independent P2-1 EvidenceStore product acceptance over real SQLite and native providers.

The Basic executable writes the source journal. Each verifier invocation starts
another kernel and loads the production EvidenceStore, SourceEvidence,
EventJournal and SQLite Storage plugins. No fake or in-process provider.
"""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys

(
    basic, runner, store, source, journal, sqlite, basic_defs, advanced_defs,
    destination,
) = sys.argv[1:]
out = Path(destination)
out.mkdir(parents=True, exist_ok=True)
database = out / "evidence.sqlite"
env = dict(os.environ, PHENIX_BASIC_SQLITE_PATH=str(database))

seed = subprocess.run(
    [basic, "jsonl"],
    input="".join(
        json.dumps({"type": "prompt", "session": "s1", "text": "echo:" + fact}) + "\\n"
        for fact in ("The project color was violet", "The project color is blue")
    ),
    capture_output=True, text=True, env=env, timeout=60,
)
assert seed.returncode == 0, (seed.stdout, seed.stderr)
events = [json.loads(line) for line in seed.stdout.splitlines()]
assert {item.get("text") for item in events if item.get("kind") == "Answer"} >= {
    "mock:echo:The project color was violet",
    "mock:echo:The project color is blue",
}, events
assert database.is_file()

def basic_records():
    with sqlite3.connect(database) as connection:
        return connection.execute(
            "SELECT session, name, revision, hex(data) FROM basic_records "
            "WHERE name NOT LIKE 'evidence-store.%' ORDER BY session, name"
        ).fetchall()

original = basic_records()
assert any(name == "event-journal.v1" for _, name, _, _ in original)

def inspect(action):
    target = out / action
    target.mkdir()
    args = [
        runner, store, source, journal, sqlite, basic_defs, advanced_defs,
        action, str(target),
    ]
    process = subprocess.run(
        args, capture_output=True, text=True, env=env, timeout=90,
    )
    assert process.returncode == 0, (
        action, process.returncode, process.stdout, process.stderr,
    )
    report = json.loads((target / "evidence-store.json").read_text())
    assert report["accepted"] is True, report
    return report

write = inspect("write")
assert write["source_verified"] is True, write
assert write["created_revision"] == 1, write
assert write["corrected_revision"] == 2, write
assert write["stale_cas"] == "conflict", write
assert write["tampered_source"] == "changed", write
assert write["missing_source"] == "rejected", write
assert write["unbacked_claim"] == "rejected", write
assert write["cross_session"] == "denied", write
assert write["source_digest_sha256"], write
assert basic_records() == original, "evidence provider mutated Basic's journal"

read = inspect("restart")
assert read["retrieved_revision"] == 2, read
assert read["current_statement"] == "The project color is blue", read
assert read["historical_statement"] == "The project color was violet", read
assert read["cross_session"] == "denied", read
assert read["source_digest_sha256"] == write["source_digest_sha256"], read
assert basic_records() == original, "EvidenceStore modified authoritative sources"

with sqlite3.connect(database) as connection:
    evidence_rows = connection.execute(
        "SELECT session, name, revision FROM basic_records "
        "WHERE name = 'evidence-store.v1'"
    ).fetchall()
assert evidence_rows == [("s1", "evidence-store.v1", 2)], evidence_rows

(out / "p2-evidence-store-product.json").write_text(json.dumps({
    "format": "phenix.p2.evidence-store-product.v1",
    "accepted": True,
    "cases": [
        "real-basic-journal-source",
        "native-store-source-journal-sqlite-chain",
        "create-and-revision-checked-correction",
        "immutable-history-and-as-of",
        "read-after-independent-process-restart",
        "stale-cas-denial",
        "forged-and-missing-source-denial",
        "unbacked-evidence-denial",
        "cross-session-denial",
        "basic-journal-not-mutated",
    ],
}, indent=2, sort_keys=True))
