"""Native Workspace/Storage contract test across process restarts."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

kernel, workspace, storage, workdefs, basicdefs, destination = sys.argv[1:]
out = Path(destination)
out.mkdir(parents=True, exist_ok=True)
work_contract = "phenix.workspace@1"
storage_contract = "phenix.basic.storage@1"
resource = "phenix.session.s1"


def encode(value):
    if value is None:
        return b"\x00"
    if isinstance(value, str):
        raw = value.encode("utf-8")
        return b"\x05" + len(raw).to_bytes(4, "big") + raw
    if isinstance(value, list):
        return b"\x07" + len(value).to_bytes(4, "big") + b"".join(
            encode(item) for item in value
        )
    if isinstance(value, int):
        return b"\x04" + value.to_bytes(8, "big")
    if isinstance(value, dict):
        pairs = sorted(value.items())
        return b"\x08" + len(pairs).to_bytes(4, "big") + b"".join(
            len(key.encode()).to_bytes(4, "big") + key.encode() + encode(item)
            for key, item in pairs
        )
    raise TypeError(type(value))


with tempfile.TemporaryDirectory(prefix="phenix-workspace-") as temporary:
    root = Path(temporary)
    env = dict(os.environ, PHENIX_BASIC_SQLITE_PATH=str(root / "records.sqlite"))

    def invoke(operation, request, grant=resource):
        spec = dict(
            format="phenix.composition.v3",
            entry=dict(interface=work_contract, operation=operation, provider="phenix.workspace.selection"),
            artifacts=[workspace, storage],
            definitions=[workdefs, basicdefs],
            slots=[dict(caller="phenix.workspace.selection", slot="Storage", index=0,
                        provider="phenix.basic.storage-sqlite", export="Storage")],
            grants=[
                dict(caller="root", interface=work_contract, operation=operation,
                     resources=[grant]),
                *[dict(caller="phenix.workspace.selection", interface=storage_contract,
                       operation=op, resources=[grant]) for op in ("read", "transact")],
            ],
            excluded=[],
            substitutions=[],
            authority=dict(
                operations=[work_contract + "/" + operation] +
                           [storage_contract + "/" + op for op in ("read", "transact")],
                resources=[grant],
            ),
            resource=resource,
        )
        (root / "composition.json").write_text(json.dumps(spec))
        request = dict(request)
        if request.get("cwd") is not None:
            # Portable Option<T> encodes Some(value) as a one-element list.
            request["cwd"] = [request["cwd"]]
        (root / "request.bin").write_bytes(b"PV\x02\0" + encode(request))
        proc = subprocess.run(
            [kernel, "invoke", str(root / "composition.json"), str(root / "request.bin")],
            env=env, cwd=root, capture_output=True, text=True, timeout=30,
        )
        report = json.loads(proc.stdout)
        return report.get("outcome"), proc.returncode

    original, status = invoke("get", dict(session="s1"))
    assert status == 0 and original == dict(kind="success", value=dict(
        session="s1", cwd=None, revision=0,
    )), original
    applied, status = invoke("set", dict(session="s1", cwd="/work/tree", expected_revision=0))
    assert status == 0 and applied == dict(kind="success", value=dict(
        session="s1", cwd=["/work/tree"], revision=1,
    )), applied
    restored, status = invoke("get", dict(session="s1"))
    assert status == 0 and restored == applied, restored

    stale, status = invoke("set", dict(session="s1", cwd="/work/other", expected_revision=0))
    assert status == 0 and stale["kind"] == "domain_error", stale
    assert stale["value"]["problem"] == "Conflict", stale

    malformed, status = invoke("set", dict(session="s1", cwd="../outside", expected_revision=1))
    assert status == 0 and malformed["kind"] == "domain_error", malformed
    assert malformed["value"]["problem"] == "InvalidInput", malformed

    denied, status = invoke("get", dict(session="s2"))
    assert status == 0 and denied["kind"] == "domain_error", denied
    assert denied["value"]["problem"] == "Denied", denied

    no_grant, status = invoke("get", dict(session="s1"), grant="phenix.session.s2")
    assert status != 0 and no_grant["kind"] == "kernel_failure", no_grant

    cleared, status = invoke("set", dict(session="s1", cwd=None, expected_revision=1))
    assert status == 0 and cleared == dict(kind="success", value=dict(
        session="s1", cwd=None, revision=2,
    )), cleared

    (out / "evidence.json").write_text(json.dumps(dict(
        contract=work_contract, cases=["new", "persist", "restart", "CAS conflict",
            "invalid path", "foreign session", "denied kernel grant", "cleared"],
    ), indent=2))
