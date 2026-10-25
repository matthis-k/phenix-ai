"""Installed ACP -> trusted host -> native credential plugin, with exact grants.

All credential bytes and request buffers stay in a temporary directory. The
Nix output contains only acceptance flags, never secrets or ACP transcripts.
"""
import json
import os
from pathlib import Path
import secrets
import stat
import subprocess
import sys
import tempfile

runtime, basic, credential_plugin, definitions, catalog, destination = sys.argv[1:]
out = Path(destination)
out.mkdir(parents=True, exist_ok=True)

contract = "phenix.provider.credentials@1"
account = {"provider_id": "openai", "connection_id": "api", "profile_id": "personal"}
other = {"provider_id": "openai", "connection_id": "api", "profile_id": "work"}


def resource(identity):
    return "phenix.credentials." + ".".join(
        identity[key].encode().hex()
        for key in ("provider_id", "connection_id", "profile_id")
    )


with tempfile.TemporaryDirectory(prefix="phenix-acp-credentials-") as temporary:
    root = Path(temporary)
    vault = root / "vault"
    vault.mkdir(mode=0o700)
    env = dict(os.environ, PHENIX_CREDENTIAL_FILE_CONSENT="1",
               PHENIX_CREDENTIAL_FILE_DIRECTORY=str(vault),
               PHENIX_BASIC_SQLITE_PATH=str(root / "basic.sqlite"),
               XDG_STATE_HOME=str(root / "state"))
    secret = secrets.token_hex(32)

    selected_resource = resource(account)
    configuration = dict(
        format="phenix.composition.v3",
        entry=dict(interface=contract, operation="import",
                   provider="phenix.provider.credential-files"),
        artifacts=[credential_plugin],
        definitions=[definitions],
        slots=[],
        grants=[
            dict(caller="root", interface=contract, operation=operation,
                 resources=[selected_resource])
            for operation in ("import", "revoke")
        ],
        excluded=[], substitutions=[],
        authority=dict(
            operations=[contract + "/" + operation for operation in ("import", "revoke")],
            resources=[selected_resource],
        ),
        resource=selected_resource,
    )

    config_file = root / "credentials.json"
    config_file.write_text(json.dumps(configuration))

    def start(config=None, resolver=None):
        argv = [runtime, "acp", basic]
        if config is not None:
            argv += ["--provider-management", catalog,
                     "--provider-credentials", str(config)]
        if resolver is not None:
            argv += ["--provider-model-resolver", str(resolver)]
        return subprocess.Popen(
            argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, text=True, bufsize=1, env=env,
        )

    def call(child, number, method, params):
        child.stdin.write(json.dumps(dict(
            jsonrpc="2.0", id=number, method=method, params=params,
        )) + "\n")
        child.stdin.flush()
        for _ in range(30):
            line = child.stdout.readline()
            assert line, (method, child.poll())
            frame = json.loads(line)
            if frame.get("method") == "session/update":
                continue
            assert frame.get("id") == number, (method, frame.get("id"))
            assert secret not in line, method
            return frame
        raise AssertionError("too many unrelated ACP notifications")

    def initialized(config, resolver=None):
        child = start(config, resolver)
        init = call(child, 1, "initialize", dict(protocolVersion=1, clientCapabilities={}))
        assert init["result"]["protocolVersion"] == 1
        return child

    def close(child):
        child.stdin.close()
        assert child.wait(timeout=15) == 0
        err = child.stderr.read()
        assert secret not in err
        child.stdout.close()
        child.stderr.close()

    # Ordinary Basic cannot reach credential operations without an independent grant.
    child = initialized(None)
    unavailable = call(child, 8, "_phenix/credentials/capabilities", {})
    assert unavailable["error"]["code"] == -32601
    blocked = call(child, 2, "_phenix/credentials/import", dict(**account, secret=secret))
    assert blocked["error"]["code"] == -32601, blocked.get("error", {}).get("code")
    close(child)
    assert not list(vault.iterdir())

    # A credential-specific grant must be strict. Reject added resolver grants
    # even when the selected default root remains import.
    excessive = json.loads(json.dumps(configuration))
    excessive["authority"]["operations"].append(contract + "/resolve")
    extra = root / "excessive.json"
    extra.write_text(json.dumps(excessive))
    bad = subprocess.run(
        [runtime, "acp", basic, "--provider-credentials", str(extra)],
        input="", capture_output=True, text=True, env=env, timeout=15,
    )
    assert bad.returncode != 0
    assert secret not in bad.stdout + bad.stderr

    # The resolver is an independent, exact-resource launch authority and has
    # no ACP extension to expose its returned secret to editor clients.
    resolver = json.loads(json.dumps(configuration))
    resolver["entry"]["operation"] = "resolve"
    resolver["grants"] = [{
        "caller": "root", "interface": contract,
        "operation": "resolve", "resources": [selected_resource]
    }]
    resolver["authority"]["operations"] = [contract + "/resolve"]
    resolver_file = root / "model-resolver.json"
    resolver_file.write_text(json.dumps(resolver))

    excessive_resolver = json.loads(json.dumps(resolver))
    excessive_resolver["authority"]["operations"].append(contract + "/import")
    excessive_resolver_file = root / "excessive-resolver.json"
    excessive_resolver_file.write_text(json.dumps(excessive_resolver))
    bad_resolver = subprocess.run(
        [runtime, "acp", basic,
         "--provider-model-resolver", str(excessive_resolver_file)],
        input="", capture_output=True, text=True, env=env, timeout=15,
    )
    assert bad_resolver.returncode != 0
    assert secret not in bad_resolver.stdout + bad_resolver.stderr

    child = initialized(config_file, resolver_file)
    denied_resolver = call(child, 10, "_phenix/credentials/resolve", account)
    assert denied_resolver["error"]["code"] == -32601
    granted = call(child, 8, "_phenix/credentials/capabilities", {})
    assert granted["result"] == dict(**account, **{"import": True, "bind_environment": False, "revoke": True})
    providers = call(child, 7, "_phenix/providers/list", {"scope": "user"})
    assert providers["result"]["revision"] > 0
    assert "openai" in {item["id"] for item in providers["result"]["providers"]}
    spoofed = call(child, 2, "_phenix/credentials/import", dict(**other, secret=secret))
    assert spoofed["error"]["code"] == -32602
    assert not list(vault.iterdir())

    imported = call(child, 3, "_phenix/credentials/import", dict(**account, secret=secret))
    assert imported["result"] == {"ok": True}
    unverified = call(child, 9, "_phenix/providers/auth/status", dict(
        scope="user", provider_id="openai", connection_id="api"))
    # The vault holds an API key, but the provider has not verified it.
    assert unverified["result"]["profiles"] == []
    assert len(list(vault.iterdir())) == 1
    stored = list(vault.iterdir())[0]
    assert stat.S_IMODE(stored.stat().st_mode) == 0o600
    assert secret.encode() in stored.read_bytes()

    # No ACP extension may read the secret back to a frontend.
    unsupported = call(child, 4, "_phenix/credentials/resolve", account)
    assert unsupported["error"]["code"] == -32601
    denied_revoke = call(child, 5, "_phenix/credentials/revoke", other)
    assert denied_revoke["error"]["code"] == -32602
    assert stored.exists()
    close(child)

    # A second process reloads the same file-backed credential namespace.
    child = initialized(config_file)
    revoked = call(child, 2, "_phenix/credentials/revoke", account)
    assert revoked["result"] == {"ok": True}
    assert not list(vault.iterdir())
    close(child)

    # Two named accounts use two independent, exact-resource compositions.
    # The ACP selector does not broaden one account's authority to another.
    second_resource = resource(other)

    def second_root(spec, destination):
        copy = json.loads(json.dumps(spec))
        copy["resource"] = second_resource
        copy["authority"]["resources"] = [second_resource]
        for grant in copy["grants"]:
            grant["resources"] = [second_resource]
        destination.write_text(json.dumps(copy))
        return destination

    work_file = second_root(configuration, root / "credentials-work.json")
    work_resolver = second_root(resolver, root / "resolver-work.json")
    argv = [
        runtime, "acp", basic,
        "--provider-management", catalog,
        "--provider-credentials", str(config_file),
        "--provider-model-resolver", str(resolver_file),
        "--provider-credentials", str(work_file),
        "--provider-model-resolver", str(work_resolver),
    ]
    child = subprocess.Popen(
        argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, text=True, bufsize=1, env=env,
    )
    assert call(child, 1, "initialize", dict(
        protocolVersion=1, clientCapabilities={}))["result"]["protocolVersion"] == 1
    capability = call(child, 2, "_phenix/credentials/capabilities", {})
    eligible = capability["result"]["profiles"]
    assert eligible == [
        dict(**account, **{"import": True, "bind_environment": False, "revoke": True}),
        dict(**other, **{"import": True, "bind_environment": False, "revoke": True}),
    ], eligible
    assert call(child, 3, "_phenix/credentials/import", dict(
        **account, secret=secret))["result"] == {"ok": True}
    assert call(child, 4, "_phenix/credentials/import", dict(
        **other, secret=secret))["result"] == {"ok": True}
    assert len(list(vault.iterdir())) == 2
    assert call(child, 5, "_phenix/credentials/revoke", other)["result"] == {"ok": True}
    assert len(list(vault.iterdir())) == 1
    assert call(child, 6, "_phenix/credentials/revoke", account)["result"] == {"ok": True}
    assert not list(vault.iterdir())
    close(child)

    # Repeating the same exact resource is never silently first-match.
    duplicate = subprocess.run(
        [runtime, "acp", basic,
         "--provider-credentials", str(config_file),
         "--provider-credentials", str(config_file)],
        input="", capture_output=True, text=True, env=env, timeout=15,
    )
    assert duplicate.returncode != 0
    assert "duplicate credential root" in duplicate.stderr
    assert secret not in duplicate.stdout + duplicate.stderr

(out / "acp-credential-management.json").write_text(json.dumps({
    "format": "phenix.acp.credentials.product.v1",
    "authorized_import": True,
    "authorized_revoke": True,
    "exact_profile": True,
    "cross_profile_denied": True,
    "resolve_unavailable_to_acp": True,
    "restart": True,
    "strict_composition_grants": True,
    "capabilities_identify_exact_authorized_profile": True,
    "capabilities_unavailable_without_grant": True,
    "catalog_coexists_without_credential_authority": True,
    "stored_secret_is_not_verified_auth": True,
    "resolve_only_model_root_requires_exact_launcher_authority": True,
    "model_resolver_never_exposed_as_acp_extension": True,
    "secret_redacted": True,
    "independent_named_accounts": True,
    "cross_account_revoke_isolation": True,
    "duplicate_account_roots_denied": True,
}, sort_keys=True, indent=2))
print("ACP credential import/revoke with exact launcher grants verified")
