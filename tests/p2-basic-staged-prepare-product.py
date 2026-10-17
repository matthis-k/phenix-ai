"""Opt-in native graph admission proof: preparation is not Model execution.

Default Basic composition and its existing graph remain unchanged.
"""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys

runtime, original_path, basic_agent, staged_agent, destination = sys.argv[1:]
out = Path(destination)
out.mkdir(parents=True, exist_ok=True)
source = json.loads(Path(original_path).read_text())
assert basic_agent in source["artifacts"], source["artifacts"]
assert staged_agent not in source["artifacts"], source["artifacts"]
assert source["entry"]["interface"] == "phenix.basic.input@1"
assert source["entry"]["operation"] == "submit"
key = "phenix.basic.turn-prepare@1/prepare"
advance_key = "phenix.basic.turn-advance@1/advance"
scoped = "phenix.session.s1"
stage = json.loads(json.dumps(source))
stage["artifacts"] = [
    staged_agent if artifact == basic_agent else artifact
    for artifact in source["artifacts"]
]
stage["authority"]["operations"].extend([key, advance_key])
pending_property = "phenix.basic.agent.property.pending_model_request@1"
stage["authority"]["operations"].extend([
    pending_property + "/set",
    pending_property + "/get",
])
stage["authority"]["resources"].append(pending_property[:-2])
stage["grants"].extend([
    {"caller": "phenix.basic.agent", "interface": pending_property,
     "operation": operation, "resources": [scoped, pending_property[:-2]]}
    for operation in ("get", "set")
])
stage["grants"].append({
    "caller": "phenix.basic.agent",
    "interface": "phenix.basic.turn-prepare@1",
    "operation": "prepare",
    "resources": [scoped],
})
stage["grants"].append({
    "caller": "phenix.basic.agent",
    "interface": "phenix.basic.turn-advance@1",
    "operation": "advance",
    "resources": [scoped],
})
prepared_property = "phenix.basic.agent.property.prepared_model_request@1"
stage["authority"]["operations"].extend([
    prepared_property + "/get",
    prepared_property + "/watch",
])
stage["authority"]["resources"].append(prepared_property[:-2])
stage["grants"].extend([
    {"caller": "phenix.basic.agent", "interface": prepared_property,
     "operation": operation, "resources": [scoped, prepared_property[:-2]]}
    for operation in ("get", "watch")
])
stage_path = out / "staged-graph.json"
stage_path.write_text(json.dumps(stage))

def submit(spec, database):
    env = dict(os.environ, PHENIX_BASIC_SQLITE_PATH=str(out / database))
    process = subprocess.run(
        [runtime, "jsonl", str(spec)],
        input=json.dumps({
            "type": "prompt", "session": "s1", "text": "graph-boundary",
            "limits": {"model_tokens": 256},
        }) + "\n",
        text=True, capture_output=True, env=env, timeout=60,
    )
    return process, [
        json.loads(line) for line in process.stdout.splitlines()
    ]

done, events = submit(stage_path, "staged.sqlite")
assert done.returncode == 0, (done.stdout, done.stderr)
kinds = [event["kind"] for event in events]
assert kinds.count("PromptAccepted") == 1, events
assert kinds.count("ModelStarted") == 1, events
assert "Answer" not in kinds, "preparation bypassed graph Model admission"
assert "ToolStarted" not in kinds, "preparation invoked a tool"
with sqlite3.connect(out / "staged.sqlite") as db:
    row = db.execute(
        "SELECT length(data) FROM basic_records "
        "WHERE session='s1' AND name='event-journal.v1'"
    ).fetchone()
    assert row is not None and row[0] > 0

# Counterfactual: selected plugin+declared graph node are not sufficient
# without an effective grant. No turn may be admitted or journalled.
denied = json.loads(json.dumps(stage))
denied["grants"] = [
    g for g in denied["grants"]
    if not (g["caller"] == "phenix.basic.agent"
            and g["interface"] == "phenix.basic.turn-prepare@1"
            and g["operation"] == "prepare")
]
denied_path = out / "stage-no-grant.json"
denied_path.write_text(json.dumps(denied))
bad, bad_events = submit(denied_path, "denied.sqlite")
assert bad.returncode != 0, (bad.stdout, bad.stderr)
assert not [e for e in bad_events if e["kind"] in (
    "PromptAccepted", "ModelStarted", "Answer", "ToolStarted"
)], bad_events

# Optional selected Advanced graph contribution. Reuse this exact staged
# composition builder rather than duplicating grant/origin logic in tests.
if len(sys.argv) == 9:
    graph_plugin, noop_plugin, advanced_definitions = sys.argv[6:]
    selected = json.loads(json.dumps(stage))
    selected["artifacts"].extend([graph_plugin, noop_plugin])
    selected["definitions"].append(advanced_definitions)
    selected["slots"].extend([
        {"caller": "phenix.advanced.graph-preparer", "slot": "Source",
         "index": 0, "provider": "phenix.basic.agent",
         "export": "property.pending_model_request"},
        {"caller": "phenix.advanced.graph-preparer", "slot": "Prepared",
         "index": 0, "provider": "phenix.basic.agent",
         "export": "property.prepared_model_request"},
        {"caller": "phenix.advanced.graph-preparer", "slot": "Preparer",
         "index": 0, "provider": "phenix.advanced.preparer-noop",
         "export": "Prepare"},
    ])
    selected["grants"].extend([
        {"caller": "phenix.advanced.graph-preparer",
         "interface": "phenix.advanced.model-preparation-change@1",
         "operation": "run", "resources": [scoped]},
        {"caller": "phenix.advanced.graph-preparer",
         "interface": "phenix.advanced.model-preparation@1",
         "operation": "prepare", "resources": [scoped]},
        {"caller": "phenix.advanced.graph-preparer",
         "interface": pending_property, "operation": op,
         "resources": [scoped, pending_property[:-2]]}
        for op in ("get", "watch")
    ])
    selected["grants"].append({
        "caller": "phenix.advanced.graph-preparer",
        "interface": prepared_property, "operation": "set",
        "resources": [scoped, prepared_property[:-2]],
    })
    selected["authority"]["operations"].extend([
        "phenix.advanced.model-preparation-change@1/run",
        "phenix.advanced.model-preparation@1/prepare",
        pending_property + "/watch",
        prepared_property + "/set",
    ])
    selected_path = out / "selected-graph.json"
    selected_path.write_text(json.dumps(selected))
    complete, complete_events = submit(selected_path, "selected.sqlite")
    assert complete.returncode == 0, (complete.stdout, complete.stderr)
    answers = [event["text"] for event in complete_events if event["kind"] == "Answer"]
    assert answers == ["mock:graph-boundary"], complete_events
    assert len([event for event in complete_events if event["kind"] == "PromptAccepted"]) == 1
    assert len([event for event in complete_events if event["kind"] == "ModelStarted"]) == 1
    assert not [event for event in complete_events if event["kind"] == "ToolStarted"]
    with sqlite3.connect(out / "selected.sqlite") as db:
        row = db.execute(
            "SELECT length(data) FROM basic_records WHERE session='s1' "
            "AND name='event-journal.v1'"
        ).fetchone()
        assert row is not None and row[0] > 0

    # Removing the separately selected advanced hook's effective write
    # authority must reject the whole graph before Basic journals any turn.
    denied_selected = json.loads(json.dumps(selected))
    denied_selected["grants"] = [
        grant for grant in denied_selected["grants"]
        if not (grant["caller"] == "phenix.advanced.graph-preparer"
                and grant["interface"] == prepared_property
                and grant["operation"] == "set")
    ]
    denied_selected_path = out / "selected-no-write-grant.json"
    denied_selected_path.write_text(json.dumps(denied_selected))
    denied_result, denied_events = submit(
        denied_selected_path, "selected-denied.sqlite",
    )
    assert denied_result.returncode != 0, (
        denied_result.stdout, denied_result.stderr,
    )
    assert not [event for event in denied_events if event["kind"] in (
        "PromptAccepted", "ModelStarted", "Answer", "ToolStarted"
    )], denied_events

(out / "accepted.json").write_text(json.dumps({
    "format": "phenix.p2.basic-staged-prepare.v1",
    "accepted": True,
    "selected_advanced_graph_tested": len(sys.argv) == 9,
    "cases": [
        "authorized-basic-prepare-publishes-model-request-without-model-execution",
        "missing-prepare-grant-denies-without-journal-effects",
        "original-basic-composition-remains-separate",
    ],
}, sort_keys=True, indent=2))
