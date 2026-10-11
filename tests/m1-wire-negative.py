"""Independent S1a wire conformance against the public Rust contract probe."""
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import sys

PREFIX = b"PV\x02\0"
MAX_BYTES, MAX_DEPTH, MAX_COUNT = 1 << 20, 32, 65536
WIDTHS = {"generation": 32, "owner": 32, "cell": 16, "schema": 32}


def value(kind, data=None):
    return {"kind": kind} if kind == "null" else {"kind": kind, "value": data}


def encode(item, profile="phenix.value.v1"):
    def put(item, depth):
        if depth > MAX_DEPTH:
            raise ValueError("depth")
        kind, data = item["kind"], item.get("value")
        n = lambda count: struct.pack(">I", count)
        if kind == "null":
            return b"\0"
        if kind == "bool":
            return bytes([2 if data else 1])
        if kind in ("i64", "u64"):
            return bytes([3 if kind == "i64" else 4]) + struct.pack(
                ">q" if kind == "i64" else ">Q", data
            )
        if kind in ("string", "bytes"):
            raw = data.encode() if kind == "string" else bytes(data)
            return bytes([5 if kind == "string" else 6]) + n(len(raw)) + raw
        if kind in ("list", "map"):
            if len(data) > MAX_COUNT:
                raise ValueError("count")
            if kind == "list":
                return b"\7" + n(len(data)) + b"".join(put(v, depth + 1) for v in data)
            return b"\10" + n(len(data)) + b"".join(
                n(len(k.encode())) + k.encode() + put(data[k], depth + 1)
                for k in sorted(data)
            )
        if kind == "ref" and profile == "phenix.value.v2":
            fields = [bytes.fromhex(data[k]) for k in WIDTHS]
            if any(len(raw) != width or not any(raw) for raw, width in zip(fields, WIDTHS.values())):
                raise ValueError("identity")
            return b"\11" + b"".join(fields)
        raise ValueError("unsupported tag/profile")

    if profile not in ("phenix.value.v1", "phenix.value.v2"):
        raise ValueError("profile")
    result = (PREFIX if profile == "phenix.value.v2" else b"") + put(item, 0)
    if len(result) > MAX_BYTES:
        raise ValueError("payload")
    return result


def decode(raw, profile):
    at = 0

    def take(count):
        nonlocal at
        if at + count > len(raw):
            raise ValueError("truncated")
        result = raw[at:at + count]
        at += count
        return result

    def n():
        return struct.unpack(">I", take(4))[0]

    def text():
        return take(n()).decode()

    def get(depth):
        if depth > MAX_DEPTH:
            raise ValueError("depth")
        tag = take(1)[0]
        if tag == 0:
            return value("null")
        if tag in (1, 2):
            return value("bool", tag == 2)
        if tag in (3, 4):
            return value("i64" if tag == 3 else "u64", struct.unpack(">q" if tag == 3 else ">Q", take(8))[0])
        if tag == 5:
            return value("string", text())
        if tag == 6:
            return value("bytes", list(take(n())))
        if tag in (7, 8):
            count = n()
            if count > MAX_COUNT:
                raise ValueError("count")
            if tag == 7:
                return value("list", [get(depth + 1) for _ in range(count)])
            fields, previous = {}, None
            for _ in range(count):
                key = text()
                if previous is not None and previous >= key:
                    raise ValueError("key order")
                fields[key], previous = get(depth + 1), key
            return value("map", fields)
        if tag == 9 and profile == "phenix.value.v2":
            fields = {k: take(width).hex() for k, width in WIDTHS.items()}
            if any(not any(bytes.fromhex(v)) for v in fields.values()):
                raise ValueError("identity")
            return value("ref", fields)
        raise ValueError("tag")

    if len(raw) > MAX_BYTES:
        raise ValueError("payload")
    if profile == "phenix.value.v2":
        if take(4) != PREFIX:
            raise ValueError("prefix")
    elif profile != "phenix.value.v1":
        raise ValueError("profile")
    result = get(0)
    if at != len(raw):
        raise ValueError("trailing")
    return result


def run(probe, fixture_path):
    fixture = json.loads(Path(fixture_path).read_text())
    requests, expectations = [], []

    def case(name, request, expected=None):
        requests.append(request)
        expectations.append((name, expected))

    for vector in fixture["values"]:
        name, profile, expected = vector["name"], vector["profile"], vector["hex"]
        assert encode(vector["value"], profile).hex() == expected, name
        assert decode(bytes.fromhex(expected), profile) == vector["value"], name
        case(name + "/decode", dict(kind="decode", profile=profile, hex=expected), {"hex": expected})
        case(name + "/encode", dict(kind="encode", profile=profile, value=vector["value"]), {"hex": expected})
    for vector in fixture["schemas"]:
        assert encode(vector["descriptor"]).hex() == vector["hex"], vector["name"]
        digest = hashlib.sha256(b"phenix.schema.v2\0" + bytes.fromhex(vector["hex"])).hexdigest()
        assert digest == vector["id"], vector["name"]
        case(vector["name"], dict(kind="schema", hex=vector["hex"]), {"hex": vector["hex"], "id": digest})

    reference = next(v for v in fixture["values"] if v["name"] == "reference")
    raw = bytes.fromhex(reference["hex"])
    negatives = [("truncate/" + str(n), raw[:n]) for n in range(len(raw))]
    negatives += [
        ("trailing", raw + b"\0"),
        ("wrong-version", b"PV\3\0" + raw[4:]),
        ("wrong-magic", b"XX\2\0" + raw[4:]),
        ("reserved-prefix", b"PV\2\1" + raw[4:]),
        ("unknown-tag", PREFIX + b"\xff"),
        ("utf8", PREFIX + b"\5\0\0\0\1\xff"),
        ("count", PREFIX + b"\7" + struct.pack(">I", MAX_COUNT + 1)),
        ("length-overflow", PREFIX + b"\5\xff\xff\xff\xff"),
        ("duplicate-key", PREFIX + b"\10\0\0\0\2\0\0\0\1a\0\0\0\0\1a\0"),
        ("unordered-key", PREFIX + b"\10\0\0\0\2\0\0\0\1z\0\0\0\0\1a\0"),
        ("depth", PREFIX + (b"\7\0\0\0\1" * (MAX_DEPTH + 1)) + b"\0"),
        ("payload", PREFIX + b"\6" + struct.pack(">I", MAX_BYTES) + bytes(MAX_BYTES)),
    ]
    offset = 5
    for name, width in WIDTHS.items():
        bad = bytearray(raw)
        bad[offset:offset + width] = bytes(width)
        negatives.append(("zero/" + name, bytes(bad)))
        offset += width
    for name, bad in negatives:
        try:
            decode(bad, "phenix.value.v2")
        except (ValueError, UnicodeError):
            pass
        else:
            raise AssertionError("independent decoder accepted " + name)
        case(name, dict(kind="decode", profile="phenix.value.v2", hex=bad.hex()))
    case("v1-rejects-v2", dict(kind="decode", profile="phenix.value.v1", hex=raw.hex()))
    case("v1-rejects-bare-ref", dict(kind="decode", profile="phenix.value.v1", hex=raw[4:].hex()))
    case("v2-rejects-bare-v1", dict(kind="decode", profile="phenix.value.v2", hex="00"))
    case("unknown-profile", dict(kind="decode", profile="phenix.value.v3", hex="00"))
    case("v1-ref-encode", dict(kind="encode", profile="phenix.value.v1", value=reference["value"]))
    nested = value("map", {"items": value("list", [reference["value"]])})
    case("v1-nested-ref-encode", dict(kind="encode", profile="phenix.value.v1", value=nested))
    case("depth-boundary", dict(kind="decode", profile="phenix.value.v2", hex=(PREFIX + b"\7\0\0\0\1" * MAX_DEPTH + b"\0").hex()), {"hex": (PREFIX + b"\7\0\0\0\1" * MAX_DEPTH + b"\0").hex()})
    case("count-boundary", dict(kind="decode", profile="phenix.value.v2", hex=(PREFIX + b"\7" + struct.pack(">I", MAX_COUNT) + bytes(MAX_COUNT)).hex()), {"hex": (PREFIX + b"\7" + struct.pack(">I", MAX_COUNT) + bytes(MAX_COUNT)).hex()})
    for descriptor in [value("string", "Unknown"), value("list", [value("string", "Ref"), value("bytes", [1] * 31)]), value("list", [value("string", "Variants"), value("list", [value("string", "Z"), value("string", "A")])]), value("list", [value("string", "Variants"), value("list", [])]), value("list", [value("string", "Struct"), value("map", {"invalid/name": value("string", "Bool")})])]:
        case("invalid-schema", dict(kind="schema", hex=encode(descriptor).hex()))
    schemas = {s["name"]: s for s in fixture["schemas"]}
    for name, schema, item, valid in [
        ("ref-valid", "ref-string", reference["value"], True),
        ("ref-schema-mismatch", "ref-bool", reference["value"], False),
        ("ref-string-is-not-a-ref", "ref-string", value("string", reference["value"]["value"]["cell"]), False),
        ("unsigned", "u64", value("u64", 1), True),
        ("signed-mismatch", "u64", value("i64", 1), False),
        ("none", "option-option-bool", value("null"), True),
        ("some-none", "option-option-bool", value("list", [value("null")]), True),
        ("some-some", "option-option-bool", value("list", [value("list", [value("bool", True)])]), True),
        ("invalid-option", "option-option-bool", value("bool", True), False),
    ]:
        request = dict(kind="dynamic", schema_hex=schemas[schema]["hex"], profile="phenix.value.v2", value_hex=encode(item, "phenix.value.v2").hex())
        case(name, request, {"id": schemas[schema]["id"]} if valid else None)

    result = subprocess.run([probe], input="".join(json.dumps(r) + "\n" for r in requests), text=True, capture_output=True, check=True)
    replies = result.stdout.splitlines()
    assert len(replies) == len(requests), (len(replies), len(requests), result.stderr)
    for request, (name, expected), reply in zip(requests, expectations, replies):
        reply = json.loads(reply)
        if expected is None:
            assert "error" in reply, (name, reply)
        else:
            assert "ok" in reply, (name, reply)
            for key, data in expected.items():
                assert reply["ok"][key] == data, (name, key)
            if request["kind"] == "encode":
                assert decode(bytes.fromhex(reply["ok"]["hex"]), request["profile"]) == request["value"], name
    print(json.dumps({
        "format": "phenix.m1.reference-wire.evidence.v1",
        "vectors_sha256": hashlib.sha256(Path(fixture_path).read_bytes()).hexdigest(),
        "cases": len(requests),
        "accepted": sum(e is not None for _, e in expectations),
        "rejected": sum(e is None for _, e in expectations),
        "profiles": ["phenix.value.v1", "phenix.value.v2"],
        "live_authority_proven": False,
    }, sort_keys=True))


if __name__ == "__main__":
    run(sys.argv[1], sys.argv[2])
