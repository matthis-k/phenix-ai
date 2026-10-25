"""Native product: both real provider auth/catalogs implement the same port.

No user credentials or network logins are required. Each provider is installed
independently with an exact-account kernel grant and a private credential backend.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import secrets
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

kernel, api, chatgpt, credentials, definitions, output, api_fixture = sys.argv[1:]
result_dir = Path(output)
result_dir.mkdir(parents=True, exist_ok=True)
contract = "phenix.provider.runtime@1"
credential_contract = "phenix.provider.credentials@1"

def encode(v):
    if v is None:
        return b"\x00"
    if isinstance(v, bool):
        return b"\x02" if v else b"\x01"
    if isinstance(v, int):
        return b"\x04" + v.to_bytes(8, "big")
    if isinstance(v, bytes):
        return b"\x06" + len(v).to_bytes(4, "big") + v
    if isinstance(v, str):
        raw = v.encode()
        return b"\x05" + len(raw).to_bytes(4, "big") + raw
    if isinstance(v, list):
        return b"\x07" + len(v).to_bytes(4, "big") + b"".join(map(encode, v))
    if isinstance(v, dict):
        fields = sorted(v.items())
        return b"\x08" + len(fields).to_bytes(4, "big") + b"".join(
            len(k.encode()).to_bytes(4, "big") + k.encode() + encode(value)
            for k, value in fields
        )
    raise TypeError(type(v))

def identity(connection):
    return dict(provider_id="openai", connection_id=connection, profile_id="personal")

def resource(account):
    return "phenix.credentials." + ".".join(
        account[field].encode().hex()
        for field in ("provider_id", "connection_id", "profile_id")
    )

with tempfile.TemporaryDirectory(prefix="phenix-concrete-provider-") as temp:
    root = Path(temp)
    vault = root / "vault"
    vault.mkdir(mode=0o700)
    env = dict(os.environ)
    env["PHENIX_CREDENTIAL_FILE_DIRECTORY"] = str(vault)
    env["PHENIX_CREDENTIAL_FILE_CONSENT"] = "1"
    env.pop("PHENIX_CREDENTIAL_API_KEY_ENV_NAME", None)
    def invoke(connection, operation, request, *, root_grant=True, fixture=False):
        provider = "phenix.provider." + ("openai-api" if connection == "api" else "chatgpt")
        selected = resource(identity(connection))
        grants = [
            dict(caller="root", interface=contract, operation=operation,
                 resources=[selected if root_grant else "phenix.credentials.foreign"]),
            dict(caller=provider, interface=credential_contract, operation="resolve",
                 resources=[selected]),
        ]
        package = (api_fixture if fixture else api) if connection == "api" else chatgpt
        composition = dict(
            format="phenix.composition.v3",
            entry=dict(interface=contract, operation=operation, provider=provider),
            artifacts=[package, credentials],
            definitions=[definitions],
            slots=[dict(caller=provider, slot="Credentials", index=0,
                        provider="phenix.provider.credential-files", export="Credentials")],
            grants=grants,
            excluded=[],
            substitutions=[],
            authority=dict(
                operations=[contract + "/" + operation, credential_contract + "/resolve"],
                resources=[selected],
            ),
            resource=selected,
        )
        (root / "composition.json").write_text(json.dumps(composition))
        (root / "request.bin").write_bytes(b"PV\x02\0" + encode(request))
        process = subprocess.run(
            [kernel, "invoke", str(root / "composition.json"), str(root / "request.bin")],
            capture_output=True, text=True, env=env, timeout=30,
        )
        assert process.stdout, (connection, operation, process.returncode,
                                process.stderr[-300:])
        report = json.loads(process.stdout)
        return report.get("outcome"), process.returncode

    for connection in ("api", "chatgpt"):
        account = identity(connection)
        status, code = invoke(connection, "authenticate", {
            "Status": {"query": {
                "scope": "user", "provider_id": "openai", "connection_id": connection,
            }}
        })
        assert code == 0 and status["kind"] == "success", (connection, status)
        details = status["value"]["Status"]["value"]
        assert details["provider_id"] == "openai", details
        assert details["connection_id"] == connection, details
        assert not any("Authenticated" in p["status"] for p in details["profiles"]), details

        query = dict(scope="user", provider_id="openai", connection_id=connection,
                     profile_id=["personal"], cursor=None, limit=10)
        models, code = invoke(connection, "models", query)
        assert code == 0 and models["kind"] == "domain_error", (connection, models)

        foreign, code = invoke(connection, "models", {
            **query, "connection_id": "chatgpt" if connection == "api" else "api",
        })
        assert code == 0 and foreign["kind"] == "domain_error", foreign

        denied, code = invoke(connection, "models", query, root_grant=False)
        assert code != 0 and denied["kind"] == "kernel_failure", denied
        assert denied["failure"]["code"] == "Denied", denied

    challenge, code = invoke("api", "authenticate", {
        "Begin": {"request": dict(
            scope="user", provider_id="openai", connection_id="api",
            method_id="api-key",
        )}
    })
    assert code == 0 and challenge["kind"] == "success", challenge
    assert "ApiKeyInput" in challenge["value"]["Challenge"]["value"], challenge

    # A real verified API key follows the same canonical methods. Only the
    # transport URL is changed by the test-specific feature; production still
    # pins HTTPS. No token is serialized into a report, log or Nix output.
    secret = secrets.token_hex(24).encode()
    class Catalog(BaseHTTPRequestHandler):
        def log_message(self, *_): pass
        def do_GET(self):
            if self.path != "/models" or self.headers.get("Authorization") != (
                "Bearer " + secret.decode()
            ):
                self.send_response(403)
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            raw = b'{"object":"list","data":[{"id":"gpt-5"}]}'
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)

    server = ThreadingHTTPServer(("127.0.0.1", 0), Catalog)
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    try:
        env["PHENIX_C11_TEST_BASE"] = "http://127.0.0.1:" + str(server.server_port)
        account = identity("api")
        selected = resource(account)
        imported = dict(
            format="phenix.composition.v3",
            entry=dict(interface=credential_contract, operation="import",
                       provider="phenix.provider.credential-files"),
            artifacts=[credentials],
            definitions=[definitions],
            slots=[],
            grants=[dict(caller="root", interface=credential_contract,
                         operation="import", resources=[selected])],
            excluded=[], substitutions=[],
            authority=dict(
                operations=[credential_contract + "/import"],
                resources=[selected],
            ),
            resource=selected,
        )
        (root / "composition.json").write_text(json.dumps(imported))
        (root / "request.bin").write_bytes(b"PV\x02\0"
                                           + encode(dict(identity=account, secret=secret)))
        imported_result = subprocess.run(
            [kernel, "invoke", str(root / "composition.json"), str(root / "request.bin")],
            env=env, capture_output=True, text=True, timeout=30,
        )
        assert imported_result.returncode == 0, (
            imported_result.returncode, "credential import denied",
        )
        imported_outcome = json.loads(imported_result.stdout).get("outcome")
        assert imported_outcome and imported_outcome["kind"] == "success", (
            "credential import did not succeed",
        )
        valid, code = invoke("api", "authenticate", {
            "Status": {"query": {
                "scope": "user", "provider_id": "openai", "connection_id": "api",
            }},
        }, fixture=True)
        assert code == 0 and valid["kind"] == "success", valid
        profiles = valid["value"]["Status"]["value"]["profiles"]
        assert any("Authenticated" in p["status"] for p in profiles), profiles

        allowed_query = dict(scope="user", provider_id="openai", connection_id="api",
                             profile_id=["personal"], cursor=None, limit=10)
        listed, code = invoke("api", "models", allowed_query, fixture=True)
        assert code == 0 and listed["kind"] == "success", listed
        assert [m["id"] for m in listed["value"]["models"]] == ["gpt-5"], listed
        completion = dict(
            scope="user", provider_id="openai", connection_id="api",
            profile_id=["personal"], model_id="gpt-5",
            request=dict(
                session="session-1", run="run-1", history=[], tools=[], skills=[],
                context=[], limits=dict(
                    model_turns=None, tool_calls=None, model_tokens=None,
                    http_timeout_ms=None,
                ),
            ),
        )
        selected_model, code = invoke("api", "complete", completion, fixture=True)
        assert code == 0 and selected_model["kind"] == "domain_error", selected_model
        assert selected_model["value"]["code"] == "Unavailable", selected_model
        unknown_model, code = invoke("api", "complete", {
            **completion, "model_id": "unlisted",
        }, fixture=True)
        assert code == 0 and unknown_model["kind"] == "domain_error", unknown_model
        assert unknown_model["value"]["code"] == "NotFound", unknown_model
    finally:
        server.shutdown()
        server.server_close()
        worker.join(timeout=5)

    (result_dir / "verified.json").write_text(json.dumps({
        "format": "phenix.provider.concrete-port.product.v1",
        "real_providers": ["openai/api", "openai/chatgpt"],
        "authenticated_profiles_not_inferred": True,
        "api_key_challenge_live": True,
        "real_api_key_verification": True,
        "selected_model_admitted": True,
        "missing_protocol_explicit": True,
        "model_catalog_reuses_real_auth": True,
        "foreign_account_rejected": True,
        "kernel_scope_denial": True,
    }, indent=2, sort_keys=True))
