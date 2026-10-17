"""P2-3aj: run the real selected Advanced state-change preparation node.

This composes the already-tested opt-in Basic prepare root; it does not
replace the P1 Basic binary or bypass the kernel graph scheduler.
"""
import json
import os
from pathlib import Path
import subprocess
import sys

runtime, basic_staged_json, advanced_plugin, destination = sys.argv[1:]
out = Path(destination)
out.mkdir(parents=True, exist_ok=True)
stage = json.loads(Path(basic_staged_json).read_text())
scoped = "phenix.session.s1"
basic_prop = "phenix.basic.agent.property.pending_model_request@1"
advanced_prop = "phenix.advanced.graph-preparer.property.proposal@1"
basic_resource = basic_prop.removesuffix("@1")
advanced_resource = advanced_prop.removesuffix("@1")
resources = [scoped, basic_resource, advanced_resource]
stage["artifacts"].append(advanced_plugin)
stage["slots"].append({
    "caller": "phenix.advanced.graph-preparer", "slot": "Pending",
    "index": 0, "provider": "phenix.basic.agent",
    "export": "property.pending_model_request",
})
stage["authority"]["resources"].append(advanced_resource)
stage["authority"]["operations"].extend([
    "phenix.advanced.graph-prepare@1/run",
    "phenix.advanced.model-preparation@1/prepare",
    basic_prop + "/get",
    advanced_prop + "/get",
    advanced_prop + "/set",
])
# The admitted root action intersects its own resource grant with the
# composition; absent the new resource the hook cannot commit its proposal.
for grant in stage["grants"]:
    if (grant["caller"] == "root"
            and grant["interface"] == "phenix.basic.input@1"
            and grant["operation"] == "submit"):
        grant["resources"].append(advanced_resource)
stage["grants"].extend([
    {"caller": "phenix.advanced.graph-preparer",
     "interface": "phenix.advanced.graph-prepare@1",
     "operation": "run", "resources": resources},
    {"caller": "phenix.advanced.graph-preparer",
     "interface": "phenix.advanced.model-preparation@1",
     "operation": "prepare", "resources": resources},
    {"caller": "phenix.advanced.graph-preparer",
     "interface": basic_prop,
     "operation": "get", "resources": resources},
    {"caller": "phenix.advanced.graph-preparer",
     "interface": advanced_prop,
     "operation": "get", "resources": resources},
    {"caller": "phenix.advanced.graph-preparer",
     "interface": advanced_prop,
     "operation": "set", "resources": resources},
])
full_path = out / "with-advanced-prepare.json"
full_path.write_text(json.dumps(stage))


def execute(spec, name):
    witness = out / (name + "-prepared.sha256")
    env = dict(os.environ,
               PHENIX_BASIC_SQLITE_PATH=str(out / (name + ".sqlite")),
               PHENIX_P2_PREPARER_WITNESS=str(witness))
    process = subprocess.run(
        [runtime, "jsonl", str(spec)],
        input=json.dumps({
            "type": "prompt", "session": "s1", "text": "graph-prepared",
            "limits": {"model_tokens": 256},
        }) + "\n",
        capture_output=True, text=True, env=env, timeout=60,
    )
    events = [json.loads(line) for line in process.stdout.splitlines()]
    return process, events, witness


result, events, witness = execute(full_path, "selected")
assert result.returncode == 0, (result.stdout, result.stderr)
assert witness.exists(), (events, result.stderr)
digest = witness.read_text()
assert len(digest) == 64 and all(c in "0123456789abcdef" for c in digest), digest
kinds = [e["kind"] for e in events]
assert kinds.count("PromptAccepted") == 1, events
assert kinds.count("ModelStarted") == 1, events
assert "Answer" not in kinds and "ToolStarted" not in kinds, events

# Missing selected property read grant cannot forge or reconstruct a Basic
# request. The Basic turn may be journalled; the Advanced hook must be denied.
denied = json.loads(json.dumps(stage))
denied["grants"] = [
    g for g in denied["grants"]
    if not (g["caller"] == "phenix.advanced.graph-preparer"
            and g["interface"] == basic_prop and g["operation"] == "get")
]
denied_path = out / "denied-advanced-read.json"
denied_path.write_text(json.dumps(denied))
failed, denied_events, denied_witness = execute(denied_path, "denied")
assert failed.returncode != 0, (failed.stdout, failed.stderr)
assert not denied_witness.exists(), denied_events
assert not [e for e in denied_events if e["kind"] in ("Answer", "ToolStarted")], denied_events

(out / "accepted.json").write_text(json.dumps({
    "format": "phenix.p2.graph-preparer.v1",
    "accepted": True,
    "cases": [
        "actual-selected-state-triggered-graph-node-publishes-source-bound-proposal",
        "denied-Basic-property-import-prevents-untrusted-Advanced-proposal",
        "no-Model-or-tool-bypass",
    ],
}, indent=2, sort_keys=True))
