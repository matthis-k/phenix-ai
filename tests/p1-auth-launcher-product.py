"""Installed P1 launcher provisions its private vault without shell setup.

The explicit P1 auth product opts into the permission-protected *unencrypted*
file fallback. Credentials never enter the Nix store or test output.
"""
import json
import os
from pathlib import Path
import queue
import stat
import subprocess
import sys
import tempfile
import threading

launcher, destination = sys.argv[1:]
account = {"provider_id": "openai", "connection_id": "api", "profile_id": "personal"}
secret = "p1-private-vault-product-key"


def launch(env, *, expected_start=True, exercise_key=False):
    child = subprocess.Popen(
        [launcher], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, text=True, bufsize=1,
    )
    responses = queue.Queue()

    def read():
        for line in child.stdout:
            responses.put(line)
        responses.put(None)

    threading.Thread(target=read, daemon=True).start()

    def call(number, method, params):
        child.stdin.write(json.dumps({
            "jsonrpc": "2.0", "id": number, "method": method, "params": params,
        }) + "\n")
        child.stdin.flush()
        for _ in range(30):
            raw = responses.get(timeout=35)
            if raw is None:
                diagnostic = child.stderr.read()[-1500:].replace(secret, "[REDACTED]")
                raise AssertionError(
                    f"ACP exited ({child.poll()}) before {method}: {diagnostic}"
                )
            frame = json.loads(raw)
            if frame.get("method") == "session/update":
                continue
            assert frame.get("id") == number, frame
            return frame
        raise AssertionError("unexpected number of ACP notifications")

    try:
        if expected_start:
            initialized = call(1, "initialize", {
                "protocolVersion": 1, "clientCapabilities": {},
            })
            assert initialized.get("result", {}).get("protocolVersion") == 1, initialized
            providers = call(2, "_phenix/providers/list", {"scope": "user"})
            assert "result" in providers and any(
                item["id"] == "openai"
                for item in providers["result"]["providers"]
            ), providers
            if exercise_key:
                stored = call(3, "_phenix/credentials/import", {
                    **account, "secret": secret,
                })
                assert stored.get("result", {}).get("ok") is True, stored
        child.stdin.close()
        result = child.wait(timeout=40)
        error = child.stderr.read()
        assert secret not in error, "ACP stderr leaked a credential"
        if expected_start:
            assert result == 0, (result, error[-1200:])
        else:
            assert result == 2, (result, error[-1200:])
        return error
    finally:
        if child.poll() is None:
            child.kill()
            child.wait(timeout=10)
        if not child.stdin.closed:
            child.stdin.close()
        child.stdout.close()
        child.stderr.close()


with tempfile.TemporaryDirectory(prefix="phenix-p1-launcher-") as temp:
    root = Path(temp)
    home = root / "home"
    home.mkdir(mode=0o700)
    base = dict(os.environ)
    for name in (
        "PHENIX_CREDENTIAL_FILE_DIRECTORY", "PHENIX_CREDENTIAL_FILE_CONSENT",
        "PHENIX_CREDENTIAL_API_KEY_ENV_NAME", "PHENIX_BASIC_SQLITE_PATH",
        "XDG_STATE_HOME", "OPENAI_API_KEY",
    ):
        base.pop(name, None)
    base["HOME"] = str(home)

    # No consent or directory environment variable: real ACP must initialize.
    launch(base, exercise_key=True)
    home_vault = home / ".local/state/phenix/p1-auth-credentials"
    assert home_vault.is_dir()
    assert stat.S_IMODE(home_vault.stat().st_mode) == 0o700
    assert list(home_vault.glob("*")), "credential import failed to persist"
    for entry in home_vault.iterdir():
        if entry.is_file():
            assert stat.S_IMODE(entry.stat().st_mode) == 0o600, entry
    launch(base)  # persistence and restart must not break initialization

    # Honor configured XDG_STATE_HOME instead of creating files under HOME.
    xdg = root / "xdg"
    xdg_env = dict(base, XDG_STATE_HOME=str(xdg))
    launch(xdg_env, exercise_key=True)
    xdg_vault = xdg / "phenix/p1-auth-credentials"
    assert xdg_vault.is_dir()
    assert stat.S_IMODE(xdg_vault.stat().st_mode) == 0o700
    assert (xdg_vault / next(iter(home_vault.iterdir())).name).exists()

    # Explicit opt-out must never be overridden by the launch wrapper.
    denied = dict(base, XDG_STATE_HOME=str(root / "denied"),
                  PHENIX_CREDENTIAL_FILE_CONSENT="0")
    launch(denied, expected_start=False)
    assert not (root / "denied").exists()

    invalid = dict(base, XDG_STATE_HOME="relative-state")
    launch(invalid, expected_start=False)

    # Existing symlink must fail closed rather than store a secret elsewhere.
    malicious = root / "malicious"
    malicious.mkdir(mode=0o700)
    link_parent = root / "linked" / "phenix"
    link_parent.mkdir(parents=True, mode=0o700)
    (link_parent / "p1-auth-credentials").symlink_to(malicious, target_is_directory=True)
    launch(dict(base, XDG_STATE_HOME=str(root / "linked")), expected_start=False)
    assert not list(malicious.iterdir())

output = Path(destination)
output.mkdir(parents=True, exist_ok=True)
(output / "accepted.json").write_text(json.dumps({
    "default_no_env_acp_initialized": True,
    "api_key_stored_in_private_xdg_state": True,
    "restart_survives": True,
    "explicit_opt_out_denied": True,
    "relative_xdg_denied": True,
    "symlink_vault_denied": True,
}, sort_keys=True))
