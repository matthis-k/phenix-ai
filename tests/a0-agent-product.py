"""Independent CLI and canonical v1 request producer for the A0 plugin loop."""
import json
import subprocess
import sys
from pathlib import Path

kernel, native_output, output = map(Path, sys.argv[1:])
output.mkdir(parents=True)
policy = json.loads((native_output / "composition.json").read_text())


def encode(value):
    if isinstance(value, str):
        payload = value.encode("utf-8")
        return bytes([5]) + len(payload).to_bytes(4, "big") + payload
    if isinstance(value, int):
        return bytes([4]) + value.to_bytes(8, "big")
    items = sorted(value.items())
    return bytes([8]) + len(items).to_bytes(4, "big") + b"".join(
        len(key.encode()).to_bytes(4, "big") + key.encode() + encode(item)
        for key, item in items
    )


def run(prompt, limit=0, selected_policy=None, expected_exit=0):
    deployment = output / "deployment.json"
    request = output / "request.bin"
    deployment.write_text(json.dumps(selected_policy or policy))
    request.write_bytes(encode(dict(prompt=prompt, max_turns=limit)))
    result = subprocess.run(
        [str(kernel), "invoke", str(deployment), str(request)],
        text=True, capture_output=True,
    )
    assert result.returncode == expected_exit, (result.stdout, result.stderr)
    return json.loads(result.stdout)


offline = json.loads(subprocess.check_output(
    [str(kernel), "inspect", str(native_output / "composition.json")], text=True,
))
assert all(p["offline_verified"] and not p["abi_negotiated"] for p in offline["plugins"])
reports = {}
for prompt, answer, turns, tools in [
    ("phenix", "mock:PHENIX", 2, 1),
    ("repeat", "mock:REPEAT", 21, 20),
    ("tool-error", "recovered", 2, 1),
]:
    report = run(prompt)
    assert report["outcome"] == dict(kind="success", value=dict(
        answer=answer, model_turns=turns, tool_calls=tools,
    ))
    assert report["calls"] == 1 + turns + tools
    assert report["roots"]["completed"] == 1
    reports[prompt] = report
limited = run("phenix", limit=1)
assert limited["outcome"] == dict(kind="domain_error", value="TurnLimit"), limited
reports["turn-limit"] = limited
denied_policy = dict(policy, grants=[g for g in policy["grants"] if not (
    g["caller"] == "example.agent" and g["interface"] == "example.tool@1"
)])
denied = run("phenix", selected_policy=denied_policy, expected_exit=1)
assert denied["outcome"]["kind"] == "kernel_failure"
assert denied["outcome"]["failure"]["code"] == "Denied"
assert denied["calls"] == 2 and denied["denied_operations"] == 1
reports["denied-tool"] = denied
records = {p["id"]: p for p in offline["plugins"]}
assert len(records) == 4
for record in records.values():
    assert len(record["artifact_sha256"]) == 64
    assert len(record["descriptor_sha256"]) == 64
(output / "product.json").write_text(json.dumps(dict(
    format="phenix.a0.product.v1", offline=offline, reports=reports,
), sort_keys=True, indent=2))
print("A0 independent agent CLI preflight passed")
