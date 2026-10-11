"""Independent canonical v2 producer and real packaged CLI assertions."""
import copy
import hashlib
import json
import subprocess
import sys
from pathlib import Path

kernel, native, output = map(Path, sys.argv[1:4])
output.mkdir(parents=True, exist_ok=True)
policy = json.loads((native / "composition.json").read_text())


def encode(value):
    if value is None:
        return b"\0"
    if isinstance(value, bool):
        return bytes([2 if value else 1])
    if isinstance(value, int):
        return b"\x04" + value.to_bytes(8, "big")
    if isinstance(value, str):
        payload = value.encode()
        return b"\x05" + len(payload).to_bytes(4, "big") + payload
    if isinstance(value, bytes):
        return b"\x06" + len(value).to_bytes(4, "big") + value
    if isinstance(value, list):
        return b"\x07" + len(value).to_bytes(4, "big") + b"".join(map(encode, value))
    items = sorted(value.items())
    return b"\x08" + len(items).to_bytes(4, "big") + b"".join(
        len(key.encode()).to_bytes(4, "big") + key.encode() + encode(item)
        for key, item in items
    )


def schema_id(schema):
    return hashlib.sha256(b"phenix.schema.v2\0" + encode(schema)).digest()


def refs():
    return dict(contract="test.refs@1", revision=1, operations=[
        dict(name="get", request=encode("String"),
             response=encode(["Ref", schema_id("U64")]), error=encode("String")),
        dict(name="notes", request=encode("String"),
             response=encode(["Ref", schema_id(["List", "String"])]), error=encode("String")),
    ])


def definition(contract, operations):
    return dict(contract=contract, revision=1, operations=[
        dict(name=name, request=encode(request), response=encode(response), error=encode("String"))
        for name, request, response in operations
    ])


def export(slot, contract):
    return dict(slot=slot, contract=contract, revision=1)


def require(slot, contract, cardinality="one"):
    return dict(slot=slot, contract=contract, revision_min=1, revision_max=1,
                cardinality=cardinality, minimum=1)


provider = dict(format="phenix.contract-metadata.v2", plugin="test.state-provider",
                version="1.0.0", execution="state-pending", definitions=[
                    definition("test.provider@1", [("run", "String", ["List", "U64"])]), refs(),
                ], provides=[export("refs", "test.refs@1"), export("test", "test.provider@1")],
                requires=[], properties=[
                    dict(id="counter", schema=encode("U64"), get=True, set=True, watch=True),
                    dict(id="notes", schema=encode(["List", "String"]), get=True, set=True, watch=True),
                ])
client = dict(format="phenix.contract-metadata.v2", plugin="test.state-client",
              version="1.0.0", execution="state-pending", definitions=[refs(), definition(
                  "test.state@1", [("read", ["Ref", schema_id("U64")], "U64"),
                                   ("run", "String", ["List", "U64"])],
              )], provides=[export("test", "test.state@1")], requires=[
                  require("counter", "test.state-provider.property.counter@1"),
                  require("many", "test.refs@1", "many"),
                  require("notes", "test.state-provider.property.notes@1"),
                  require("refs", "test.refs@1"),
              ], properties=[])
metadata_hashes = {}
alternative=copy.deepcopy(provider)
alternative["plugin"]="test.state-alternative"
for expected in [provider, client, alternative]:
    package = next(Path(p) for p in policy["composition"]["artifacts"]
                   if Path(p, "descriptor.bin").read_bytes() == b"PV\x02\x00" + encode(expected))
    record = json.loads((package / "package.json").read_text())
    assert record["format"] == "phenix.native-package.v2"
    actual = (package / record["descriptor"]).read_bytes()
    assert hashlib.sha256(actual).hexdigest() == record["descriptor_sha256"]
    metadata_hashes[expected["plugin"]] = record["descriptor_sha256"]

if "--metadata-only" in sys.argv:
    (output / "metadata.json").write_text(json.dumps(metadata_hashes, indent=2))
    print("S2 independent native metadata vectors passed")
    sys.exit(0)

offline = json.loads(subprocess.check_output([
    str(kernel), "inspect", str(native / "composition.json"),
], text=True))
assert all(p["offline_verified"] and not p["abi_negotiated"] for p in offline["plugins"])
assert offline["calls"] == 0


def run(command, selected=None, expected_exit=0, raw=None):
    deployment = output / "deployment.json"
    request = output / "request.bin"
    deployment.write_text(json.dumps(selected or policy))
    request.write_bytes(raw if raw is not None else b"PV\x02\x00" + encode(command))
    result = subprocess.run([str(kernel), "invoke", str(deployment), str(request)],
                            text=True, capture_output=True)
    assert result.returncode == expected_exit, (result.stdout, result.stderr)
    return json.loads(result.stdout)


reports = {}
for command, expected in [("basics", [1, 0, 0, 1, 1, 1, 0]),
                          ("watch", [0, 0, 2, 2, 1, 1]),
                          ("batch", [1, 1, 1, 1]), ("many", [2, 1])]:
    report = run(command)
    assert report["outcome"] == dict(kind="success", value=expected), report
    assert report["live_state"]["cells"] == report["live_state"]["subscriptions"] == 0
    reports[command] = report
for command, error in [("bad-batch", "Conflict"), ("schema", "SchemaMismatch"),
                       ("owner", "OwnerGone"), ("cell", "StaleReference"), ("many-ref-denied", "Denied")]:
    report = run(command)
    assert report["outcome"] == dict(kind="domain_error", value=error), report
    reports[command] = report
denied = copy.deepcopy(policy)
denied["composition"]["grants"] = [g for g in denied["composition"]["grants"]
    if not (g["caller"] == "test.state-client" and g["interface"].endswith(".counter@1")
            and g["operation"] == "set")]
report = run("denied", denied)
assert report["outcome"] == dict(kind="domain_error", value="Denied"), report
reports["denied-write"] = report
bad_wire = run("basics", raw=encode("basics"), expected_exit=1)
assert bad_wire["phase"] == "offline_preparation", bad_wire
reports["v1-wire-refused"] = bad_wire
(output / "product.json").write_text(json.dumps(dict(
    format="phenix.m1-state-product.v1", metadata_hashes=metadata_hashes,
    offline=offline, cases=reports,
), indent=2))
print("S2 independent metadata and packaged CLI gate passed")
