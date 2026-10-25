"""End-to-end M1 test: install a tool implementation without editing Basic Tools."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

kernel, manager, fixture, definitions, destination = sys.argv[1:]
out = Path(destination)
out.mkdir(parents=True, exist_ok=True)

catalog = "phenix.basic.catalog@1"
execute = "phenix.basic.tool@1"
contribution = "phenix.basic.tool-registrations@1"
session = "tool-session"
resource = "phenix.session." + session

def encode(value):
    if value is None:
        return b"\x00"
    if isinstance(value, bool):
        return b"\x02" if value else b"\x01"
    if isinstance(value, str):
        raw = value.encode()
        return b"\x05" + len(raw).to_bytes(4, "big") + raw
    if isinstance(value, int):
        return b"\x04" + value.to_bytes(8, "big")
    if isinstance(value, list):
        return b"\x07" + len(value).to_bytes(4, "big") + b"".join(map(encode, value))
    if isinstance(value, dict):
        fields = sorted(value.items())
        return b"\x08" + len(fields).to_bytes(4, "big") + b"".join(
            len(key.encode()).to_bytes(4, "big") + key.encode() + encode(v)
            for key, v in fields
        )
    raise TypeError(type(value))

with tempfile.TemporaryDirectory(prefix="phenix-tool-registrations-") as temp:
    root = Path(temp)
    def invoke(contract, operation, request, *, selected=True, granted=True):
        service = "phenix.basic.tools"
        ops = [contract + "/" + operation]
        slots = []
        grants = [
            dict(caller="root", interface=contract, operation=operation,
                 resources=[resource if granted else "phenix.session.foreign"]),
            # Kernel lifecycle graph handlers are explicitly granted even
            # for the legacy pull-only invoke test; no event is emitted here.
            dict(caller=service, interface="phenix.tools.startup@1",
                 operation="begin", resources=[resource]),
            dict(caller=service, interface="phenix.tools.settlement@1",
                 operation="complete", resources=[resource]),
        ]
        ops += ["phenix.tools.startup@1/begin", "phenix.tools.settlement@1/complete"]
        if selected:
            slots = [
                dict(caller=service, slot="Contributors", index=0,
                     provider="phenix.tool.fixture-reverse",
                     export="Registrations"),
                dict(caller="phenix.tool.fixture-reverse", slot="Registry",
                     index=0, provider=service, export="Registry"),
            ]
            grants += [
                dict(caller="phenix.tool.fixture-reverse",
                     interface="phenix.tools.registry@1",
                     operation="register", resources=[resource]),
                dict(caller="phenix.tool.fixture-reverse",
                     interface="phenix.tools.populate@1",
                     operation="register", resources=[resource]),
            ]
            ops += ["phenix.tools.registry@1/register",
                    "phenix.tools.populate@1/register"]
            for operation_name in ("list", "execute"):
                grants.append(dict(
                    caller=service,
                    interface=contribution,
                    operation=operation_name,
                    resources=[resource],
                ))
                ops.append(contribution + "/" + operation_name)
        composition = dict(
            format="phenix.composition.v3",
            entry=dict(interface=contract, operation=operation, provider=service),
            artifacts=[manager, fixture] if selected else [manager],
            definitions=[definitions],
            slots=slots,
            grants=grants,
            excluded=[],
            substitutions=[],
            authority=dict(operations=ops, resources=[resource]),
            resource=resource,
        )
        (root / "composition.json").write_text(json.dumps(composition))
        (root / "request.bin").write_bytes(b"PV\x02\0" + encode(request))
        run = subprocess.run(
            [kernel, "invoke", str(root / "composition.json"), str(root / "request.bin")],
            capture_output=True, text=True, timeout=30,
        )
        if not run.stdout:
            raise AssertionError((run.returncode, run.stderr))
        return json.loads(run.stdout).get("outcome"), run.returncode

    listed, code = invoke(catalog, "list", dict(
        session=session, query="", cursor=None, limit=32
    ))
    assert code == 0 and listed["kind"] == "success", listed
    tools = listed["value"]["tools"]
    assert [item["id"] for item in tools] == [
        "fail", "fixture.reverse", "length", "uppercase"
    ], tools
    assert all(item["arguments_schema"] for item in tools)
    assert listed["value"]["revision"] > 0

    default, code = invoke(catalog, "list", dict(
        session=session, query="", cursor=None, limit=32
    ), selected=False)
    assert code == 0 and default["kind"] == "success", default
    assert [item["id"] for item in default["value"]["tools"]] == [
        "fail", "length", "uppercase"
    ]

    reversed_result, code = invoke(execute, "execute", dict(
        session=session, run="one",
        call=dict(id="call-one", tool="fixture.reverse", arguments='"abß"')
    ))
    assert code == 0 and reversed_result == {
        "kind": "success", "value": {"text": "ßba"}
    }, reversed_result

    unknown, code = invoke(execute, "execute", dict(
        session=session, run="one",
        call=dict(id="call-two", tool="unregistered", arguments='"ok"')
    ))
    assert code == 0 and unknown["kind"] == "domain_error", unknown
    assert unknown["value"]["code"] == "NotFound"

    malformed, code = invoke(execute, "execute", dict(
        session=session, run="one",
        call=dict(id="call-three", tool="fixture.reverse", arguments="{broken")
    ))
    assert code == 0 and malformed["kind"] == "domain_error", malformed
    assert malformed["value"]["code"] == "InvalidInput"

    denied, code = invoke(catalog, "list", dict(
        session=session, query="", cursor=None, limit=32
    ), granted=False)
    assert code != 0 and denied["kind"] == "kernel_failure", denied
    assert denied["failure"]["code"] == "Denied", denied

    (out / "verified.json").write_text(json.dumps({
        "format": "phenix.tool.contributor.product.v1",
        "external_id": "fixture.reverse",
        "registered_without_manager_source_edit": True,
        "selected_dispatch": True,
        "unknown_tool_rejected": True,
        "malformed_json_rejected": True,
        "wrong_resource_denied": True,
        "builtin_fallback": True,
    }, indent=2))
    print("Selected external tool registration and execution verified")
