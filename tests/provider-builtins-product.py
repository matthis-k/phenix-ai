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
    assert [p["id"] for p in connections["api"]["protocols"]] == [
        "openai-responses", "openai-chat",
    ]
    assert [p["id"] for p in connections["chatgpt"]["protocols"]] == [
        "openai-responses",
    ]
    assert all(
        protocol["endpoint"] is None
        for connection in connections.values()
        for protocol in connection["protocols"]
    )

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

    # An offline registration never makes either connection authenticated,
    # even when the two connections belong to the same visible provider group.
    chatgpt_status, code = invoke(auth, "status", dict(
        scope="user", provider_id="openai", connection_id="chatgpt",
    ))
    assert code == 0 and chatgpt_status == dict(kind="success", value=dict(
        provider_id="openai", connection_id="chatgpt", profiles=[],
    )), chatgpt_status
    assert connections["chatgpt"]["auth_methods"][0]["kind"] == "OAuthAuthorizationCode"
    assert connections["api"]["auth_methods"][0]["kind"] == "ApiKey"
    assert connections["chatgpt"]["protocols"] != connections["api"]["protocols"], (
        "API-key and subscription connections must remain independently defined"
    )

    # No client can authenticate one connection by submitting the method ID
    # advertised by another connection in the same logical provider.
    for connection_id, foreign_method in [
        ("api", "chatgpt-sign-in"),
        ("chatgpt", "api-key"),
    ]:
        wrong_method, code = invoke(auth, "begin", dict(
            scope="user", provider_id="openai",
            connection_id=connection_id, method_id=foreign_method,
        ))
        assert code == 0 and wrong_method["kind"] == "domain_error", wrong_method
        assert wrong_method["value"]["code"] == "InvalidInput", wrong_method

    # A nonexistent provider/connection cannot be resolved to a fallback
    # account or silently become a selectable model.
    for provider_id, connection_id in [
        ("missing", "api"), ("openai", "missing"), ("openai", "standard"),
    ]:
        absent, code = invoke(auth, "status", dict(
            scope="user", provider_id=provider_id, connection_id=connection_id,
        ))
        assert code == 0 and absent["kind"] == "domain_error"
        assert absent["value"]["code"] == "NotFound"
        missing_models, code = invoke(catalog, "models", dict(
            scope="user", provider_id=provider_id, connection_id=connection_id,
            profile_id=None, cursor=None, limit=20,
        ))
        assert code == 0 and missing_models["kind"] == "domain_error"
        assert missing_models["value"]["code"] == "NotFound"

    # The registered subscription's OAuth method is not a completed login:
    # it must fail closed until a provider-authorized backend is installed.
    oauth, code = invoke(auth, "begin", dict(
        scope="user", provider_id="openai", connection_id="chatgpt",
        method_id="chatgpt-sign-in",
    ))
    assert code == 0 and oauth["kind"] == "domain_error", oauth
    assert oauth["value"]["code"] == "Unavailable", oauth
    no_catalog, code = invoke(catalog, "models", dict(
        scope="user", provider_id="openai", connection_id="chatgpt",
        profile_id=None, cursor=None, limit=20,
    ))
    assert code == 0 and no_catalog["kind"] == "domain_error", no_catalog
    assert no_catalog["value"]["code"] == "AuthenticationRequired", no_catalog

    for contract, operation, request in [
        (auth, "status", dict(
            scope="other", provider_id="openai", connection_id="api",
        )),
        (auth, "begin", dict(
            scope="other", provider_id="openai",
            connection_id="api", method_id="api-key",
        )),
        (catalog, "models", dict(
            scope="other", provider_id="openai", connection_id="api",
            profile_id=None, cursor=None, limit=20,
        )),
    ]:
        refused, code = invoke(contract, operation, request)
        assert code == 0 and refused["kind"] == "domain_error", refused
        assert refused["value"]["code"] == "Denied", refused
        no_authority, code = invoke(
            contract, operation, {**request, "scope": "user"},
            grant="phenix.providers.other",
        )
        assert code != 0 and no_authority["kind"] == "kernel_failure", no_authority
        assert no_authority["failure"]["code"] == "Denied", no_authority

    (out / "product.json").write_text(json.dumps(dict(
        format="phenix.provider.builtins.product.v1",
        provider_ids=sorted(providers),
        provider_revision=page["revision"],
        verified_operations=["list", "describe", "status", "models", "begin"],
        denied_scope=True,
        denied_grant=True,
        authenticated_profiles=0,
        available_models=0,
        wrong_connection_method_denied=True,
        unknown_connection_fallback_denied=True,
        subscription_oauth_unavailable=True,
        unauthorized_status_begin_models_denied=True,
    ), sort_keys=True, indent=2))
    print("Native provider discovery, authorization and unavailable-model checks passed")
