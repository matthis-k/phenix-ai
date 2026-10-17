"""P2-3l RED: a selected Advanced adapter must measure before calling Model.

The Model and ModelTokenMeter slots point at the same fixture provider.
The default public Advanced composition has no Meter slot and stays unchanged.
"""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys

(
    runtime, advanced_json, meter_provider, checkpoint_runner,
    checkpoint_provider, journal_provider, storage_provider,
    basic_definitions, advanced_definitions, destination,
) = sys.argv[1:]
out = Path(destination)
out.mkdir(parents=True, exist_ok=True)
source = json.loads(Path(advanced_json).read_text())
adapter = "phenix.advanced.model-adapter"
model = "phenix.model.meter-fixture"
scoped = "phenix.session.s1"

def selected(spec, slot):
    found = [
        value for value in spec["slots"]
        if value["caller"] == adapter and value["slot"] == slot
    ]
    assert len(found) == 1, found
    return found[0]["provider"]

assert selected(source, "Upstream") == "phenix.model.scripted"
assert not [
    value for value in source["slots"]
    if value["caller"] == adapter and value["slot"] == "Meter"
], "default Advanced preset must stay unmetered"
spec = json.loads(json.dumps(source))
spec["artifacts"].append(meter_provider)
for binding in spec["slots"]:
    if binding["caller"] == adapter and binding["slot"] == "Upstream":
        binding["provider"] = model
spec["slots"].append({
    "caller": adapter,
    "slot": "Meter",
    "index": 0,
    "provider": model,
    "export": "Meter",
})
spec["grants"].append({
    "caller": adapter,
    "interface": "phenix.advanced.model-tokens@1",
    "operation": "measure",
    "resources": [scoped],
})
spec["authority"]["operations"].append("phenix.advanced.model-tokens@1/measure")
assert selected(spec, "Upstream") == selected(spec, "Meter") == model
path = out / "metered-composition.json"
path.write_text(json.dumps(spec))

def prompt(text, limit=512):
    return {
        "type": "prompt", "session": "s1", "text": text,
        "limits": {"model_tokens": limit},
    }

def run(messages, composition=path, expected_error=None, expected_failure_text=None, database=None):
    if database is None:
        database = "metered.sqlite" if composition == path else f"{composition.stem}.sqlite"
    env = dict(os.environ, PHENIX_BASIC_SQLITE_PATH=str(out / database))
    proc = subprocess.run(
        [runtime, "jsonl", str(composition)],
        input="".join(json.dumps(item) + "\n" for item in messages),
        text=True, capture_output=True, env=env, timeout=90,
    )
    if expected_error is None:
        assert proc.returncode == 0, (proc.stdout, proc.stderr)
    else:
        # Invalid replies and kernel denials exit nonzero. Some domain turn
        # rejections instead return a successful JSONL command that contains
        # the durable Failure event. In either case no Answer may be emitted.
        assert proc.returncode != 0, (proc.stdout, proc.stderr)
        assert expected_error in proc.stderr, (proc.stdout, proc.stderr)
    events = [json.loads(line) for line in proc.stdout.splitlines()]
    if expected_error is not None or expected_failure_text is not None:
        assert not [event for event in events if event["kind"] == "Answer"], events
        # Kernel authority denial aborts the imported effect before the Basic
        # agent can journal a Failure. Agent-level Model errors instead write
        # the durable Failure event, including invalid replies and turn caps.
        if expected_error != "has no effective grant":
            assert any(event["kind"] == "Failure" for event in events), events
    if expected_failure_text is not None:
        assert any(
            event["kind"] == "Failure" and expected_failure_text in event["text"]
            for event in events
        ), (events, proc.stderr)
    return events

accepted = run([prompt("hello")])
assert [e["text"] for e in accepted if e["kind"] == "Answer"] == [
    "fixture:hello"
], accepted

# Selection and ancestor declarations are ceilings, never independent grants.
# Removing the adapter's explicit meter grant must block its child import,
# even though the provider remains selected and the root permits the operation.
ungranted = json.loads(json.dumps(spec))
ungranted["grants"] = [
    grant for grant in ungranted["grants"]
    if not (
        grant["caller"] == adapter
        and grant["interface"] == "phenix.advanced.model-tokens@1"
        and grant["operation"] == "measure"
    )
]
ungranted_path = out / "meter-denied-composition.json"
ungranted_path.write_text(json.dumps(ungranted))
no_grant = run([prompt("no-meter-grant")], composition=ungranted_path, expected_error="has no effective grant")
assert not [e for e in no_grant if e["kind"] == "Answer"], no_grant
assert any(e["kind"] == "ModelStarted" for e in no_grant), no_grant

# RED P2-3m: a declared and granted meter is insufficient if the selected
# upstream Model resolves to a different provider. The scripted Model would
# otherwise produce a normal answer with this valid fixture measurement.
mismatched = json.loads(json.dumps(spec))
for binding in mismatched["slots"]:
    if binding["caller"] == adapter and binding["slot"] == "Upstream":
        binding["provider"] = "phenix.model.scripted"
assert selected(mismatched, "Meter") != selected(mismatched, "Upstream")
mismatched_path = out / "mismatched-model-meter-composition.json"
mismatched_path.write_text(json.dumps(mismatched))
different_provider = run(
    [prompt("wrong-provider")],
    composition=mismatched_path,
    expected_error="InvalidReply",
)
assert not [e for e in different_provider if e["kind"] == "Answer"], different_provider
assert any(
    e["kind"] == "Failure" and "selected Model provider" in e["text"]
    for e in different_provider
), different_provider

# This token-meter fixture returns an invalid digest for the next prompt.
# The direct Model would still return a fixture answer, so a failure proves
# the adapter actually invoked the meter before its Model import.
invalid = run([prompt("meter-stale")], expected_error="InvalidReply")
assert not [e for e in invalid if e["kind"] == "Answer"], invalid
assert any(e["kind"] == "Failure" for e in invalid), invalid

# Without explicit output reserve, the opted-in measured model must fail
# rather than invent a context budget.
missing_limit = {"type": "prompt", "session": "s1", "text": "no-limit"}
no_reserve = run([missing_limit], expected_failure_text="requires an explicit positive token budget")
assert not [e for e in no_reserve if e["kind"] == "Answer"], no_reserve
assert any(e["kind"] == "Failure" for e in no_reserve), no_reserve

# The exact preflight must also account for the selected Model's output
# reservation, not only the observed input length. This call's Model would
# otherwise generate a short valid fixture answer.
over_budget = run([prompt("reserve-too-large", limit=65_535)], expected_failure_text="selected Model token measurement rejected")
assert not [e for e in over_budget if e["kind"] == "Answer"], over_budget
assert any(e["kind"] == "Failure" for e in over_budget), over_budget

after = run([prompt("recovered")])
assert [e["text"] for e in after if e["kind"] == "Answer"] == [
    "fixture:recovered"
], after

# RED P2-3o: the Advanced Model adapter may opt into an authenticated
# Journal import to persist the exact assistant tool-call arguments before
# Basic executes tools. The public Advanced preset stays unchanged.
provenance = json.loads(json.dumps(source))
assert not [
    slot for slot in source["slots"]
    if slot["caller"] == adapter and slot["slot"] == "Transcript"
]
provenance["slots"].append({
    "caller": adapter,
    "slot": "Transcript",
    "index": 0,
    "provider": "phenix.basic.event-journal",
    "export": "Journal",
})
provenance_path = out / "tool-reply-provenance-composition.json"
provenance_path.write_text(json.dumps(provenance))
tool_events = run([prompt("tools:hello")], composition=provenance_path)
assert [e["text"] for e in tool_events if e["kind"] == "Answer"] == [
    "mock:call-1:HELLO|call-2:5"
], tool_events
receipts = [e for e in tool_events if e["kind"] == "AssistantReply"]
assert len(receipts) == 1 and receipts[0]["data"], tool_events
assert receipts[0]["text"] == "", receipts
assert receipts[0]["sequence"] < min(
    e["sequence"] for e in tool_events if e["kind"] == "ToolStarted"
), tool_events
restarted = run([prompt("after-tools")], composition=provenance_path)
assert [e["text"] for e in restarted if e["kind"] == "Answer"] == [
    "mock:after-tools"
], restarted
assert not [e for e in restarted if e["kind"] == "AssistantReply"], restarted

# A selected Transcript without its own grant must not record a receipt or
# allow tool execution. Ancestor ceiling and root authority do not suffice.
provenance_denied = json.loads(json.dumps(provenance))
provenance_denied["grants"] = [
    grant for grant in provenance_denied["grants"]
    if not (
        grant["caller"] == adapter
        and grant["interface"] == "phenix.basic.journal@1"
        and grant["operation"] == "append"
    )
]
denied_path = out / "tool-reply-provenance-denied-composition.json"
denied_path.write_text(json.dumps(provenance_denied))
denied_events = run(
    [prompt("tools:denied")],
    composition=denied_path,
    expected_error="has no effective grant",
)
assert not [e for e in denied_events if e["kind"] in (
    "Answer", "AssistantReply", "ToolStarted"
)], denied_events

# RED P2-3q: a fully opted-in Advanced composition must submit a
# source-verified, token-metered compacted ModelRequest. A distinct native
# kernel commits a checkpoint over actual Basic journal bytes.
live = json.loads(json.dumps(spec))
live["slots"] += [
    {"caller": adapter, "slot": "Transcript", "index": 0,
     "provider": "phenix.basic.event-journal", "export": "Journal"},
    {"caller": adapter, "slot": "Checkpoints", "index": 0,
     "provider": "phenix.advanced.compaction-checkpoints", "export": "Checkpoints"},
]
live["grants"] += [
    {"caller": adapter, "interface": "phenix.basic.journal@1",
     "operation": "read", "resources": [scoped]},
    {"caller": adapter, "interface": "phenix.advanced.compaction-checkpoints@1",
     "operation": "latest", "resources": [scoped]},
]
live_path = out / "live-verified-compaction-composition.json"
live_path.write_text(json.dumps(live))
seed_events = run(
    [prompt("seed-one"), prompt("seed-two")],
    composition=live_path,
)
assert [event["text"] for event in seed_events if event["kind"] == "Answer"] == [
    "fixture:seed-one", "fixture:seed-two",
], seed_events
live_db = out / f"{live_path.stem}.sqlite"
seed_folder = out / "live-checkpoint-seed"
seed = subprocess.run(
    [checkpoint_runner, checkpoint_provider, journal_provider, storage_provider,
     basic_definitions, advanced_definitions, "live-seed", str(seed_folder)],
    capture_output=True, text=True, timeout=90,
    env=dict(os.environ, PHENIX_BASIC_SQLITE_PATH=str(live_db)),
)
assert seed.returncode == 0, (seed.stdout, seed.stderr)
assert json.loads((seed_folder / "compaction-product.json").read_text())[
    "live_checkpoint"
] == "cp-live"
live_events = run([prompt("inspect-compaction")], composition=live_path)
assert [event["text"] for event in live_events if event["kind"] == "Answer"] == [
    "fixture:history:3:compaction:true"
], live_events

def fork_live_database(stem):
    target = out / f"{stem}.sqlite"
    with sqlite3.connect(live_db) as origin, sqlite3.connect(target) as fork:
        origin.backup(fork)
    return target.name

# Opt-in source reads require an explicit caller grant; discovery/selection
# cannot grant journal access. Deny before the Model sees a compacted request.
no_read = json.loads(json.dumps(live))
no_read["grants"] = [g for g in no_read["grants"] if not (
    g["caller"] == adapter
    and g["interface"] == "phenix.basic.journal@1"
    and g["operation"] == "read"
)]
no_read_path = out / "live-compaction-read-denied-composition.json"
no_read_path.write_text(json.dumps(no_read))
denied_read = run(
    [prompt("inspect-compaction")],
    composition=no_read_path,
    expected_error="has no effective grant",
    database=fork_live_database("journal-read-denied"),
)
assert not [event for event in denied_read if event["kind"] == "Answer"], denied_read

# P2-3x positive control: the un-compacted request cannot fit the
# selected Model window, but the cited, rebuilt request can. This must
# continue through the real Advanced adapter rather than denying early.
capacity_fork = fork_live_database("dual-meter-overwindow")
capacity_events = run(
    [prompt("inspect-original-overwindow")], composition=live_path,
    database=capacity_fork,
)
assert [e["text"] for e in capacity_events if e["kind"] == "Answer"] == [
    "fixture:inspect-original-overwindow"
], capacity_events

# P2-3x RED: the same selected Model must measure both the original
# authoritative request and its source-rebuilt compacted counterpart.
# These fault probes act only inside the verification provider and use
# independent SQLite forks. The direct Model would otherwise answer them.
original_meter_fork = fork_live_database("dual-meter-original-stale")
original_invalid = run(
    [prompt("inspect-original-stale")], composition=live_path,
    database=original_meter_fork, expected_error="InvalidReply",
)
assert not [e for e in original_invalid if e["kind"] == "Answer"], original_invalid

configuration_fork = fork_live_database("dual-meter-config-drift")
changed_configuration = run(
    [prompt("inspect-config-drift")], composition=live_path,
    database=configuration_fork, expected_error="InvalidReply",
)
assert not [e for e in changed_configuration if e["kind"] == "Answer"], changed_configuration

# P2-3r: an already persisted checkpoint cannot become optional because
# its provider denies the selected caller or its durable blob is corrupt.
# Fork the same SQLite state so every failure is observed on an independent
# native restart without perturbing a successful session.
no_checkpoint_grant = json.loads(json.dumps(live))
no_checkpoint_grant["grants"] = [g for g in no_checkpoint_grant["grants"] if not (
    g["caller"] == adapter
    and g["interface"] == "phenix.advanced.compaction-checkpoints@1"
    and g["operation"] == "latest"
)]
no_checkpoint_grant_path = out / "live-checkpoint-grant-denied-composition.json"
no_checkpoint_grant_path.write_text(json.dumps(no_checkpoint_grant))
denied_checkpoint = run(
    [prompt("inspect-compaction")], composition=no_checkpoint_grant_path,
    database=fork_live_database("checkpoint-grant-denied"),
    expected_error="has no effective grant",
)
assert not [e for e in denied_checkpoint if e["kind"] == "Answer"], denied_checkpoint

no_selected_meter = json.loads(json.dumps(live))
no_selected_meter["slots"] = [s for s in no_selected_meter["slots"] if not (
    s["caller"] == adapter and s["slot"] == "Meter"
)]
no_selected_meter_path = out / "live-no-selected-meter-composition.json"
no_selected_meter_path.write_text(json.dumps(no_selected_meter))
denied_unmetered = run(
    [prompt("inspect-compaction")], composition=no_selected_meter_path,
    database=fork_live_database("no-selected-meter"),
    expected_error="InvalidReply",
)
assert not [e for e in denied_unmetered if e["kind"] == "Answer"], denied_unmetered

corrupted_db = out / fork_live_database("corrupt-checkpoint")
with sqlite3.connect(corrupted_db) as connection:
    changed = connection.execute(
        "UPDATE basic_records SET data = ? WHERE session = ? AND name = ?",
        (bytes([0]), "s1", "compaction-checkpoints.v1"),
    ).rowcount
    assert changed == 1, changed
    connection.commit()
denied_corrupt = run(
    [prompt("inspect-compaction")], composition=live_path,
    database=corrupted_db.name, expected_error="InvalidReply",
)
assert not [e for e in denied_corrupt if e["kind"] == "Answer"], denied_corrupt

# An unrelated denied process must not mutate the original journal, checkpoint
# or selected-model behavior after another native process restart.
continued = run([prompt("inspect-compaction")], composition=live_path)
assert [e["text"] for e in continued if e["kind"] == "Answer"] == [
    "fixture:history:5:compaction:true"
], continued

# P2-3s RED: the checkpoint spans more than a single 128-event
# journal page. A native checkpoint writer and the selected Model adapter
# must both read all contiguous source pages, never truncate at page one.
page_turns = 36
page_prompts = [prompt("seed-one")] + [
    prompt(f"page-{n:02d}") for n in range(1, page_turns)
]
page_database = "paginated-checkpoint.sqlite"
page_events = run(
    page_prompts, composition=live_path, database=page_database,
)
assert len([e for e in page_events if e["kind"] == "Answer"]) == page_turns
assert len(page_events) > 128, len(page_events)
page_seed_folder = out / "paginated-checkpoint-seed"
page_seed = subprocess.run(
    [checkpoint_runner, checkpoint_provider, journal_provider, storage_provider,
     basic_definitions, advanced_definitions, "live-seed", str(page_seed_folder)],
    capture_output=True, text=True, timeout=90,
    env=dict(os.environ, PHENIX_BASIC_SQLITE_PATH=str(out / page_database)),
)
assert page_seed.returncode == 0, (page_seed.stdout, page_seed.stderr)
assert json.loads((page_seed_folder / "compaction-product.json").read_text())[
    "live_checkpoint"
] == "cp-live"
page_continuation = run(
    [prompt("inspect-compaction")], composition=live_path,
    database=page_database,
)
assert [event["text"] for event in page_continuation if event["kind"] == "Answer"] == [
    f"fixture:history:{2 * (page_turns - 1) + 1}:compaction:true"
], page_continuation

# P2-3aa RED: the existing Basic Model graph call may use an opt-in
# selected summarizer to create a source-backed CAS checkpoint, remeasure
# the exact compacted request and continue through the selected Model.
# No model/tool loop is added, and the default Advanced preset stays unchanged.
automatic = json.loads(json.dumps(live))
automatic["slots"].append({
    "caller": adapter, "slot": "Summarizer", "index": 0,
    "provider": model, "export": "Summarizer",
})
automatic["grants"] += [
    {"caller": adapter, "interface": "phenix.advanced.compaction-summary@1",
     "operation": "summarize", "resources": [scoped]},
    {"caller": adapter, "interface": "phenix.advanced.compaction-checkpoints@1",
     "operation": "put", "resources": [scoped]},
]
automatic["authority"]["operations"] += [
    "phenix.advanced.compaction-summary@1/summarize",
    "phenix.advanced.compaction-checkpoints@1/put",
]
auto_path = out / "live-automatic-compaction-composition.json"
auto_path.write_text(json.dumps(automatic))
auto_db = "live-automatic-compaction.sqlite"
auto_seed = run(
    [prompt("seed-one"), prompt("seed-two"), prompt("seed-three")],
    composition=auto_path, database=auto_db,
)
assert [e["text"] for e in auto_seed if e["kind"] == "Answer"] == [
    "fixture:seed-one", "fixture:seed-two", "fixture:seed-three"
], auto_seed
# Denied imports must not create a checkpoint or execute the selected
# Model. Each test starts from the identical completed source turns.
def fork_auto_source(name):
    destination = out / f"{name}.sqlite"
    with sqlite3.connect(out / auto_db) as source, sqlite3.connect(destination) as target:
        source.backup(target)
    return destination.name

no_summary_grant = json.loads(json.dumps(automatic))
no_summary_grant["grants"] = [g for g in no_summary_grant["grants"] if not (
    g["caller"] == adapter
    and g["interface"] == "phenix.advanced.compaction-summary@1"
    and g["operation"] == "summarize"
)]
no_summary_path = out / "auto-summary-grant-denied.json"
no_summary_path.write_text(json.dumps(no_summary_grant))
no_summary_events = run(
    [prompt("auto-compact")], composition=no_summary_path,
    database=fork_auto_source("auto-summary-grant-denied"),
    expected_error="has no effective grant",
)
assert not [e for e in no_summary_events if e["kind"] == "Answer"], no_summary_events

no_put_grant = json.loads(json.dumps(automatic))
no_put_grant["grants"] = [g for g in no_put_grant["grants"] if not (
    g["caller"] == adapter
    and g["interface"] == "phenix.advanced.compaction-checkpoints@1"
    and g["operation"] == "put"
)]
no_put_path = out / "auto-put-grant-denied.json"
no_put_path.write_text(json.dumps(no_put_grant))
no_put_events = run(
    [prompt("auto-compact")], composition=no_put_path,
    database=fork_auto_source("auto-put-grant-denied"),
    expected_error="has no effective grant",
)
assert not [e for e in no_put_events if e["kind"] == "Answer"], no_put_events

bad_summary_db = fork_auto_source("auto-summary-citation-forged")
bad_summary_events = run(
    [prompt("auto-invalid-citation")], composition=auto_path,
    database=bad_summary_db, expected_error="InvalidReply",
)
assert not [e for e in bad_summary_events if e["kind"] == "Answer"], bad_summary_events
with sqlite3.connect(out / bad_summary_db) as connection:
    assert connection.execute(
        "SELECT count(*) FROM basic_records WHERE session = 's1' "
        "AND name = 'compaction-checkpoints.v1'"
    ).fetchone() == (0,), "forged citation created checkpoint"

auto_events = run(
    [prompt("auto-compact")], composition=auto_path, database=auto_db,
)
assert [e["text"] for e in auto_events if e["kind"] == "Answer"] == [
    "fixture:auto:history:3:compaction:true"
], auto_events
with sqlite3.connect(out / auto_db) as connection:
    checkpoint_rev = connection.execute(
        "SELECT revision FROM basic_records WHERE session = 's1' "
        "AND name = 'compaction-checkpoints.v1'"
    ).fetchone()
    assert checkpoint_rev == (1,), checkpoint_rev
auto_restart = run(
    [prompt("auto-compact")], composition=auto_path, database=auto_db,
)
assert [e["text"] for e in auto_restart if e["kind"] == "Answer"] == [
    "fixture:auto:history:5:compaction:true"
], auto_restart
with sqlite3.connect(out / auto_db) as connection:
    assert connection.execute(
        "SELECT revision FROM basic_records WHERE session = 's1' "
        "AND name = 'compaction-checkpoints.v1'"
    ).fetchone() == (1,), "restart must not create a duplicate checkpoint"

# P2-3ab RED: the next selected-model pressure may require a second
# checkpoint revision. It must verify the prior checkpoint ancestry,
# preserve the full original journal and publish a monotone CAS revision.
incremental_db = out / "auto-incremental-restart.sqlite"
with sqlite3.connect(out / auto_db) as source, sqlite3.connect(incremental_db) as target:
    source.backup(target)
before_incremental = None
with sqlite3.connect(incremental_db) as connection:
    before_incremental = connection.execute(
        "SELECT hex(data) FROM basic_records "
        "WHERE session = 's1' AND name = 'event-journal.v1'"
    ).fetchone()
incremental_events = run(
    [prompt("auto-compact-again")], composition=auto_path,
    database=incremental_db.name,
)
assert [e["text"] for e in incremental_events if e["kind"] == "Answer"] == [
    "fixture:auto2:history:3:compaction:true"
], incremental_events
with sqlite3.connect(incremental_db) as connection:
    assert connection.execute(
        "SELECT revision FROM basic_records "
        "WHERE session = 's1' AND name = 'compaction-checkpoints.v1'"
    ).fetchone() == (2,), "second pressure did not advance checkpoint CAS"
    checkpoint_v2 = connection.execute(
        "SELECT hex(data) FROM basic_records "
        "WHERE session = 's1' AND name = 'compaction-checkpoints.v1'"
    ).fetchone()[0]
    journal_size_v2 = connection.execute(
        "SELECT length(data) FROM basic_records "
        "WHERE session = 's1' AND name = 'event-journal.v1'"
    ).fetchone()[0]

# This is a distinct submitted user turn after a native restart. Its
# expanded retained history again exceeds the deterministic fixture limit,
# so revision 3 is legitimate. Exact same-candidate CAS retry idempotence is
# proved separately by the CompactionStore publication tests.
incremental_restart = run(
    [prompt("auto-compact-again")], composition=auto_path,
    database=incremental_db.name,
)
assert [e["text"] for e in incremental_restart if e["kind"] == "Answer"] == [
    "fixture:auto2:history:3:compaction:true"
], incremental_restart
with sqlite3.connect(incremental_db) as connection:
    assert connection.execute(
        "SELECT revision FROM basic_records "
        "WHERE session = 's1' AND name = 'compaction-checkpoints.v1'"
    ).fetchone() == (3,), "new pressured turn must advance the linear checkpoint CAS"
    assert connection.execute(
        "SELECT hex(data) FROM basic_records "
        "WHERE session = 's1' AND name = 'compaction-checkpoints.v1'"
    ).fetchone()[0] != checkpoint_v2
    assert connection.execute(
        "SELECT length(data) FROM basic_records "
        "WHERE session = 's1' AND name = 'event-journal.v1'"
    ).fetchone()[0] > journal_size_v2
assert before_incremental is not None

(out / "accepted.json").write_text(json.dumps({
    "format": "phenix.p2.metered-model-adapter.v1",
    "accepted": True,
    "cases": [
        "default-advanced-composition-remains-unmetered",
        "same-provider-selected-for-meter-and-model",
        "good-preflight-before-upstream-Model",
        "no-explicit-meter-grant-denied",
        "cross-provider-token-meter-denied-before-model",
        "changed-request-digest-denied-before-model",
        "missing-output-reserve-denied",
        "input-plus-reserved-output-over-context-denied",
        "recovery-after-denial",
        "authorized-tool-reply-journal-receipt-before-execution",
        "journal-receipt-survives-sqlite-restart-without-duplicate",
        "missing-journal-grant-denies-before-tool-effect",
        "live-checkpoint-and-tail-model-request-is-metered",
        "overwindow-original-enters-compaction-and-fitted-request-succeeds",
        "original-uncompacted-model-measurement-must-be-source-authentic",
        "selected-tokenizer-drift-between-original-and-compact-denied",
        "live-checkpoint-read-requires-explicit-grant",
        "missing-checkpoint-grant-denies-after-sqlite-restart",
        "selected-meter-removal-denies-live-compaction",
        "corrupt-checkpoint-ledger-denies-without-model-fallback",
        "independent-denial-does-not-change-authoritative-session",
        "multi-page-checkpoint-source-and-live-continuation",
        "opt-in-selected-summarizer-creates-native-checkpoint-at-pressure",
        "automatic-cas-creates-one-revision-and-reuses-after-native-restart",
        "missing-summary-grant-denies-before-model",
        "missing-checkpoint-write-grant-denies-before-model",
        "forged-summary-source-citation-denies-before-CAS",
        "automatic-second-checkpoint-CAS-under-repeat-pressure",
        "new-pressured-turn-after-restart-advances-checkpoint-lineage",
    ],
}, indent=2, sort_keys=True))
