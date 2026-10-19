"""Verify installed provider discovery and denial using the real native kernel."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile

kernel, package, definitions, destination = sys.argv[1:]
out = Path(destination)
out.mkdir(parents=True, exist_ok=True)
provider = "phenix.provider.builtins"


def encode(value):
    if value is None:
        return b"\x00"
    if isinstance(value, str):
        payload = value.encode("utf-8")
        return b"\x05" + len(payload).to_bytes(4, "big") + payload
    if isinstance(value, int):
        return b"\x04" + value.to_bytes(8, "big")
    if isinstance(value, dict):
        items = sorted(value.items())
        return b"\x08" + len(items).to_bytes(4, "big") + b"".join(
            len(key.encode()).to_bytes(4, "big") + key.encode() + encode(item)
            for key, item in items
        )
    raise TypeError(type(value))


with tempfile.TemporaryDirectory(prefix="phenix-provider-native-") as temp:
    root = Path(temp)

    def invoke(contract, operation, request, *, grant="phenix.providers.user"):
        resource = "phenix.providers.user"
        composition = dict(
            format="phenix.composition.v3",
            entry=dict(interface=contract, operation=operation, provider=provider),
            artifacts=[package],
            definitions=[definitions],
            slots=[],
            grants=[dict(caller="root", interface=contract, operation=operation,
                         resources=[grant])],
            excluded=[],
            substitutions=[],
            authority=dict(operations=[contract + "/" + operation], resources=[grant]),
            resource=resource,
        )
        (root / "composition.json").write_text(json.dumps(composition))
        (root / "request.bin").write_bytes(b"PV\x02\0" + encode(request))
        process = subprocess.run(
            [kernel, "invoke", str(root / "composition.json"), str(root / "request.bin")],
            capture_output=True, text=True, timeout=30,
        )
        report = json.loads(process.stdout)
        return report.get("outcome"), process.returncode

    catalog = "phenix.provider.catalog@1"
    auth = "phenix.provider.auth@1"

    listed, status = invoke(catalog, "list", dict(scope="user"))
    assert status == 0 and listed["kind"] == "success", listed
    page = listed["value"]
    assert page["revision"] > 0
    providers = {item["id"]: item for item in page["providers"]}
    assert set(providers) == {
        "openai", "openrouter", "anthropic", "opencode-zen", "opencode-go", "zai"
    }, providers.keys()
    openai = providers["openai"]
    connections = {c["id"]: c for c in openai["connections"]}
    assert set(connections) == {"api", "chatgpt"}
    assert connections["api"]["auth_methods"][0]["id"] == "api-key"
    assert connections["chatgpt"]["auth_methods"][0]["id"] == "chatgpt-sign-in"
    assert connections["api"]["base_url"] != "" and connections["chatgpt"]["base_url"] != ""

    described, status = invoke(
        catalog, "describe", dict(scope="user", provider_id="openai")
    )
    assert status == 0 and described == dict(kind="success", value=openai)

    missing, status = invoke(
        catalog, "describe", dict(scope="user", provider_id="missing")
    )
    assert status == 0 and missing["kind"] == "domain_error"
    assert missing["value"]["code"] == "NotFound"

    denied, status = invoke(catalog, "list", dict(scope="other"))
    assert status == 0 and denied["kind"] == "domain_error"
    assert denied["value"]["code"] == "Denied"

    no_grant, status = invoke(
        catalog, "list", dict(scope="user"), grant="phenix.providers.other"
    )
    assert status != 0 and no_grant["kind"] == "kernel_failure"
    assert no_grant["failure"]["code"] == "Denied"

    readiness, status = invoke(auth, "status", dict(
        scope="user", provider_id="openai", connection_id="api"
    ))
    assert status == 0 and readiness == dict(kind="success", value=dict(
        provider_id="openai", connection_id="api", profiles=[]
    )), readiness

    models, status = invoke(catalog, "models", dict(
        scope="user", provider_id="openai", connection_id="api",
        profile_id=None, cursor=None, limit=20,
    ))
    assert status == 0 and models["kind"] == "domain_error"
    assert models["value"]["code"] == "AuthenticationRequired"

    challenge, status = invoke(auth, "begin", dict(
        scope="user", provider_id="openai", connection_id="api", method_id="api-key",
    ))
    assert status == 0 and challenge["kind"] == "domain_error"
    assert challenge["value"]["code"] == "Unavailable"

    (out / "product.json").write_text(json.dumps(dict(
        format="phenix.provider.builtins.product.v1",
        provider_ids=sorted(providers),
        provider_revision=page["revision"],
        verified_operations=["list", "describe", "status", "models", "begin"],
        denied_scope=True,
        denied_grant=True,
        authenticated_profiles=0,
        available_models=0,
    ), sort_keys=True, indent=2))
    print("Native provider discovery, authorization and unavailable-model checks passed")
