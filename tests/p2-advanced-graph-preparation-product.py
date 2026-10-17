"""Native M1 graph proof: Basic prepared request -> selected Advanced hook.

The Basic P1 preset is not replaced. The tested staging composition is
intentionally not a usable agent until a separate Basic advance node exists.
"""
import json
import os
from pathlib import Path
import subprocess
import sys

runtime, basic_composition, basic_agent, staged_agent, advanced_hook, output = sys.argv[1:]
out = Path(output)
out.mkdir(parents=True, exist_ok=True)
source = json.loads(Path(basic_composition).read_text())
assert basic_agent in source["artifacts"]
assert advanced_hook not in source["artifacts"]
stage = json.loads(json.dumps(source))
stage["artifacts"] = [
    staged_agent if item == basic_agent else item for item in source["artifacts"]
] + [advanced_hook]
session = "phenix.session.s1"
basic_property = "phenix.basic.agent.property.pending_model_request@1"
advanced_property = "phenix.advanced.graph-preparation.property.prepared@1"
prepare = "phenix.basic.turn-prepare@1"
hook = "phenix.advanced.graph-preparation@1"
resources = [session, basic_property.removesuffix("@1"), advanced_property.removesuffix("@1")]
operations = [
    prepare + "/prepare", hook + "/run",
    basic_property + "/get", basic_property + "/set", basic_property + "/watch",
    advanced_property + "/get", advanced_property + "/set", advanced_property + "/watch",
]
stage["authority"]["operations"] += operations
stage["authority"]["resources"] += resources[1:]
# The root action's resource ceiling precedes *all* graph node attenuation.
for grant in stage["grants"]:
    if grant["caller"] == "root" and grant["interface"] == "phenix.basic.input@1" and grant["operation"] == "submit":
        grant["resources"] += resources[1:]
        break
else:
    raise AssertionError("missing trusted Basic root submit grant")
stage["slots"].append({
    "caller": "phenix.advanced.graph-preparation",
    "slot": "Pending",
    "index": 0,
    "provider": "phenix.basic.agent",
    "export": "property.pending_model_request",
})
for caller, interface, verbs, granted_resources in [
    ("phenix.basic.agent", prepare, ["prepare"], resources),
    ("phenix.basic.agent", basic_property, ["get", "set"], resources),
    ("phenix.advanced.graph-preparation", hook, ["run"], resources),
    ("phenix.advanced.graph-preparation", basic_property, ["get", "watch"], resources),
    ("phenix.advanced.graph-preparation", advanced_property, ["get", "set"], resources),
]:
    stage["grants"] += [
        {"caller": caller, "interface": interface, "operation": verb,
         "resources": granted_resources} for verb in verbs
    ]
staged = out / "with-advanced-graph.json"
staged.write_text(json.dumps(stage))
witness = out / "advanced-prepared.sha256"

def invoke(spec, db, witness_path):
    env = dict(
        os.environ,
        PHENIX_BASIC_SQLITE_PATH=str(out / db),
        PHENIX_P2_GRAPH_PREPARED_WITNESS=str(witness_path),
    )
    run = subprocess.run(
        [runtime, "jsonl", str(spec)],
        input=json.dumps({
            "type": "prompt", "session": "s1",
            "text": "graph-preparation-candidate",
            "limits": {"model_tokens": 256},
        }) + "\n",
        text=True, capture_output=True, env=env, timeout=60,
    )
    events = [json.loads(line) for line in run.stdout.splitlines()]
    return run, events

run, events = invoke(staged, "allowed.sqlite", witness)
assert run.returncode == 0, (run.stdout, run.stderr)
assert witness.is_file(), ("Advanced state hook did not commit", events)
digest = witness.read_text()
assert len(digest) == 64 and all(char in "0123456789abcdef" for char in digest), digest
kinds = [e["kind"] for e in events]
assert kinds.count("PromptAccepted") == 1, events
assert kinds.count("ModelStarted") == 1, events
assert "Answer" not in kinds and "ToolStarted" not in kinds, events

# The observed trigger and selected property import grant are required:
# deleting the grant must not produce a prepared receipt or invoke Model.
denied = json.loads(json.dumps(stage))
denied["grants"] = [
    grant for grant in denied["grants"]
    if not (grant["caller"] == "phenix.advanced.graph-preparation"
            and grant["interface"] == basic_property
            and grant["operation"] == "get")
]
denied_path = out / "no-authorized-basic-read.json"
denied_path.write_text(json.dumps(denied))
denied_witness = out / "denied-prepared.sha256"
bad, bad_events = invoke(denied_path, "denied.sqlite", denied_witness)
assert bad.returncode != 0, (bad.stdout, bad.stderr)
assert not denied_witness.exists(), bad_events
assert not [e for e in bad_events if e["kind"] in ("Answer", "ToolStarted")], bad_events

(out / "evidence.json").write_text(json.dumps({
    "format": "phenix.p2.graph-preparation-native.v1",
    "accepted": True,
    "cases": [
        "basic-watched-request-activates-selected-advanced-hook",
        "selected-advanced-hook-commits-source-bound-receipt",
        "no-model-or-tool-effect-before-basic-advance",
        "missing-basic-property-read-grant-denies-advanced-receipt",
    ],
}, sort_keys=True, indent=2))
