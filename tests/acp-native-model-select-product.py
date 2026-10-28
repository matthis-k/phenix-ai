"""Executable ACP-selected HTTP model through a standalone test-only provider.

Unlike metadata-only provider builtins, this opt-in local loopback fixture
proves native selection, session Storage persistence, HTTP execution, restart,
live second-client session observation, and revocation. No real account,
credential, remote endpoint, or production provider registration is involved.
"""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import queue
import sqlite3
import subprocess
import sys
import threading
import time

runtime, http_basic, management, destination = sys.argv[1:]
output = Path(destination)
output.mkdir(parents=True, exist_ok=True)
sqlite_path = output / "selected-loopback.sqlite"
received = []
children = []
REQUEST_DEADLINE_SECONDS = 45


class ModelHTTP(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_):
        pass

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        assert self.path == "/completions"
        assert self.headers.get("Authorization") is None
        assert request["model"] in ("fixture-a", "fixture-b"), request
        assert request["stream"] is True
        received.append(request["model"])
        stream = [
            {"choices": [{"index": 0, "delta": {"content": "selected-live"},
                           "finish_reason": "stop"}]},
            {"choices": [], "usage": {"prompt_tokens": 4, "completion_tokens": 2}},
        ]
        data = "".join(
            "data: " + json.dumps(frame) + "\r\n\r\n" for frame in stream
        ).encode() + b"data: [DONE]\r\n\r\n"
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)
        self.wfile.flush()


server = ThreadingHTTPServer(("127.0.0.1", 0), ModelHTTP)
threading.Thread(target=server.serve_forever, daemon=True).start()
base_env = dict(os.environ)
base_env.update(
    PHENIX_C9_TEST_HTTP_ENDPOINT=f"http://127.0.0.1:{server.server_port}/completions",
    PHENIX_BASIC_SQLITE_PATH=str(sqlite_path),
    PHENIX_MODEL_HTTP_ENDPOINT="http://127.0.0.1:1/wrong-preset",
    PHENIX_MODEL_HTTP_MODEL="wrong-preset",
    PHENIX_MODEL_HTTP_API_KEY="unselected-wrong-credential",
)


def start(revoked=False):
    env = dict(base_env)
    if revoked:
        env["PHENIX_C9_TEST_REVOKED"] = "1"
    process = subprocess.Popen(
        [runtime, "acp", http_basic, "--provider-management", management],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, bufsize=1, env=env,
    )
    children.append(process)
    process.acp_frames = queue.Queue()

    def read_stdout():
        for line in process.stdout:
            process.acp_frames.put(line)
        process.acp_frames.put(None)

    threading.Thread(target=read_stdout, daemon=True).start()
    return process


def request(process, number, method, params):
    process.stdin.write(json.dumps({
        "jsonrpc": "2.0", "id": number, "method": method, "params": params,
    }) + "\n")
    process.stdin.flush()
    deadline = time.monotonic() + REQUEST_DEADLINE_SECONDS
    for _ in range(256):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise AssertionError(
                f"ACP {method} timed out after {REQUEST_DEADLINE_SECONDS}s "
                f"(session process status: {process.poll()})"
            )
        try:
            line = process.acp_frames.get(timeout=remaining)
        except queue.Empty as error:
            raise AssertionError(
                f"ACP {method} did not respond within {REQUEST_DEADLINE_SECONDS}s "
                f"(process status: {process.poll()})"
            ) from error
        if line is None:
            raise AssertionError(
                f"ACP {method} ended before its response "
                f"(process exit status: {process.poll()})"
            )
        response = json.loads(line)
        if response.get("method") == "session/update":
            continue
        assert response.get("id") == number, response
        return response
    raise AssertionError(f"Too many ACP notifications for {method}")


def initialize(process):
    result = request(process, 1, "initialize", {
        "protocolVersion": 1, "clientCapabilities": {},
    })
    assert result["result"]["protocolVersion"] == 1


def close(process):
    process.stdin.close()
    status = process.wait(timeout=30)
    stderr = process.stderr.read()
    process.stdout.close()
    process.stderr.close()
    assert status == 0, (status, stderr[:1600])


def model_options(response):
    assert "result" in response, response
    choices = response["result"]["configOptions"]
    assert len(choices) == 1 and choices[0]["id"] == "model", choices
    options = choices[0]["options"]
    offered = {
        part["value"]: part["name"] for part in options
        if part["value"].startswith("phenix.native-model@1:")
    }
    assert len(offered) == 2 and len(set(offered)) == 2, offered
    selected = choices[0]["currentValue"]
    return selected, offered


def identity(value):
    assert value.startswith("phenix.native-model@1:"), value
    return json.loads(value.split(":", 1)[1])


def chosen(offered, model):
    matches = [
        value for value in offered
        if identity(value)["model_id"] == model
    ]
    assert len(matches) == 1, (model, offered)
    return matches[0]


def confirmed(response, expected_model, expected_revision):
    current, offered = model_options(response)
    selected = identity(current)
    assert selected["model_id"] == expected_model, selected
    assert selected["session_revision"] == expected_revision, selected
    assert current in offered
    return current, offered


def prompt(process, session, number, text):
    result = request(process, number, "session/prompt", {
        "sessionId": session,
        "prompt": [{"type": "text", "text": text}],
    })
    assert result["result"]["stopReason"] == "end_turn", result


def load(process, session, number):
    return request(process, number, "session/load", {
        "sessionId": session, "cwd": "/tmp", "mcpServers": [],
    })


try:
    # Verify the positive connection-level path against an executable
    # test-only model provider, not just a static state-machine unit test.
    # Choosing a default never creates a session implicitly.
    pre_session = start()
    initialize(pre_session)
    control = request(pre_session, 2, "_phenix/control/state", {})
    assert control["result"]["format"] == "phenix.control.state.v1", control
    assert control["result"]["phase"] == "models_available", control
    assert control["result"]["session_active"] is False, control
    assert control["result"]["selected"] is None, control
    actions = {entry["action"]: entry for entry in control["result"]["actions"]}
    assert actions["models_select"]["allowed"], control
    assert not actions["session_prompt"]["allowed"], control
    offers = {item["id"]: item["name"] for item in control["result"]["models"]}
    assert len(offers) == 2, control
    preselected = chosen(offers, "fixture-b")
    default = request(pre_session, 3, "_phenix/models/select", {
        "value": preselected,
    })
    assert default["result"]["phase"] == "model_selected", default
    assert identity(default["result"]["selected"])["model_id"] == "fixture-b"
    assert identity(default["result"]["selected"])["session_revision"] == 1
    assert "session_active" not in str(default["result"]["phase"])
    created_default = request(pre_session, 4, "session/new", {
        "cwd": "/tmp", "mcpServers": [],
    })
    default_session = created_default["result"]["sessionId"]
    effective_default, _ = confirmed(created_default, "fixture-b", 1)
    assert identity(effective_default)["model_id"] == "fixture-b"
    prompt(pre_session, default_session, 5, "pre-session default used in first turn")
    assert received == ["fixture-b"], received
    close(pre_session)
    # Preserve the existing isolated session-CAS/restart assertions below.
    received.clear()

    original = start()
    initialize(original)
    created = request(original, 2, "session/new", {
        "cwd": "/tmp", "mcpServers": [],
    })
    session = created["result"]["sessionId"]
    preset, initial_offers = model_options(created)
    assert preset == "phenix.model.http"
    a, b = chosen(initial_offers, "fixture-a"), chosen(initial_offers, "fixture-b")

    set_a = request(original, 3, "session/set_config_option", {
        "sessionId": session, "configId": "model", "value": a,
    })
    effective, offered_a = confirmed(set_a, "fixture-a", 1)
    assert identity(a)["session_revision"] == 0
    # Retrying the exact prior choice is safely idempotent.
    repeat = request(original, 4, "session/set_config_option", {
        "sessionId": session, "configId": "model", "value": a,
    })
    assert model_options(repeat)[0] == effective
    prompt(original, session, 5, "first selected turn")
    assert received == ["fixture-a"], received
    close(original)

    # Genuine selected identity and the typed witness must have been committed
    # atomically to the existing database before ACP reported success.
    with sqlite3.connect(sqlite_path) as connection:
        stored = connection.execute(
            "SELECT name, revision FROM basic_records WHERE session = ? "
            "AND name IN (?, ?) ORDER BY name",
            (session, "phenix.provider.selected-model.v1",
             "phenix.provider.native-routing-witness.v1"),
        ).fetchall()
    assert len(stored) == 2 and stored[0][1] == stored[1][1] == 1, stored

    first = start()
    second = start()
    initialize(first)
    initialize(second)
    selected, offers_first = confirmed(load(first, session, 2), "fixture-a", 1)
    assert selected == effective
    selected, offers_second = confirmed(load(second, session, 2), "fixture-a", 1)
    assert selected == effective

    # Two ACP clients have the same observed session revision (1).
    fresh_b = chosen(offers_first, "fixture-b")
    set_b = request(first, 3, "session/set_config_option", {
        "sessionId": session, "configId": "model", "value": fresh_b,
    })
    effective_b, _ = confirmed(set_b, "fixture-b", 2)

    # A stale A choice cannot revert the newer B selection. The previous
    # revision is an idempotent replay only for the SAME effective model.
    stale_a = chosen(offers_second, "fixture-a")
    refused = request(second, 3, "session/set_config_option", {
        "sessionId": session, "configId": "model", "value": stale_a,
    })
    assert "error" in refused, refused
    typed = refused["error"]["data"]
    assert typed["format"] == "phenix.basic.error.v1", refused
    assert typed["code"] == "Conflict", refused
    assert "refresh" in typed["message"], refused

    # The other client's prompt must read committed B from shared Storage.
    prompt(second, session, 4, "fresh cross-client current model")
    assert received == ["fixture-a", "fixture-b"], received
    close(first)
    close(second)

    third = start()
    initialize(third)
    selected, third_offers = confirmed(load(third, session, 2), "fixture-b", 2)
    assert selected == effective_b
    # Refreshing the menu permits deliberate B -> A after rejecting stale A.
    fresh_a = chosen(third_offers, "fixture-a")
    assert identity(fresh_a)["session_revision"] == 2
    prompt(third, session, 3, "survives native controller restart")
    assert received == ["fixture-a", "fixture-b", "fixture-b"], received
    set_fresh_a = request(third, 4, "session/set_config_option", {
        "sessionId": session, "configId": "model", "value": fresh_a,
    })
    current_a, _ = confirmed(set_fresh_a, "fixture-a", 3)
    prompt(third, session, 5, "refreshed menu selects A after B")
    assert received == ["fixture-a", "fixture-b", "fixture-b", "fixture-a"], received
    close(third)

    revoked = start(revoked=True)
    initialize(revoked)
    denied = load(revoked, session, 2)
    assert "error" in denied, denied
    rejected = request(revoked, 3, "session/prompt", {
        "sessionId": session, "prompt": [{"type": "text", "text": "forbidden"}],
    })
    assert "error" in rejected, rejected
    close(revoked)
    assert len(received) == 4

    with sqlite3.connect(sqlite_path) as connection:
        connection.execute(
            "DELETE FROM basic_records WHERE session = ? AND name = ?",
            (session, "phenix.provider.selected-model.v1"),
        )
        connection.commit()
    corrupt = start()
    initialize(corrupt)
    denied = load(corrupt, session, 2)
    assert "error" in denied, denied
    rejected = request(corrupt, 3, "session/prompt", {
        "sessionId": session, "prompt": [{"type": "text", "text": "forbidden"}],
    })
    assert "error" in rejected, rejected
    close(corrupt)
    assert len(received) == 4

    (output / "native-model-acceptance.json").write_text(json.dumps({
        "format": "phenix.c9.selected-http.v1",
        "real_native_http_requests": len(received),
        "runtime_authorized_options": 2,
        "selected_model_CAS": True,
        "atomic_witness": True,
        "restart_reselection": True,
        "second_client_observes_new_model": True,
        "revoked_catalog_denies_load_and_prompt": True,
        "deleted_native_identity_denies_after_restart": True,
        "fallback_environment_ignored": True,
        "production_provider_login": False,
        "stale_client_selection_rejected": True,
        "stale_client_typed_conflict": True,
        "fresh_menu_preconditions": True,
        "refreshed_client_successful_switch": True,
        "contending_client_CAS_conflict": False,
    }, sort_keys=True, indent=2))
    print("C9 test-only authoritative provider native ACP selection verified")
finally:
    # Even assertion failures and ACP request timeouts must not leave native
    # runtime processes or HTTP handler threads keeping the Nix check alive.
    for child in children:
        if child.poll() is None:
            child.terminate()
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait(timeout=5)
        for pipe in (child.stdin, child.stdout, child.stderr):
            if pipe is not None and not pipe.closed:
                pipe.close()
    server.shutdown()
    server.server_close()
