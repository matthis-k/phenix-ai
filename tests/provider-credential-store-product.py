"""Real native credential calls, independent processes and exact resource denial.

Only non-secret acceptance metadata is retained in the Nix output. Temporary
requests/vault and captured resolve responses are removed after the test.
"""
import json
import os
from pathlib import Path
import secrets
import stat
import subprocess
import sys
import tempfile

kernel, plugin, definitions, destination = sys.argv[1:]
out = Path(destination)
out.mkdir(parents=True, exist_ok=True)
contract = "phenix.provider.credentials@1"
provider = "phenix.provider.credential-files"


def encode(value):
    if value is None:
        return b"\x00"
    if isinstance(value, str):
        data = value.encode()
        return b"\x05" + len(data).to_bytes(4, "big") + data
    if isinstance(value, bytes):
        return b"\x06" + len(value).to_bytes(4, "big") + value
    items = sorted(value.items())
    return b"\x08" + len(items).to_bytes(4, "big") + b"".join(
        len(key.encode()).to_bytes(4, "big") + key.encode() + encode(item)
        for key, item in items
    )


def identity(profile="personal", connection="api"):
    return dict(provider_id="openai", connection_id=connection, profile_id=profile)


def resource(account):
    return "phenix.credentials." + ".".join(
        account[key].encode().hex()
        for key in ("provider_id", "connection_id", "profile_id")
    )


with tempfile.TemporaryDirectory(prefix="phenix-credentials-") as temporary:
    root = Path(temporary)
    vault = root / "vault"
    vault.mkdir(mode=0o700)
    env = dict(os.environ, PHENIX_CREDENTIAL_FILE_DIRECTORY=str(vault),
               PHENIX_CREDENTIAL_FILE_CONSENT="1")
    first = identity()
    second = identity("work")
    first_secret = secrets.token_hex(32).encode()
    second_secret = secrets.token_hex(32).encode()
    rotated_secret = secrets.token_hex(32).encode()

    def invoke(operation, request, *, granted=None, authorized=True, environment=None):
        selected = resource(first) if granted is None else granted
        policy = dict(
            format="phenix.composition.v3",
            entry=dict(interface=contract, operation=operation, provider=provider),
            artifacts=[plugin], definitions=[definitions], slots=[],
            grants=[dict(caller="root", interface=contract, operation=item,
                         resources=[selected]) for item in ("import", "resolve", "revoke")
                    if authorized or item != operation],
            excluded=[], substitutions=[],
            authority=dict(operations=[contract + "/" + item for item in ("import", "resolve", "revoke")], resources=[selected]),
            resource=selected,
        )
        composition = root / "composition.json"
        composition.write_text(json.dumps(policy))
        payload = root / "request.bin"
        payload.touch(mode=0o600)
        payload.write_bytes(b"PV\x02\0" + encode(request))
        proc = subprocess.run([kernel, "invoke", str(composition), str(payload)],
                              capture_output=True, text=True, env=environment or env, timeout=30)
        # Never include raw captured stdout in an assertion diagnostic: an
        # explicitly authorized resolve response is itself secret-bearing.
        report = json.loads(proc.stdout)
        outcome = report.pop("outcome", None)
        if outcome is None and report.get("last_failure"):
            outcome = dict(kind="kernel_failure", failure=report["last_failure"])
        for secret in [first_secret, second_secret, rotated_secret]:
            assert secret.decode() not in json.dumps(report)
            assert secret.decode() not in proc.stderr
            assert json.dumps(list(secret)) not in json.dumps(report)
        return outcome, proc.returncode

    def domain(operation, request, code, **kwargs):
        result, status = invoke(operation, request, **kwargs)
        assert status == 0 and result["kind"] == "domain_error"
        assert result["value"]["code"] == code

    # RED before implementation: the package must export the typed operations.
    domain("resolve", first, "NotFound")
    missing_consent = dict(env, PHENIX_CREDENTIAL_FILE_CONSENT="0")
    domain("import", dict(identity=first, secret=first_secret), "Denied",
           environment=missing_consent)
    unconfigured = dict(env, PHENIX_CREDENTIAL_FILE_DIRECTORY="")
    domain("import", dict(identity=first, secret=first_secret), "Unavailable",
           environment=unconfigured)
    relative_directory = dict(env, PHENIX_CREDENTIAL_FILE_DIRECTORY="relative-vault")
    domain("import", dict(identity=first, secret=first_secret), "Unavailable",
           environment=relative_directory)
    assert not list(vault.iterdir())

    boundary = dict(provider_id="a" * 64, connection_id="b" * 48, profile_id="c")
    result, status = invoke("import", dict(identity=boundary, secret=first_secret),
                            granted=resource(boundary))
    assert status == 0 and result == dict(kind="success", value={})
    result, status = invoke("resolve", boundary, granted=resource(boundary))
    assert status == 0 and result == dict(kind="success", value=dict(bytes=list(first_secret)))
    result, status = invoke("revoke", boundary, granted=resource(boundary))
    assert status == 0 and result == dict(kind="success", value={})
    # Invalid identities cannot reach a credential provider as domain values:
    # the typed Portable decoder rejects them at the plugin boundary first.
    for size in [38, 64]:
        oversized = dict(provider_id="a" * size, connection_id="b" * size, profile_id="c" * size)
        for operation, request in [
            ("import", dict(identity=oversized, secret=first_secret)),
            ("resolve", oversized),
            ("revoke", oversized),
        ]:
            result, status = invoke(operation, request)
            assert status != 0 and result["kind"] == "kernel_failure"
            failure = result["failure"]
            assert (failure["code"] if isinstance(failure, dict) else failure) == "SchemaRejected"
    assert not list(vault.iterdir())

    result, status = invoke("import", dict(identity=first, secret=first_secret))
    assert status == 0 and result == dict(kind="success", value={})
    result, status = invoke("import", dict(identity=second, secret=second_secret),
                            granted=resource(second))
    assert status == 0 and result == dict(kind="success", value={})
    for path in vault.iterdir():
        assert stat.S_IMODE(path.stat().st_mode) == 0o600
    assert len(list(vault.iterdir())) == 2

    # Each invoke starts another actual kernel/plugin process, proving restart.
    result, status = invoke("resolve", first)
    assert status == 0 and result == dict(kind="success", value=dict(bytes=list(first_secret)))
    domain("resolve", second, "Denied")
    domain("import", dict(identity=second, secret=first_secret), "Denied")
    domain("revoke", second, "Denied")
    domain("resolve", identity(connection="chatgpt"), "Denied")
    domain("resolve", first, "Denied", granted="phenix.session.foreign")
    # Omit only the operation under test, retaining both independent grants.
    # Check bytes and names too: denied import/revoke must have no effect.
    for operation, request in [("import", dict(identity=first, secret=rotated_secret)),
                               ("resolve", first), ("revoke", first)]:
        before = {path.name: path.read_bytes() for path in vault.iterdir()}
        result, status = invoke(operation, request, authorized=False)
        assert status != 0 and result["kind"] == "kernel_failure"
        failure = result["failure"]
        assert (failure["code"] if isinstance(failure, dict) else failure) == "Denied"
        assert before == {path.name: path.read_bytes() for path in vault.iterdir()}

    result, status = invoke("import", dict(identity=first, secret=rotated_secret))
    assert status == 0 and result == dict(kind="success", value={})
    result, status = invoke("resolve", first)
    assert status == 0 and result == dict(kind="success", value=dict(bytes=list(rotated_secret)))
    assert len(list(vault.iterdir())) == 2

    result, status = invoke("revoke", first)
    assert status == 0 and result == dict(kind="success", value={})
    domain("resolve", first, "NotFound")
    result, status = invoke("resolve", second, granted=resource(second))
    assert status == 0 and result == dict(kind="success", value=dict(bytes=list(second_secret)))
    vault.chmod(0o755)
    domain("resolve", second, "Denied", granted=resource(second))

(out / "accepted.json").write_text(json.dumps(dict(
    format="phenix.provider.credential-store.v1", accepted=True,
    cases=["explicit-consent", "explicit-absolute-directory", "identity-boundaries", "private-files", "restart", "profile-isolation",
           "connection-isolation", "session-resource-denial", "import-grant-denial",
           "resolve-grant-denial", "revoke-grant-denial", "rotation-restart",
           "revoke-without-fallback", "insecure-vault-denial", "nonsecret-diagnostics"],
), indent=2))
