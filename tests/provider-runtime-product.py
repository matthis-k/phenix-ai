"""Native M1 acceptance of the provider's three canonical operations."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

kernel, fixture, definitions, destination = sys.argv[1:]
out = Path(destination)
out.mkdir(parents=True, exist_ok=True)
contract = "phenix.provider.runtime@1"
provider = "phenix.provider.loopback-fixture"
resource = "phenix.providers.user"
os.environ["PHENIX_C9_TEST_HTTP_ENDPOINT"] = "http://127.0.0.1:18765/completions"

def portable(value):
    if value is None:
        return b"\x00"
    if value is False:
        return b"\x01"
    if value is True:
        return b"\x02"
    if isinstance(value, int):
        return b"\x04" + value.to_bytes(8, "big")
    if isinstance(value, str):
        data = value.encode()
        return b"\x05" + len(data).to_bytes(4, "big") + data
    if isinstance(value, list):
        return b"\x07" + len(value).to_bytes(4, "big") + b"".join(map(portable, value))
    if isinstance(value, dict):
        fields = sorted(value.items())
        return b"\x08" + len(fields).to_bytes(4, "big") + b"".join(
            len(k.encode()).to_bytes(4, "big") + k.encode() + portable(v)
            for k, v in fields
        )
    raise TypeError(type(value))

with tempfile.TemporaryDirectory(prefix="phenix-provider-runtime-") as d:
    root = Path(d)
    def invoke(operation, request, *, grant=resource):
        composition = {
            "format": "phenix.composition.v3",
            "entry": {"interface": contract, "operation": operation, "provider": provider},
            "artifacts": [fixture],
            "definitions": [definitions],
            "slots": [],
            "grants": [{"caller": "root", "interface": contract,
                        "operation": operation, "resources": [grant]}],
            "excluded": [],
            "substitutions": [],
            "authority": {
                "operations": [contract + "/" + operation],
                "resources": [resource],
            },
            "resource": resource,
        }
        (root / "composition.json").write_text(json.dumps(composition))
        (root / "request.bin").write_bytes(b"PV\x02\0" + portable(request))
        result = subprocess.run(
            [kernel, "invoke", str(root / "composition.json"), str(root / "request.bin")],
            capture_output=True, text=True, timeout=30,
        )
        assert result.stdout, (result.returncode, result.stderr)
        return json.loads(result.stdout)["outcome"], result.returncode

    status_request = {
        "Status": {"query": {
            "scope": "user", "provider_id": "c9-test-loopback", "connection_id": "local",
        }},
    }
    status, code = invoke("authenticate", status_request)
    assert code == 0 and status["kind"] == "success", status
    state = status["value"]["Status"]["value"]
    assert state == {
        "provider_id": "c9-test-loopback", "connection_id": "local", "profiles": []
    }, state

    unavailable, code = invoke("authenticate", {
        "Begin": {"request": {
            "scope": "user", "provider_id": "c9-test-loopback",
            "connection_id": "local", "method_id": "api-key"
        }},
    })
    assert code == 0 and unavailable["kind"] == "domain_error", unavailable
    assert unavailable["value"]["code"] == "Unavailable", unavailable

    models, code = invoke("models", {
        "scope": "user", "provider_id": "c9-test-loopback", "connection_id": "local",
        "profile_id": None, "cursor": None, "limit": 20,
    })
    assert code == 0 and models["kind"] == "success", models
    assert [model["id"] for model in models["value"]["models"]] == [
        "fixture-a", "fixture-b"
    ], models

    selected = {
        "scope": "user", "provider_id": "c9-test-loopback",
        "connection_id": "local", "profile_id": None, "model_id": "fixture-a",
        "request": {
            "session": "session-1", "run": "run-1", "history": [], "tools": [],
            "skills": [], "context": [],
            "limits": {
                "model_turns": None, "tool_calls": None, "model_tokens": None,
                "http_timeout_ms": None,
            },
        },
    }
    no_protocol, code = invoke("complete", selected)
    assert code == 0 and no_protocol["kind"] == "domain_error", no_protocol
    assert no_protocol["value"]["code"] == "Unavailable", no_protocol

    missing, code = invoke("complete", {**selected, "model_id": "unlisted"})
    assert code == 0 and missing["kind"] == "domain_error", missing
    assert missing["value"]["code"] == "NotFound", missing

    other, code = invoke("complete", {**selected, "profile_id": ["other-account"]})
    assert code == 0 and other["kind"] == "domain_error", other
    assert other["value"]["code"] == "InvalidInput", other

    denied, code = invoke("models", {
        "scope": "user", "provider_id": "c9-test-loopback", "connection_id": "local",
        "profile_id": None, "cursor": None, "limit": 20,
    }, grant="phenix.providers.foreign")
    assert code != 0 and denied["kind"] == "kernel_failure", denied
    assert denied["failure"]["code"] == "Denied", denied

    (out / "verified.json").write_text(json.dumps({
        "format": "phenix.provider.runtime.product.v1",
        "operations": ["authenticate", "models", "complete"],
        "auth_is_typed": True, "models_are_discovered": True,
        "absent_protocol_fails_closed": True, "unknown_model_denied": True,
        "foreign_profile_denied": True, "kernel_authority_enforced": True
    }, indent=2, sort_keys=True))
    print("Canonical provider interface native product verified")
