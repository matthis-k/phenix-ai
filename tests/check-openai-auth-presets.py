"""Inspect the opt-in OpenAI ACP preset's three native authority roots.

This inspects the files that Nix actually installs, not a reconstructed
test-only composition. Secrets and user-specific vault paths are absent.
"""
import json
import sys
from pathlib import Path

management_path, writes_path, resolver_path, credential_resource, destination = sys.argv[1:]
management, writes, resolver = [
    json.loads(Path(path).read_text())
    for path in (management_path, writes_path, resolver_path)
]
catalog = "phenix.provider.catalog@1"
auth = "phenix.provider.auth@1"
credentials = "phenix.provider.credentials@1"

expected_provider = {
    f"{catalog}/{operation}" for operation in ("list", "describe", "models")
} | {f"{auth}/{operation}" for operation in ("status", "begin", "poll", "cancel")}
credential_resolve = f"{credentials}/resolve"

for document in (management, writes, resolver):
    assert document["format"] == "phenix.composition.v3"
    assert document["resource"] == credential_resource
    assert document["authority"]["resources"] == [credential_resource]
    assert document["excluded"] == document["substitutions"] == []
    assert document["definitions"]
    assert document["artifacts"]
    assert "secret" not in document
    for grant in document["grants"]:
        assert grant["resources"] == [credential_resource], grant
        assert grant["operation"] in ("list", "describe", "models", "status",
                                      "begin", "poll", "cancel",
                                      "import", "bind_environment", "revoke", "resolve")
    for operation in document["authority"]["operations"]:
        assert operation in expected_provider | {
            f"{credentials}/{method}" for method in ("import", "bind_environment", "revoke", "resolve")
        }, operation

assert management["entry"] == {
    "interface": catalog, "operation": "list",
    "provider": "phenix.provider.openai-api",
}
assert len(management["artifacts"]) == 2
assert management["slots"] == [{
    "caller": "phenix.provider.openai-api",
    "slot": "Credentials",
    "provider": "phenix.provider.credential-files",
    "export": "Credentials",
    "index": 0,
}]
assert set(management["authority"]["operations"]) == expected_provider | {credential_resolve}
assert len(management["grants"]) == len(expected_provider) + 1
assert {
    f"{grant['interface']}/{grant['operation']}" for grant in management["grants"]
} == expected_provider | {credential_resolve}
for grant in management["grants"]:
    expected_caller = ("phenix.provider.openai-api" if grant["interface"] == credentials
                       else "root")
    assert grant["caller"] == expected_caller

for document, entry_operation, expected_operations in (
    (writes, "import", {f"{credentials}/import", f"{credentials}/bind_environment", f"{credentials}/revoke"}),
    (resolver, "resolve", {credential_resolve}),
):
    assert document["entry"] == {
        "interface": credentials, "operation": entry_operation,
        "provider": "phenix.provider.credential-files",
    }
    assert len(document["artifacts"]) == 1
    assert document["slots"] == []
    assert set(document["authority"]["operations"]) == expected_operations
    assert len(document["grants"]) == len(expected_operations)
    for grant in document["grants"]:
        assert grant["caller"] == "root"
        assert f"{grant['interface']}/{grant['operation']}" in expected_operations

Path(destination).mkdir(parents=True, exist_ok=True)
(Path(destination) / "launcher-authority.json").write_text(json.dumps({
    "exact_credential_resource": credential_resource,
    "provider_root_operations": sorted(expected_provider),
    "profile_specific": True,
    "credential_secret_in_store": False,
    "sibling_resource_authorized": False,
}, indent=2, sort_keys=True))
