"""P2-3l RED: a selected Advanced adapter must measure before calling Model.

The Model and ModelTokenMeter slots point at the same fixture provider.
The default public Advanced composition has no Meter slot and stays unchanged.
"""
import json
import os
from pathlib import Path
import subprocess
import sys

runtime, advanced_json, meter_provider, destination = sys.argv[1:]
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

def run(messages, composition=path, expected_error=None, expected_failure_text=None):
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
    ],
}, indent=2, sort_keys=True))
