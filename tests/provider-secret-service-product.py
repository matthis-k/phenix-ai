"""Native credential acceptance against an isolated real Secret Service only."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

kernel, plugin, definitions, destination = sys.argv[1:]
provider = "phenix.provider.secret-service"
contract = "phenix.provider.credentials@1"


def encode(value):
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


with tempfile.TemporaryDirectory(prefix="phenix-secret-service-") as temporary:
    root = Path(temporary)
    # The outer dbus-run-session and private HOME/XDG paths are mandatory.
    assert os.environ.get("PHENIX_ISOLATED_SECRET_SERVICE_TEST") == "1"
    assert Path(os.environ["HOME"]).is_relative_to(Path(os.environ["TMPDIR"]))
    control = root / "control"
    control.mkdir(mode=0o700)

    def start_service(replace=False):
        command = ["gnome-keyring-daemon", "--unlock", "--control-directory=" + str(control)]
        if replace:
            command.append("--replace")
        subprocess.run(command, input=b"isolated-fixture-password",
                       capture_output=True, check=True, timeout=30)
        # Unlock only prepares the daemon; --start registers the secrets component.
        subprocess.run(["gnome-keyring-daemon", "--start", "--components=secrets",
                        "--control-directory=" + str(control)],
                       capture_output=True, check=True, timeout=30)
        subprocess.run(["dbus-send", "--session", "--print-reply",
                        "--dest=org.freedesktop.DBus", "/org/freedesktop/DBus",
                        "org.freedesktop.DBus.GetNameOwner", "string:org.freedesktop.secrets"],
                       capture_output=True, check=True, timeout=30)

    start_service()
    first, second = identity(), identity("work")
    secrets = [b"fixture-personal-key", b"fixture-work-key", b"fixture-rotated-key"]

    def invoke(operation, request, *, granted=None, authorized=True, environment=None):
        selected = resource(first) if granted is None else granted
        policy = dict(
            format="phenix.composition.v2",
            entry=dict(interface=contract, operation=operation, provider=provider),
            artifacts=[plugin], definitions=[definitions], slots=[],
            grants=[dict(caller="root", interface=contract, operation=item,
                         resources=[selected]) for item in ("import", "resolve", "revoke")
                    if authorized or item != operation],
            excluded=[], substitutions=[],
            authority=dict(operations=[contract + "/" + item for item in
                                      ("import", "resolve", "revoke")], resources=[selected]),
            resource=selected,
        )
        composition = root / "composition.json"
        composition.write_text(json.dumps(policy))
        payload = root / "request.bin"
        payload.touch(mode=0o600)
        payload.write_bytes(b"PV\x02\0" + encode(request))
        proc = subprocess.run([kernel, "invoke", str(composition), str(payload)],
                              capture_output=True, text=True,
                              env=environment, timeout=30)
        report = json.loads(proc.stdout)
        outcome = report.pop("outcome", None)
        if outcome is None and report.get("last_failure"):
            outcome = dict(kind="kernel_failure", failure=report["last_failure"])
        for secret in secrets:
            assert secret.decode() not in json.dumps(report) + proc.stderr
            assert json.dumps(list(secret)) not in json.dumps(report)
        return outcome, proc.returncode

    def success(operation, request, value=None, **kwargs):
        outcome, status = invoke(operation, request, **kwargs)
        # Do not print a secret-bearing outcome even on failure.
        assert status == 0 and outcome == dict(kind="success", value={} if value is None else value)

    def domain(operation, request, code, **kwargs):
        outcome, status = invoke(operation, request, **kwargs)
        assert status == 0 and outcome["kind"] == "domain_error"
        assert outcome["value"]["code"] == code

    domain("resolve", first, "NotFound")
    success("import", dict(identity=first, secret=secrets[0]))
    success("import", dict(identity=second, secret=secrets[1]), granted=resource(second))
    # Every invocation is a new kernel/native plugin process.
    success("resolve", first, dict(bytes=list(secrets[0])))
    for operation, request in [("resolve", second), ("revoke", second),
                               ("import", dict(identity=second, secret=secrets[2])),
                               ("resolve", identity(connection="chatgpt"))]:
        domain(operation, request, "Denied")
    domain("resolve", first, "Denied", granted="phenix.session.foreign")
    for operation, request in [("resolve", first), ("revoke", first),
                               ("import", dict(identity=first, secret=secrets[2]))]:
        outcome, status = invoke(operation, request, authorized=False)
        assert status != 0 and outcome["kind"] == "kernel_failure"
        failure = outcome["failure"]
        assert (failure["code"] if isinstance(failure, dict) else failure) == "Denied"
        success("resolve", first, dict(bytes=list(secrets[0])))
    success("import", dict(identity=first, secret=secrets[2]))
    # Replace the actual service, not just the caller, to verify at-rest persistence.
    start_service(replace=True)
    success("resolve", first, dict(bytes=list(secrets[2])))
    success("revoke", first)
    domain("resolve", first, "NotFound")
    success("resolve", second, dict(bytes=list(secrets[1])), granted=resource(second))
    # A broken service must not select the file backend, even with consent set.
    fallback = root / "fallback"
    fallback.mkdir(mode=0o700)
    unavailable = dict(os.environ, DBUS_SESSION_BUS_ADDRESS="unix:path=/nonexistent/phenix-test-bus",
                       PHENIX_CREDENTIAL_FILE_CONSENT="1",
                       PHENIX_CREDENTIAL_FILE_DIRECTORY=str(fallback))
    for operation, request in [("resolve", second), ("revoke", second),
                               ("import", dict(identity=second, secret=secrets[0]))]:
        domain(operation, request, "Unavailable", granted=resource(second), environment=unavailable)
    assert not list(fallback.iterdir())
    success("resolve", second, dict(bytes=list(secrets[1])), granted=resource(second))
    success("revoke", second, granted=resource(second))

out = Path(destination)
out.mkdir(parents=True, exist_ok=True)
(out / "accepted.json").write_text(json.dumps(dict(
    format="phenix.provider.secret-service.v1", accepted=True,
    cases=["real-isolated-secret-service", "native-process-restart", "service-restart", "profile-isolation",
           "connection-isolation", "session-resource-denial", "independent-operation-grants",
           "rotation-restart", "revocation-no-account-substitution", "redacted-diagnostics",
           "service-unavailable-no-file-fallback"],
), indent=2))
