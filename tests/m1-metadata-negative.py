"""Independent portable encoder, metadata/package and state-envelope conformance."""
import copy
import hashlib
import json
import os
from pathlib import Path
import struct
import shlex
import subprocess
import sys
import tempfile

PREFIX = b"PV\x02\0"
class Ref:
    def __init__(self, cell=3):
        self.raw = bytes([1])*32 + bytes([2])*32 + bytes([cell])*16 + bytes([4])*32

def encode(v, prefix=True):
    def put(v):
        if v is None: return b"\0"
        if isinstance(v, bool): return bytes([2 if v else 1])
        if isinstance(v, int): return b"\4" + struct.pack(">Q", v)
        if isinstance(v, str):
            raw = v.encode(); return b"\5" + struct.pack(">I", len(raw)) + raw
        if isinstance(v, bytes): return b"\6" + struct.pack(">I", len(v)) + v
        if isinstance(v, list): return b"\7" + struct.pack(">I", len(v)) + b"".join(map(put, v))
        if isinstance(v, dict):
            return b"\10" + struct.pack(">I", len(v)) + b"".join(struct.pack(">I", len(k.encode())) + k.encode() + put(v[k]) for k in sorted(v))
        if isinstance(v, Ref): return b"\11" + v.raw
        raise TypeError(type(v))
    return (PREFIX if prefix else b"") + put(v)

def schema(v): return encode(v, False)
def definition():
    return dict(format="phenix.contract-metadata.v2", plugin="example.reactive-contracts", version="1.0.0", execution="none",
                definitions=[dict(contract="example.counter@1", revision=1, operations=[dict(name=name, request=schema("U64"), response=schema("U64"), error=schema(["Variants", ["Missing"]])) for name in ["read", "write"]]),
                             dict(contract="example.echo@1", revision=1, operations=[dict(name="echo", request=schema("String"), response=schema("String"), error=schema(["Variants", ["Missing"]]))])],
                requires=[], provides=[], properties=[])
def native():
    return dict(format="phenix.contract-metadata.v2", plugin="example.counter", version="1.0.0", execution="state-pending", definitions=[],
                requires=[dict(slot=slot, contract="example.counter@1", revision_min=1, revision_max=3, cardinality=kind, minimum=n) for slot, kind, n in [("all", "many", 0), ("fallback", "optional", 0), ("primary", "one", 1)]],
                provides=[dict(slot="service", contract="example.counter@1", revision=1)],
                properties=[dict(id="counter", schema=schema("U64"), get=True, set=True, watch=True), dict(id="history", schema=schema(["List", "String"]), get=True, set=False, watch=False)])

def native_sections(describe):
    """Inspect real ELF files without executing their trapping constructors."""
    cases = []
    def section(payload, size, length=None, padding=False):
        length = len(payload) if length is None else length
        data = bytearray(struct.pack(">I", length) + payload)
        data.extend(bytes(max(0, size-len(data))))
        if padding: data[-1] = 1
        return bytes(data[:size])
    small = encode(native())
    large = native()
    large["properties"] = [dict(id=f"property.{n:04}", schema=schema(["List", "String"]), get=True, set=True, watch=True) for n in range(256)]
    large = encode(large)
    assert 16*1024 < len(large) < 128*1024-4
    cases.extend([
        ("m1-16k", section(small, 16*1024), True, small),
        ("m1-128k", section(small, 128*1024), True, small),
        ("m1-large-payload", section(large, 128*1024), True, large),
        ("length-outside-section", section(small, 128*1024, 128*1024), False, None),
        ("length-overflow", section(small, 128*1024, 0xffffffff), False, None),
        ("zero-length", section(small, 128*1024, 0), False, None),
        ("nonzero-padding", section(small, 128*1024, padding=True), False, None),
        ("definitions-in-native", section(encode(definition()), 128*1024), False, None),
        ("m0-in-large-section", section(encode(None, False), 128*1024), False, None),
        ("truncated-payload", section(small[:-1], 128*1024), False, None),
    ])
    for size in [0, 4, 16383, 65536, 131073]:
        cases.append((f"wrong-section-size-{size}", section(small, size), False, None))
    bad = native(); bad["properties"].reverse()
    cases.append(("noncanonical-native", section(encode(bad), 128*1024), False, None))
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        for name, data, accepted, expected in cases:
            source = root / "fixture.c"
            source.write_text('__attribute__((constructor)) static void trap(void) { __builtin_trap(); }\n'
                              '__attribute__((used,section(".phenix"))) const unsigned char descriptor[] = {'
                              + ",".join(map(str, data)) + '};\n')
            library = root / "fixture.so"
            subprocess.run(shlex.split(os.environ.get("CC", "cc")) + ["-shared", "-fPIC", str(source), "-o", str(library)], check=True, capture_output=True)
            output = root / "descriptor.bin"
            output.unlink(missing_ok=True)
            process = subprocess.run([describe, str(library), str(output)], capture_output=True, text=True)
            assert (process.returncode == 0) == accepted, (name, process.stderr)
            if accepted: assert output.read_bytes() == expected, name
    return dict(cases=len(cases), accepted=sum(c[2] for c in cases), constructor_executed=False)

def run(probe, describe, fixture, vectors):
    positive = [definition(), native()]
    expected = json.loads(Path(vectors).read_text())
    for obj, vector in zip(positive, expected["metadata"], strict=True):
        raw = encode(obj)
        assert raw.hex() == vector["hex"]
        assert hashlib.sha256(raw).hexdigest() == vector["sha256"]
    cases = []
    def case(name, kind, obj, accepted, opcode=0, raw=False):
        data = obj if raw else encode(obj)
        cases.append((name, accepted, dict(kind=kind, bytes=data.hex(), opcode=opcode)))
    for n, obj in enumerate(positive): case(f"metadata-{n}", "metadata", obj, True)
    # Each schema tag is externally authored, including nested Ref schemas.
    for s in ["Bool", "I64", "String", "Bytes", ["Option", "U64"], ["Struct", {"count": "U64"}], ["Ref", bytes([4])*32]]:
        obj = native(); obj["properties"][0]["schema"] = schema(s); case("schema-"+str(s), "metadata", obj, True)
    for key in definition():
        obj = definition(); del obj[key]; case("missing-"+key, "metadata", obj, False)
    for key, bad in [("format", "phenix.descriptor.v1"), ("plugin", "root"), ("plugin", "bad/path"), ("execution", "sync"), ("version", ""), ("definitions", None), ("requires", {})]:
        obj = definition(); obj[key] = bad; case("bad-root-"+key, "metadata", obj, False)
    obj = definition(); obj["unrecognized"] = 0; case("unknown-root", "metadata", obj, False)
    for key in ["requires", "provides", "properties"]:
        obj = definition(); obj[key] = native()[key]; case("inert-"+key, "metadata", obj, False)
    for array, field in [("requires", "slot"), ("provides", "slot"), ("properties", "id")]:
        obj = native(); obj[array].append(copy.deepcopy(obj[array][0])); case("duplicate-"+array, "metadata", obj, False)
        obj = native(); obj[array][0][field] = "bad/path"; case("identity-"+array, "metadata", obj, False)
        obj = native(); obj[array][0]["forged"] = "owner"; case("unknown-"+array, "metadata", obj, False)
    for array in ["requires", "properties"]:
        obj = native(); obj[array].reverse(); case("unsorted-"+array, "metadata", obj, False)
    for key, bad in [("revision_min", 0), ("revision_max", 0), ("revision_max", True), ("cardinality", "best"), ("minimum", 65537), ("contract", "example.counter@01")]:
        obj = native(); obj["requires"][0][key] = bad; case("bad-import-"+key, "metadata", obj, False)
    for kind, n in [("one", 0), ("optional", 1), ("many", True)]:
        obj = native(); obj["requires"][0].update(cardinality=kind, minimum=n); case("cardinality-"+kind, "metadata", obj, False)
    for key, bad in [("revision", 0), ("revision", True), ("contract", "counter"), ("operations", [])]:
        obj = definition(); obj["definitions"][0][key] = bad; case("bad-definition-"+key, "metadata", obj, False)
    obj = definition(); obj["definitions"].reverse(); case("unsorted-definitions", "metadata", obj, False)
    obj = definition(); obj["definitions"][0]["operations"].reverse(); case("unsorted-operations", "metadata", obj, False)
    obj = definition(); obj["definitions"] *= 2; case("duplicate-definition", "metadata", obj, False)
    obj = definition(); obj["definitions"][0]["operations"] *= 2; case("duplicate-operation", "metadata", obj, False)
    for bad in [schema("Unknown"), schema(["Variants", ["B", "A"]]), schema(["Ref", bytes(32)]), schema("U64") + b"\0", b"\xff", encode("U64")]:
        obj = native(); obj["properties"][0]["schema"] = bad; case("bad-schema-"+bad.hex(), "metadata", obj, False)
    obj = native(); obj["properties"][0].update(get=False, set=False, watch=False); case("no-accessors", "metadata", obj, False)
    obj = native(); obj["properties"][0]["get"] = 1; case("numeric-get", "metadata", obj, False)
    obj = native(); obj["properties"][1]["id"] = "counter"; case("conflicting-property", "metadata", obj, False)
    for end in range(len(encode(definition()))): case("truncated-metadata-"+str(end), "metadata", encode(definition())[:end], False, raw=True)
    case("wrong-profile", "metadata", encode(definition(), False), False, raw=True)
    case("trailing", "metadata", encode(definition())+b"\0", False, raw=True)
    r, r2 = Ref(), Ref(5)
    valid_requests = [(1, ["counter", 0]), (2, r), (3, [r, 0, 1]), (4, r), (5, 1), (6, 1), (7, [[r, 0, 1], [r2, 1, 2]])]
    for op, obj in valid_requests:
        case("request-"+str(op), "request", obj, True, op)
        for end in range(len(encode(obj))): case(f"truncated-request-{op}-{end}", "request", encode(obj)[:end], False, op, raw=True)
    for op, obj in [(0, None), (8, None), (1, ["", 0]), (1, ["counter"]), (1, ["bad/path", 0]), (2, None), (3, [r, True, 1]), (3, [None, 1, 1]), (4, 1), (5, 0), (6, 0), (7, []), (7, [[r2, 0, 1], [r, 0, 1]]), (7, [[r, 0, 1], [r, 0, 2]])]:
        case("bad-request-"+str(op), "request", obj, False, op)
    errors = ["OwnerGone", "StaleReference", "SchemaMismatch", "Denied", "RevisionExhausted", "EpochExhausted"]
    for error in errors: case(error, "error", [error], True)
    case("conflict", "error", ["Conflict", 3, 4], True)
    for error in [None, "Denied", [], ["Unknown"], ["Denied", 0], ["Conflict"], ["Conflict", True, 2], ["Conflict", 0, 1, 2]]: case("bad-error-"+str(error), "error", error, False)
    process = subprocess.run([probe], input="".join(json.dumps(c[2])+"\n" for c in cases), text=True, capture_output=True, check=True)
    results = [json.loads(line) for line in process.stdout.splitlines()]
    assert len(results) == len(cases)
    for (name, accepted, item), result in zip(cases, results):
        assert result["accepted"] == accepted, (name, result)
        if accepted: assert result["bytes"] == item["bytes"], name
    package_cases = 0
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)/"package"; root.mkdir()
        subprocess.run([fixture, str(root/"descriptor.bin")], check=True)
        data = (root/"descriptor.bin").read_bytes(); assert data == encode(definition())
        digest = hashlib.sha256(data).hexdigest()
        record = dict(format="phenix.contract-package.v2", descriptor="descriptor.bin", descriptor_sha256=digest)
        def verify(rec, ok):
            nonlocal package_cases
            (root/"package.json").write_text(json.dumps(rec))
            p = subprocess.run([describe, "--metadata", str(root)], text=True, capture_output=True)
            assert (p.returncode == 0) == ok, (rec, p.stderr)
            package_cases += 1
            return json.loads(p.stdout) if ok else None
        first = verify(record, True)
        expected_id = hashlib.sha256(b"phenix.contract-package.v2\0"+bytes.fromhex(digest)).hexdigest()
        assert first["package_sha256"] == expected_id
        assert first["descriptor_sha256"] == digest
        (root/"renamed.bin").write_bytes(data)
        assert verify(dict(record, descriptor="renamed.bin"), True)["package_sha256"] == expected_id
        (root/"package.json").write_text(json.dumps(record, indent=4))
        assert json.loads(subprocess.check_output([describe, "--metadata", str(root)]))["package_sha256"] == expected_id
        package_cases += 1
        for rec in [dict(record, artifact="lib/plugin.so"), dict(record, target="x86_64-unknown-linux-gnu"), dict(record, format="phenix.package.v1"), dict(record, descriptor_sha256="0"*64), dict(record, descriptor="../descriptor.bin"), dict(record, descriptor="/tmp/file")]: verify(rec, False)
        (Path(tmp)/"outside.bin").write_bytes(data); (root/"escape.bin").symlink_to(Path(tmp)/"outside.bin")
        verify(dict(record, descriptor="escape.bin"), False)
        (root/"descriptor.bin").write_bytes(encode(native())); verify(dict(record, descriptor_sha256=hashlib.sha256(encode(native())).hexdigest()), False)
        (root/"descriptor.bin").write_bytes(data+b"\0"); verify(record, False)
        assert not (root/"lib").exists()
    print(json.dumps(dict(gate="m1-metadata-state", vectors_sha256=hashlib.sha256(Path(vectors).read_bytes()).hexdigest(), cases=len(cases), accepted=sum(c[1] for c in cases), rejected=sum(not c[1] for c in cases), package_cases=package_cases, native_sections=native_sections(describe), descriptor_sha256=digest, package_sha256=expected_id, private_kernel_dependency=False, live_state_proven=False), sort_keys=True))

if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--freeze-vectors":
        Path(sys.argv[2]).write_text(json.dumps(dict(metadata=[dict(hex=encode(v).hex(),sha256=hashlib.sha256(encode(v)).hexdigest()) for v in [definition(), native()]]), indent=2)+"\n")
    else: run(*sys.argv[1:])
