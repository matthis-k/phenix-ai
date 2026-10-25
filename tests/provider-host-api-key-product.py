"""Native product: shared data-driven host performs account-bound API key auth.

The conventional provider route is selected by registration data. The native
host owns auth and model discovery; no concrete provider auth/catalog operation
is permitted to answer these root calls. Every secret stays in a private store
or the HTTP test fixture, never in a report or subprocess arguments.
"""
import json
import os
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import secrets
import subprocess
import sys
import tempfile
import threading

kernel, host, contributor, credentials, definitions, destination = sys.argv[1:]
out = Path(destination)
out.mkdir(parents=True, exist_ok=True)

IDENTITY = dict(provider_id="openai", connection_id="api", profile_id="personal")
RESOURCE = "phenix.credentials." + ".".join(
    IDENTITY[key].encode().hex() for key in ("provider_id", "connection_id", "profile_id")
)
AUTH = "phenix.provider.auth@1"
RUNTIME = "phenix.provider.runtime@1"
CATALOG = "phenix.provider.catalog@1"
CREDS = "phenix.provider.credentials@1"
REG = "phenix.provider.registrations@1"


def encode(value):
    if value is None:
        return b"\x00"
    if isinstance(value, bool):
        return b"\x02" if value else b"\x01"
    if isinstance(value, bytes):
        return b"\x06" + len(value).to_bytes(4, "big") + value
    if isinstance(value, int):
        return b"\x04" + value.to_bytes(8, "big")
    if isinstance(value, str):
        raw = value.encode()
        return b"\x05" + len(raw).to_bytes(4, "big") + raw
    if isinstance(value, list):
        return b"\x07" + len(value).to_bytes(4, "big") + b"".join(
            encode(item) for item in value
        )
    if isinstance(value, dict):
        pairs = sorted(value.items())
        return b"\x08" + len(pairs).to_bytes(4, "big") + b"".join(
            len(key.encode()).to_bytes(4, "big") + key.encode() + encode(v)
            for key, v in pairs
        )
    raise TypeError(type(value))


with tempfile.TemporaryDirectory(prefix="phenix-host-key-") as temporary:
    root = Path(temporary)
    vault = root / "vault"
    vault.mkdir(mode=0o700)
    env = dict(os.environ)
    env.update(
        PHENIX_CREDENTIAL_FILE_DIRECTORY=str(vault),
        PHENIX_CREDENTIAL_FILE_CONSENT="1",
    )
    env.pop("PHENIX_CREDENTIAL_API_KEY_ENV_NAME", None)
    key = secrets.token_hex(24).encode()

    class Catalog(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_GET(self):
            if (self.path != "/models"
                    or self.headers.get("Authorization") != "Bearer " + key.decode()):
                self.send_response(401)
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            body = (
                b'{"object":"list","data":['
                b'{"id":"gpt-5"},{"id":"text-embedding-3-large"}]}'
            )
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    server = ThreadingHTTPServer(("127.0.0.1", 0), Catalog)
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    env["PHENIX_C11_TEST_BASE"] = f"http://127.0.0.1:{server.server_port}"

    def invoke(interface, operation, request, *, source=RESOURCE, authorized=True,
               entry="phenix.provider.host"):
        selected = source if authorized else "phenix.credentials.unrelated"
        root_op = f"{interface}/{operation}"
        composition = dict(
            format="phenix.composition.v3",
            entry=dict(interface=interface, operation=operation, provider=entry),
            artifacts=[host, contributor, credentials] if entry == "phenix.provider.host"
                      else [credentials],
            definitions=[definitions],
            slots=[
                dict(caller="phenix.provider.host", slot="Contributors",
                     provider="phenix.provider.openai-api", export="Registrations",
                     index=0),
                dict(caller="phenix.provider.host", slot="Credentials",
                     provider="phenix.provider.credential-files", export="Credentials",
                     index=0),
                dict(caller="phenix.provider.openai-api", slot="Credentials",
                     provider="phenix.provider.credential-files", export="Credentials",
                     index=0),
            ] if entry == "phenix.provider.host" else [],
            grants=[
                dict(caller="root", interface=interface, operation=operation,
                     resources=[selected]),
                *([
                    dict(caller="phenix.provider.host", interface=REG, operation="list",
                         resources=[source]),
                    dict(caller="phenix.provider.host", interface=CREDS, operation="resolve",
                         resources=[source]),
                    # Graph handlers are separate, explicitly scoped operations.
                    # Merely installing a provider host grants neither one.
                    dict(caller="phenix.provider.host",
                         interface="phenix.provider.startup@1", operation="begin",
                         resources=[source]),
                    dict(caller="phenix.provider.host",
                         interface="phenix.provider.settlement@1", operation="complete",
                         resources=[source]),
                ] if entry == "phenix.provider.host" else []),
            ],
            excluded=[],
            substitutions=[],
            authority=dict(
                operations=[root_op] + ([
                    REG + "/list", CREDS + "/resolve",
                    "phenix.provider.startup@1/begin",
                    "phenix.provider.settlement@1/complete",
                ] if entry == "phenix.provider.host" else []),
                resources=[source],
            ),
            resource=source,
        )
        (root / "composition.json").write_text(json.dumps(composition))
        (root / "request.bin").write_bytes(b"PV\x02\0" + encode(request))
        proc = subprocess.run(
            [kernel, "invoke", str(root / "composition.json"), str(root / "request.bin")],
            capture_output=True, text=True, env=env, timeout=30,
        )
        assert proc.stdout, ("missing native response", proc.returncode, proc.stderr[-150:])
        report = json.loads(proc.stdout)
        if "outcome" not in report:
            raise AssertionError({
                "operation": root_op, "exit_code": proc.returncode,
                "last_failure": report.get("last_failure"),
                "stderr": proc.stderr[-4000:],
            })
        return report["outcome"], proc.returncode

    try:
        query = dict(scope="user", provider_id="openai", connection_id="api")
        before, code = invoke(AUTH, "status", query)
        assert code == 0 and before["kind"] == "success", before
        profiles = before["value"]["profiles"]
        assert len(profiles) == 1, profiles
        assert "Unavailable" in profiles[0]["status"], profiles

        challenge, code = invoke(AUTH, "begin", {
            **query,
            "method_id": "api-key",
        })
        assert code == 0 and challenge["kind"] == "success", challenge
        assert "ApiKeyInput" in challenge["value"], challenge
        assert "secret" not in repr(challenge).lower(), "challenge must not expose key"

        wrong, code = invoke(AUTH, "status", {
            **query,
            "connection_id": "chatgpt",
        })
        assert code == 0 and wrong["kind"] == "domain_error", wrong
        assert wrong["value"]["code"] == "Denied", wrong

        rejected, code = invoke(AUTH, "status", query, authorized=False)
        assert code != 0 and rejected["kind"] == "kernel_failure", rejected
        assert rejected["failure"]["code"] == "Denied", rejected

        imported, code = invoke(
            CREDS, "import",
            dict(identity=IDENTITY, secret=key),
            entry="phenix.provider.credential-files",
        )
        assert code == 0 and imported["kind"] == "success", imported

        after, code = invoke(AUTH, "status", query)
        assert code == 0 and after["kind"] == "success", after
        profiles = after["value"]["profiles"]
        assert len(profiles) == 1 and "Authenticated" in profiles[0]["status"], profiles
        assert profiles[0]["id"] == "personal", profiles

        page, code = invoke(CATALOG, "models", {
            **query, "profile_id": ["personal"], "cursor": None, "limit": 20,
        })
        assert code == 0 and page["kind"] == "success", page
        assert page["value"]["profile_id"] == ["personal"], page
        assert [m["id"] for m in page["value"]["models"]] == ["gpt-5"], page

        # The canonical port MUST reuse these very same host-owned auth and
        # discovery decisions; provider implementations do not get a second
        # state machine or alternative model verification path.
        canonical_status, code = invoke(RUNTIME, "authenticate", {
            "Status": {"query": query},
        })
        assert code == 0 and canonical_status["kind"] == "success", canonical_status
        assert canonical_status["value"]["Status"]["value"] == after["value"], (
            canonical_status, after,
        )
        canonical_models, code = invoke(RUNTIME, "models", {
            **query, "profile_id": ["personal"], "cursor": None, "limit": 20,
        })
        assert code == 0 and canonical_models["kind"] == "success", canonical_models
        assert canonical_models["value"] == page["value"], canonical_models

        request = dict(
            scope="user", provider_id="openai", connection_id="api",
            profile_id=["personal"], model_id="gpt-5",
            request=dict(
                session="session-1", run="run-1",
                history=[], tools=[], skills=[], context=[],
                limits=dict(
                    model_turns=None, tool_calls=None,
                    model_tokens=None, http_timeout_ms=None,
                ),
            ),
        )
        selected, code = invoke(RUNTIME, "complete", request)
        assert code == 0 and selected["kind"] == "domain_error", selected
        assert selected["value"]["code"] == "Unavailable", selected

        foreign_model, code = invoke(RUNTIME, "complete", {
            **request, "model_id": "text-embedding-3-large",
        })
        assert code == 0 and foreign_model["kind"] == "domain_error", foreign_model
        assert foreign_model["value"]["code"] == "NotFound", foreign_model

        bad_run, code = invoke(RUNTIME, "complete", {
            **request, "request": {**request["request"], "run": ""},
        })
        assert code == 0 and bad_run["kind"] == "domain_error", bad_run
        assert bad_run["value"]["code"] == "InvalidInput", bad_run

        other, code = invoke(CATALOG, "models", {
            **query, "profile_id": ["other"], "cursor": None, "limit": 20,
        })
        assert code == 0 and other["kind"] == "domain_error", other
        assert other["value"]["code"] == "Denied", other
    finally:
        server.shutdown()
        server.server_close()
        worker.join(timeout=5)

    (out / "verified.json").write_text(json.dumps(dict(
        format="phenix.provider.host-api-key.product.v1",
        host_executes_standard_auth=True,
        key_presence_is_not_authentication=True,
        verified_catalog_and_profile=True,
        exact_credential_authority=True,
        foreign_connection_denied=True,
        no_embedding_models=True,
        canonical_host_contract=True,
        cross_port_exact_model_selection=True,
        completion_requires_executable_adapter=True,
    ), indent=2, sort_keys=True))
