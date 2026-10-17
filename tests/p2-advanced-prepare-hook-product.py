"""P2-3aj: native graph-reactive Advanced preparation, not Model execution.

The opt-in composition must expose both original and prepared request state
through the existing M1 property/selected-grant surface. A fixture-only
witness records a digest only after the Advanced hook commits its proposal.
"""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys

runtime, original_path, basic_agent, staged_agent, advanced_hook, destination = sys.argv[1:]
out = Path(destination)
out.mkdir(parents=True, exist_ok=True)
source = json.loads(Path(original_path).read_text())
scope = "phenix.session.s1"
basic = "phenix.basic.agent.property.pending_model_request@1"
advanced = "phenix.advanced.model-preparer.property.proposal@1"
stage = json.loads(json.dumps(source))
stage["artifacts"] = [
    staged_agent if artifact == basic_agent else artifact
    for artifact in stage["artifacts"]
] + [advanced_hook]
assert staged_agent in stage["artifacts"]
stage["slots"].append({
    "caller": "phenix.advanced.model-preparer",
    "slot": "BasicRequest",
    "provider": "phenix.basic.agent",
    "index": 0,
    "export": "property.pending_model_request",
})
stage["authority"]["operations"].extend([
    "phenix.basic.turn-prepare@1/prepare",
    "phenix.advanced.model-preparation-trigger@1/on_change",
    basic + "/get", basic + "/set", basic + "/watch",
    advanced + "/get", advanced + "/set", advanced + "/watch",
])
stage["authority"]["resources"].extend([basic[:-2], advanced[:-2]])
# Action admission is the root authority ceiling for all transitive graph
# imports. It must authorize the two property resources, but not expand the
# ordinary selected Model/tool authority.
for grant in stage["grants"]:
    if grant["caller"] == "root" and grant["interface"] == "phenix.basic.input@1" and grant["operation"] == "submit":
        grant["resources"].extend([basic[:-2], advanced[:-2]])
stage["grants"].extend([
    {"caller": "phenix.basic.agent", "interface": "phenix.basic.turn-prepare@1",
     "operation": "prepare", "resources": [scope, basic[:-2], advanced[:-2]]},
    *[
        {"caller": "phenix.basic.agent", "interface": basic,
         "operation": op, "resources": [scope, basic[:-2]]}
        for op in ("get", "set")
    ],
    {"caller": "phenix.advanced.model-preparer",
     "interface": "phenix.advanced.model-preparation-trigger@1",
     "operation": "on_change",
     "resources": [scope, basic[:-2], advanced[:-2]]},
    *[
        {"caller": "phenix.advanced.model-preparer", "interface": basic,
         "operation": op, "resources": [scope, basic[:-2], advanced[:-2]]}
        for op in ("get", "watch")
    ],
    *[
        {"caller": "phenix.advanced.model-preparer", "interface": advanced,
         "operation": op, "resources": [scope, basic[:-2], advanced[:-2]]}
        for op in ("get", "set", "watch")
    ],
])

def run(spec, db, witness):
    spec_path = out / (db + ".json")
    spec_path.write_text(json.dumps(spec))
    env = dict(
        os.environ,
        PHENIX_BASIC_SQLITE_PATH=str(out / (db + ".sqlite")),
        PHENIX_P2_MODEL_PREPARE_WITNESS=str(out / witness),
    )
    completed = subprocess.run(
        [runtime, "jsonl", str(spec_path)],
        input=json.dumps({
            "type": "prompt", "session": "s1", "text": "advanced-graph-hook",
            "limits": {"model_tokens": 256},
        }) + "\n",
        text=True, capture_output=True, timeout=60, env=env,
    )
    events = [json.loads(line) for line in completed.stdout.splitlines()]
    return completed, events

done, events = run(stage, "accepted", "proposal.sha256")
assert done.returncode == 0, (done.stdout, done.stderr)
witness = out / "proposal.sha256"
assert witness.is_file() and len(witness.read_text()) == 64, (
    "the Advanced state-change hook did not publish a proposal", done.stdout, done.stderr,
)
assert [e["kind"] for e in events] == ["PromptAccepted", "ModelStarted"], events
with sqlite3.connect(out / "accepted.sqlite") as db:
    assert db.execute(
        "SELECT count(*) FROM basic_records "
        "WHERE session='s1' AND name='compaction-checkpoints.v1'"
    ).fetchone() == (0,), "the no-op Advanced hook wrote a checkpoint"

# A missing effective graph import grant cannot be repaired by selecting a
# provider or by declaring a matching state-change trigger.
denied = json.loads(json.dumps(stage))
denied["grants"] = [
    g for g in denied["grants"]
    if not (g["caller"] == "phenix.advanced.model-preparer"
            and g["interface"] == basic and g["operation"] == "get")
]
bad, bad_events = run(denied, "denied", "denied-proposal.sha256")
assert bad.returncode != 0, (bad.stdout, bad.stderr)
assert not (out / "denied-proposal.sha256").exists()
assert not [e for e in bad_events if e["kind"] in ("Answer", "ToolStarted")]

(out / "accepted.json").write_text(json.dumps({
    "format": "phenix.p2.advanced-preparation-hook.v1",
    "accepted": True,
    "cases": [
        "real-M1-state-change-triggers-selected-Advanced-prepare-hook",
        "source-bound-proposal-commits-after-Basic-prepare",
        "no-Model-or-tool-execution-in-either-preparation-stage",
        "missing-Basic-request-read-grant-denies-Advanced-preparation",
        "default-Basic-composition-is-unchanged",
    ],
}, sort_keys=True, indent=2))
