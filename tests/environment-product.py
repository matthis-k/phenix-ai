"""Exercise the selected Environment provider through the real native kernel."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

kernel, package, definitions, destination = sys.argv[1:]
output = Path(destination)
output.mkdir(parents=True, exist_ok=True)
resource = "phenix.session.s1"
contract = "phenix.environment@1"
operation = "resolve"


def encode(value):
    if value is None:
        return b"\x00"
    if isinstance(value, str):
        raw = value.encode("utf-8")
        return b"\x05" + len(raw).to_bytes(4, "big") + raw
    if isinstance(value, dict):
        items = sorted(value.items())
        return b"\x08" + len(items).to_bytes(4, "big") + b"".join(
            len(key.encode()).to_bytes(4, "big") + key.encode() + encode(item)
            for key, item in items
        )
    raise TypeError(type(value))


with tempfile.TemporaryDirectory(prefix="phenix-environment-") as temporary:
    root = Path(temporary)
    env = dict(os.environ)
    env.update(
        HOME=str(root / "home"),
        XDG_CONFIG_HOME=str(root / "config"),
        XDG_STATE_HOME=str(root / "state"),
        XDG_DATA_HOME=str(root / "data"),
        XDG_CACHE_HOME=str(root / "cache"),
        XDG_RUNTIME_DIR=str(root / "run"),
    )

    def invoke(request, grant=resource):
        composition = dict(
            format="phenix.composition.v3",
            entry=dict(interface=contract, operation=operation, provider="phenix.environment.default"),
            artifacts=[package],
            definitions=[definitions],
            slots=[],
            grants=[dict(caller="root", interface=contract, operation=operation, resources=[grant])],
            excluded=[],
            substitutions=[],
            authority=dict(operations=[contract + "/" + operation], resources=[grant]),
            resource=resource,
        )
        (root / "composition.json").write_text(json.dumps(composition))
        (root / "request.bin").write_bytes(b"PV\x02\0" + encode(request))
        done = subprocess.run(
            [kernel, "invoke", str(root / "composition.json"), str(root / "request.bin")],
            cwd=root, env=env, capture_output=True, text=True, timeout=30,
        )
        return json.loads(done.stdout)["outcome"], done.returncode

    good, code = invoke(dict(resource=resource, namespace="sample.plugin"))
    assert code == 0 and good["kind"] == "success", good
    paths = good["value"]
    assert paths["resource"] == resource
    assert paths["launch_cwd"] == [str(root)], paths
    for field in ("config", "state", "data", "cache", "runtime"):
        # XDG_RUNTIME_DIR uses "run"; "runtime" is the contract field name.
        directory = "run" if field == "runtime" else field
        expected = str(root / directory / "phenix" / "sample.plugin")
        # Portable Option<String> wraps Some(value) in a one-element list.
        assert paths[field] == ([expected] if field == "runtime" else expected), paths
        assert not (root / field / "phenix").exists()

    wrong, code = invoke(dict(resource="phenix.session.s2", namespace="sample.plugin"))
    assert code == 0 and wrong["kind"] == "domain_error", wrong
    assert wrong["value"]["problem"] == "Denied", wrong

    traversal, code = invoke(dict(resource=resource, namespace="../escape"))
    assert code == 0 and traversal["kind"] == "domain_error", traversal
    assert traversal["value"]["problem"] == "InvalidInput", traversal

    denied, code = invoke(dict(resource=resource, namespace="sample.plugin"),
                          grant="phenix.session.other")
    assert code != 0 and denied["kind"] == "kernel_failure", denied

    env["XDG_CONFIG_HOME"] = "relative"
    fallback, code = invoke(dict(resource=resource, namespace="sample.plugin"))
    assert code == 0 and fallback["kind"] == "success", fallback
    assert fallback["value"]["config"] == str(
        root / "home" / ".config" / "phenix" / "sample.plugin"
    )

    (output / "evidence.json").write_text(
        json.dumps(dict(provider="phenix.environment.default", cases=[
            "xdg", "workspace", "no filesystem effects", "scope denial",
            "namespace traversal", "missing grant", "relative XDG fallback",
        ]), indent=2)
    )
