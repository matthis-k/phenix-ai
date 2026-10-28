"""Verify installed provider discovery and denial using the real native kernel."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile

kernel, host_package, builtin_package, definitions, destination, openai_api, credentials, chatgpt = sys.argv[1:]
out = Path(destination)
out.mkdir(parents=True, exist_ok=True)
provider = "phenix.provider.host"
builtin = "phenix.provider.builtins"


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

    def invoke(contract, operation, request, *, grant="phenix.providers.user",
               selected_contributor=False, both_openai_connections=False):
        resource = "phenix.providers.user"
        registrations_contract = "phenix.provider.registrations@1"
        operations = [contract + "/" + operation]
        grants = [dict(caller="root", interface=contract, operation=operation,
                       resources=[grant])]
        slots = []
        artifacts = [host_package]
        if selected_contributor:
            artifacts.extend([openai_api, credentials])
            slots.append(dict(caller="phenix.provider.openai-api", slot="Credentials",
                              index=0, provider="phenix.provider.credential-files",
                              export="Credentials"))
            slots.append(dict(caller=provider, slot="Contributors", index=0,
                              provider="phenix.provider.openai-api", export="Registrations"))
            if both_openai_connections:
                artifacts.append(chatgpt)
                slots.extend([
                    dict(caller="phenix.provider.chatgpt", slot="Credentials",
                         index=0, provider="phenix.provider.credential-files",
                         export="Credentials"),
                    dict(caller=provider, slot="Contributors", index=1,
                         provider="phenix.provider.chatgpt", export="Registrations"),
                ])
        else:
            artifacts.append(builtin_package)
            slots.append(dict(caller=provider, slot="Contributors", index=0,
                              provider=builtin, export="Registrations"))
        grants.append(dict(caller=provider, interface=registrations_contract,
                           operation="list", resources=[grant]))
        operations.append(registrations_contract + "/list")
        composition = dict(
            format="phenix.composition.v3",
            entry=dict(interface=contract, operation=operation,
                       provider=builtin if contract == registrations_contract else provider),
            artifacts=artifacts,
            definitions=[definitions],
            slots=slots,
            grants=grants,
            excluded=[],
            substitutions=[],
            authority=dict(operations=operations, resources=[grant]),
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
    registrations = "phenix.provider.registrations@1"

    # The actual native contributor is invokable independently of the
    # catalog/auth facade. It returns only immutable provider metadata.
    # Portable<()> uses the empty struct/map schema, not a null value.
    contributed, code = invoke(registrations, "list", {})
    assert code == 0 and contributed["kind"] == "success", contributed
    assert contributed["value"]["revision"] == 1
    assert len(contributed["value"]["providers"]) == 6
    assert not any("credential" in key for key in contributed["value"]), contributed

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
    key = connections["api"]["auth_methods"][0]["adapter"]
    subscription = connections["chatgpt"]["auth_methods"][0]["adapter"]
    assert key["id"] == "api-key", key
    assert subscription["id"] == "chatgpt.oauth", subscription
    assert len(key["config"]["schema_id"]) == 64
    assert key["config"]["schema_id"] != subscription["config"]["schema_id"]
    assert connections["api"]["model_sources"][0]["adapter"]["id"] == "openai.models"
    assert connections["chatgpt"]["model_sources"][0]["adapter"]["id"] == "chatgpt.models"
    assert [p["id"] for p in connections["api"]["protocols"]] == [
        "openai-responses", "openai-chat",
    ]
    assert [p["id"] for p in connections["chatgpt"]["protocols"]] == [
        "openai-responses",
    ]
    assert {p["id"]: p["endpoint"] for p in connections["api"]["protocols"]} == {
        "openai-responses": ["https://api.openai.com/v1/responses"],
        "openai-chat": ["https://api.openai.com/v1/chat/completions"],
    }
    # Shared protocol conventions resolve supported routes for both API keys
    # and OAuth. Endpoint metadata alone does not authenticate the account or
    # install an executable model adapter; subscription inference is still
    # unavailable until a real authorized backend is selected.
    assert {
        p["id"]: p["endpoint"] for p in connections["chatgpt"]["protocols"]
    } == {
        "openai-responses": ["https://api.openai.com/v1/responses"],
    }

    # Real selected M1 import: the OpenAI API plugin contributes its own
    # registration while the existing manager retains ownership of the public
    # catalog. It must not inherit the six builtin providers or authenticate.
    external, code = invoke(catalog, "list", dict(scope="user"),
                            selected_contributor=True)
    assert code == 0 and external["kind"] == "success", external
    contributed_providers = external["value"]["providers"]
    assert len(contributed_providers) == 1, contributed_providers
    assert contributed_providers[0]["id"] == "openai"
    assert [c["id"] for c in contributed_providers[0]["connections"]] == ["api"]
    # Independent native API-key and OAuth provider plugins share logical
    # "openai" without a central hardcoded list or cross-account fallback.
    combined, code = invoke(
        catalog, "list", dict(scope="user"),
        selected_contributor=True, both_openai_connections=True,
    )
    assert code == 0 and combined["kind"] == "success", combined
    assert len(combined["value"]["providers"]) == 1, combined
    openai_both = combined["value"]["providers"][0]
    assert openai_both["id"] == "openai", openai_both
    assert [entry["id"] for entry in openai_both["connections"]] == [
        "api", "chatgpt"
    ], openai_both
    methods = {
        c["id"]: [m["id"] for m in c["auth_methods"]]
        for c in openai_both["connections"]
    }
    assert methods == {
        "api": ["api-key"], "chatgpt": ["chatgpt-sign-in"]
    }, methods

    absent, code = invoke(
        catalog, "describe", dict(scope="user", provider_id="anthropic"),
        selected_contributor=True,
    )
    assert code == 0 and absent["kind"] == "domain_error", absent
    assert absent["value"]["code"] == "NotFound"

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
        verified_operations=["registration_list", "selected_catalog", "joined_api_oauth", "list", "describe", "status", "models", "begin"],
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
