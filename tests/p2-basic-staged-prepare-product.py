"""Opt-in native graph admission proof: preparation is not Model execution.

Default Basic composition and its existing graph remain unchanged.
"""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys

runtime, original_path, basic_agent, staged_agent, destination, *selected_args = sys.argv[1:]
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
# Basic owns this value. The graph may observe it, but neither the root
# nor an imported consumer can set it through a property operation.
stage["authority"]["operations"].append(pending_property + "/get")
stage["authority"]["resources"].append(pending_property[:-2])
# The root Basic action is the authority ceiling for its entire graph wave.
# Unless this root grant contains the property resource, the child node's
# narrower property grant cannot restore it later.
for grant in stage["grants"]:
    if (grant["caller"] == "root"
            and grant["interface"] == "phenix.basic.input@1"
            and grant["operation"] == "submit"):
        grant["resources"].append(pending_property[:-2])
stage["grants"].extend([
    {"caller": "phenix.basic.agent", "interface": pending_property,
     "operation": operation, "resources": [scoped, pending_property[:-2]]}
    for operation in ("get",)
])
stage["grants"].append({
    "caller": "phenix.basic.agent",
    "interface": "phenix.basic.turn-prepare@1",
    "operation": "prepare",
    "resources": [scoped, pending_property[:-2]],
})
stage["grants"].append({
    "caller": "phenix.basic.agent",
    "interface": "phenix.basic.turn-advance@1",
    "operation": "advance",
    "resources": [scoped],
})
stage_path = out / "staged-graph.json"
stage_path.write_text(json.dumps(stage))

def submit(spec, database, prompt="graph-boundary"):
    env = dict(os.environ, PHENIX_BASIC_SQLITE_PATH=str(out / database))
    process = subprocess.run(
        [runtime, "jsonl", str(spec)],
        input=json.dumps({
            "type": "prompt", "session": "s1", "text": prompt,
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
if len(sys.argv) in (9, 10):
    graph_plugin, noop_plugin, advanced_definitions = sys.argv[6:9]
    failure_model_plugin = sys.argv[9] if len(sys.argv) == 10 else None
    selected = json.loads(json.dumps(stage))
    selected["artifacts"].extend([graph_plugin, noop_plugin])
    selected["definitions"].append(advanced_definitions)
    prepared_property = "phenix.advanced.graph-preparer.property.prepared_model_request@1"
    selected["authority"]["operations"].extend([
        prepared_property + "/get",
        prepared_property + "/watch",
    ])
    selected["authority"]["resources"].append(prepared_property[:-2])
    # The root's submitted graph wave is the authority ceiling for both
    # producer publication and selected Basic advance.
    for grant in selected["grants"]:
        if (grant["caller"] == "root"
                and grant["interface"] == "phenix.basic.input@1"
                and grant["operation"] == "submit"):
            grant["resources"].append(prepared_property[:-2])
    # The executing Basic handler reads Advanced's exported candidate.
    for grant in selected["grants"]:
        if (grant["caller"] == "phenix.basic.agent"
                and grant["interface"] == "phenix.basic.turn-advance@1"
                and grant["operation"] == "advance"):
            grant["resources"].extend([
                pending_property[:-2], prepared_property[:-2],
            ])
    selected["slots"].extend([
        {"caller": "phenix.advanced.graph-preparer", "slot": "Source",
         "index": 0, "provider": "phenix.basic.agent",
         "export": "property.pending_model_request"},
        {"caller": "phenix.basic.agent", "slot": "Prepared",
         "index": 0, "provider": "phenix.advanced.graph-preparer",
         "export": "property.prepared_model_request"},
        {"caller": "phenix.advanced.graph-preparer", "slot": "Advance",
         "index": 0, "provider": "phenix.basic.agent",
         "export": "Advance"},
        {"caller": "phenix.advanced.graph-preparer", "slot": "Preparer",
         "index": 0, "provider": "phenix.advanced.preparer-noop",
         "export": "Prepare"},
    ])
    selected["grants"].extend([
        {"caller": "phenix.advanced.graph-preparer",
         "interface": "phenix.advanced.model-preparation-change@1",
         "operation": "run",
         "resources": [scoped, pending_property[:-2], prepared_property[:-2]]},
        {"caller": "phenix.advanced.graph-preparer",
         "interface": "phenix.advanced.model-preparation@1",
         "operation": "prepare", "resources": [scoped]},
    ])
    selected["grants"].extend([
        {"caller": "phenix.advanced.graph-preparer",
         "interface": pending_property, "operation": op,
         "resources": [scoped, pending_property[:-2]]}
        for op in ("get", "watch")
    ])
    # Advanced is the only writer of the prepared value. The state trigger
    # and Basic consumer need read/watch, never property set authority.
    selected["grants"].extend([
        {"caller": "phenix.advanced.graph-preparer",
         "interface": prepared_property, "operation": "watch",
         "resources": [scoped, prepared_property[:-2]]},
        {"caller": "phenix.basic.agent",
         "interface": prepared_property, "operation": "get",
         "resources": [scoped, prepared_property[:-2]]},
        {"caller": "phenix.advanced.graph-preparer",
         "interface": "phenix.basic.turn-advance@1",
         "operation": "advance",
         "resources": [scoped, pending_property[:-2], prepared_property[:-2]]},
    ])
    selected["authority"]["operations"].extend([
        "phenix.advanced.model-preparation-change@1/run",
        "phenix.advanced.model-preparation@1/prepare",
        pending_property + "/watch",
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

    # A multi-turn tool workflow must re-enter the watched preparation graph
    # after the complete batch of tool results from each Model reply.
    # The scripted Model first requests
    # uppercase+length, then derives its answer from the exact tool results
    # it receives in the re-prepared second request. The prior slice failed
    # closed on any tool-bearing reply; the re-prepared history must now
    # reach the second Model call instead of being denied.
    tooled, tool_events = submit(selected_path, "selected-tools.sqlite", "tools:hello")
    assert tooled.returncode == 0, (tooled.stdout, tooled.stderr)
    tool_kinds = [event["kind"] for event in tool_events]
    assert tool_kinds.count("PromptAccepted") == 1, tool_events
    assert tool_kinds.count("ModelStarted") == 2, tool_events
    assert tool_kinds.count("ToolStarted") == 2, tool_events
    assert tool_kinds.count("ToolFinished") == 2, tool_events
    assert "Failure" not in tool_kinds, tool_events
    assert [event["text"] for event in tool_events if event["kind"] == "Answer"] == [
        "mock:call-1:HELLO|call-2:5"
    ], tool_events
    # Exactly one second Model wave follows *both* completed tools; no
    # transient empty RunResult may appear as a premature terminal Answer.
    model_waves = [event["sequence"] for event in tool_events
                   if event["kind"] == "ModelStarted"]
    tool_finish = [event["sequence"] for event in tool_events
                   if event["kind"] == "ToolFinished"]
    terminal = [event["sequence"] for event in tool_events
                if event["kind"] == "Answer"]
    assert len(model_waves) == 2 and len(tool_finish) == 2 and len(terminal) == 1
    assert model_waves[0] < tool_finish[0] < tool_finish[1] < model_waves[1] < terminal[0]

    # A new process must resume the completed session without executing any
    # old tool twice or surfacing an earlier internal graph-wave result.
    resumed, resumed_events = submit(selected_path, "selected-tools.sqlite", "after-tools")
    assert resumed.returncode == 0, (resumed.stdout, resumed.stderr)
    assert [event["text"] for event in resumed_events if event["kind"] == "Answer"] == [
        "mock:after-tools"
    ], resumed_events
    assert "ToolStarted" not in [event["kind"] for event in resumed_events], resumed_events


    # An actual Model domain failure must terminate its durable Running
    # checkpoint. A fresh launcher can then reuse the same session, without
    # dropping history or spuriously repeating the failed Model effect.
    failed, failure_events = submit(selected_path, "failed-then-retry.sqlite", "model-error")
    assert failed.returncode == 0, (failed.stdout, failed.stderr)
    assert any(event["kind"] == "Failure" for event in failure_events), failure_events
    assert not any(event["kind"] == "Answer" for event in failure_events), failure_events
    retry, retry_events = submit(selected_path, "failed-then-retry.sqlite", "after-failure")
    assert retry.returncode == 0, (retry.stdout, retry.stderr)
    assert [event["text"] for event in retry_events if event["kind"] == "Answer"] == [
        "mock:after-failure"
    ], retry_events
    assert not any(event["kind"] == "ToolStarted" for event in retry_events)

    if failure_model_plugin is not None:
        # Use a genuine native pending-capable Model plugin to exercise
        # kernel-level failures. Do not replace the real kernel/graph/Session.
        failing = json.loads(json.dumps(selected))
        failing["artifacts"].append(failure_model_plugin)
        bound = [
            slot for slot in failing["slots"]
            if slot["caller"] == "phenix.basic.agent" and slot["slot"] == "Model"
        ]
        assert len(bound) == 1, bound
        bound[0]["provider"] = "phenix.model.meter-fixture"
        failure_path = out / "selected-kernel-model-failures.json"
        failure_path.write_text(json.dumps(failing))
        for failure_prompt in ("kernel-error-begin", "kernel-error-poll"):
            database = failure_prompt + ".sqlite"
            failed, failure_events = submit(failure_path, database, failure_prompt)
            assert not any(event["kind"] == "Answer" for event in failure_events)
            assert any(event["kind"] == "Failure" for event in failure_events), (
                failed.stdout, failed.stderr, failure_events
            )
            retried, retry_events = submit(failure_path, database, "after-failure")
            assert retried.returncode == 0, (retried.stdout, retried.stderr)
            assert [event["text"] for event in retry_events
                    if event["kind"] == "Answer"] == ["fixture:after-failure"], retry_events
            assert not any(event["kind"] == "ToolStarted" for event in retry_events)

    # Removing the selected Advanced producer's *property resource* from
    # its operation grant must deny publication before any Basic answer.
    denied_selected = json.loads(json.dumps(selected))
    for grant in denied_selected["grants"]:
        if (grant["caller"] == "phenix.advanced.graph-preparer"
                and grant["interface"] == "phenix.advanced.model-preparation-change@1"
                and grant["operation"] == "run"):
            grant["resources"].remove(prepared_property[:-2])
    denied_selected_path = out / "selected-no-publish-resource.json"
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
    "selected_advanced_graph_tested": len(sys.argv) in (9, 10),
    "kernel_model_failures_tested": len(sys.argv) == 10,
    "cases": [
        "authorized-basic-prepare-publishes-model-request-without-model-execution",
        "missing-prepare-grant-denies-without-journal-effects",
        "original-basic-composition-remains-separate",
        "selected-advanced-graph-reenters-after-tool-batch",
        "selected-advanced-tool-completion-restart-no-reexecution",
    ],
}, sort_keys=True, indent=2))
