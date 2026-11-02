"""Authenticated selected JSONL through the installed exact-account resolver."""
import json
import queue
import secrets
import sqlite3
import subprocess
import threading
import time

from model_protocol_fixture import ResponsesWire
from selected_provider_jsonl import assert_request_failure


def exercise(global_scope, scope):
    call, acp = scope["call"], scope["p"]
    original_key = global_scope["key"]
    rotated_key = secrets.token_hex(32)
    secrets_used = (original_key, rotated_key, "no-preset-token")
    requests = []
    errors = queue.Queue()
    entered, drained = threading.Event(), threading.Event()
    live_delta = threading.Event()
    mode = {"value": "reactive"}
    clients = []
    timeout = 45

    def handle(http, packet):
        try:
            requests.append(packet["model"])
            writer_parts = []
            wire = ResponsesWire(writer_parts.append)
            if mode["value"] == "tool-only":
                wire.chunk({"choices": [{"delta": {"tool_calls": [{
                    "index": 0, "id": "incomplete-jsonl-tool", "function": {
                        "name": "uppercase", "arguments": '{"value":"',
                    },
                }]}}]})
                prefix = b"".join(writer_parts)
                http.send_response(200)
                http.send_header("Content-Type", "text/event-stream")
                http.send_header("Content-Length", "1000000")
                http.end_headers()
                http.wfile.write(prefix)
                http.wfile.flush()
                entered.set()
                http.connection.settimeout(timeout)
                try:
                    assert http.connection.recv(1) == b"", "cancel did not close socket"
                except ConnectionResetError:
                    pass
                try:
                    http.wfile.write(b"data: forbidden-auth-late\n\n")
                    http.wfile.flush()
                except (BrokenPipeError, ConnectionResetError):
                    pass
                http.close_connection = True
                drained.set()
                return
            wire.chunk({"choices": [{"delta": {"content": "authenticated-jsonl"}}]})
            wire.chunk({"choices": [], "usage": {"prompt_tokens": 5, "completion_tokens": 4}})
            wire.done()
            payload = b"".join(writer_parts)
            http.send_response(200)
            http.send_header("Content-Type", "text/event-stream")
            http.send_header("Content-Length", str(len(payload)))
            http.end_headers()
            if mode["value"] == "reactive":
                boundary = payload.index(b"event: response.completed")
                http.wfile.write(payload[:boundary])
                http.wfile.flush()
                assert live_delta.wait(timeout), "JSONL buffered TextDelta until completion"
                http.wfile.write(payload[boundary:])
            else:
                http.wfile.write(payload)
            http.wfile.flush()
        except Exception as error:
            errors.put(type(error).__name__)
            entered.set()
            drained.set()

    def launch(resolver=None):
        child = subprocess.Popen([
            scope["runtime"], "jsonl", scope["http_basic"],
            "--provider-management", scope["configs"]["management"],
            "--provider-credentials", scope["configs"]["credential"],
            "--provider-model-resolver", resolver or scope["configs"]["resolver"],
        ], env=scope["env"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, text=True, bufsize=1)
        scope["children"].append(child)
        clients.append(child)
        child.frames, child.events = queue.Queue(), []

        def read():
            for line in child.stdout:
                child.frames.put(json.loads(line))
            child.frames.put(None)

        threading.Thread(target=read, daemon=True).start()
        return child

    def send(child, session, text=None):
        packet = {"type": "cancel" if text is None else "prompt", "session": session}
        if text is not None:
            packet["text"] = text
        child.stdin.write(json.dumps(packet) + "\n")
        child.stdin.flush()

    def terminal(child, session, kind):
        deadline = time.monotonic() + timeout
        while True:
            event = child.frames.get(timeout=max(0.001, deadline - time.monotonic()))
            assert event is not None, "authenticated JSONL closed before terminal"
            child.events.append(event)
            assert not any(secret in json.dumps(event) for secret in secrets_used), "secret in JSONL output"
            if event["kind"] == "TextDelta" and event["text"] == "authenticated-jsonl":
                live_delta.set()
            if event["session"] == session and event["kind"] in ("Answer", "Failure", "Cancelled"):
                assert event["kind"] == kind, event
                return event

    def seed():
        created = call(acp, 300, "session/new", {"cwd": "/tmp", "mcpServers": []})
        session = created["result"]["sessionId"]
        choices = [o["value"] for o in created["result"]["configOptions"][0]["options"]
                   if o["value"].startswith("phenix.native-model@1:")]
        assert len(choices) == 1, created
        assert "result" in call(acp, 301, "session/set_config_option", {
            "sessionId": session, "configId": "model", "value": choices[0],
        })
        return session

    def credential(secret):
        result = call(acp, 302, "_phenix/credentials/import", {**scope["account"], "secret": secret})
        assert result["result"]["ok"] is True

    global_scope["controlled_post"] = handle
    try:
        session = seed()
        child = launch()
        send(child, session, "authenticated-first")
        assert terminal(child, session, "Answer")["text"] == "authenticated-jsonl"
        assert requests == ["gpt-5"]
        assert live_delta.is_set()
        mode["value"] = "success"
        # Change the secret through the legitimate credential API while the same
        # JSONL resolver/controller remains alive. HTTP checks the new exact key.
        global_scope["key"] = rotated_key
        credential(rotated_key)
        send(child, session, "authenticated-after-rotation")
        assert terminal(child, session, "Answer")["text"] == "authenticated-jsonl"
        assert requests == ["gpt-5"] * 2
        revoked = call(acp, 303, "_phenix/credentials/revoke", scope["account"])
        assert revoked["result"]["ok"] is True
        send(child, session, "revoked-must-not-run")
        denied = terminal(child, session, "Failure")
        assert_request_failure(denied)
        assert len(requests) == 2
        credential(rotated_key)
        send(child, session, "restored-same-controller")
        terminal(child, session, "Answer")
        assert len(requests) == 3

        wrong = json.loads(scope["Path"](scope["configs"]["resolver"]).read_text())
        wrong_resource = "phenix.credentials." + ".".join(
            value.encode().hex() for value in ("openai", "api", "work"))
        wrong["resource"] = wrong_resource
        wrong["authority"]["resources"] = [wrong_resource]
        for grant in wrong["grants"]:
            grant["resources"] = [wrong_resource]
        wrong_path = scope["root"] / "wrong-jsonl-resolver.json"
        wrong_path.write_text(json.dumps(wrong))
        other = launch(str(wrong_path))
        send(other, session, "wrong-account-resolver")
        assert_request_failure(terminal(other, session, "Failure"))
        assert len(requests) == 3

        cancelled = seed()
        mode["value"] = "tool-only"
        send(child, cancelled, "partial-tool-without-text")
        assert entered.wait(timeout), "tool-only HTTP not admitted"
        assert errors.empty(), list(errors.queue)
        with sqlite3.connect(scope["root"] / "selected.sqlite") as db:
            (journal,) = db.execute(
                "SELECT data FROM basic_records WHERE session=? AND name='event-journal.v1'",
                (cancelled,),
            ).fetchone()
        assert journal.count(b"ModelStarted") == 1
        assert all(journal.count(kind) == 0 for kind in
                   (b"TextDelta", b"ToolStarted", b"ToolFinished", b"Answer", b"Cancelled"))
        send(child, cancelled)
        terminal(child, cancelled, "Cancelled")
        cancelled_events = [e for e in child.events if e["session"] == cancelled]
        assert not any(e["kind"] in ("TextDelta", "ToolStarted", "ToolFinished", "Answer")
                       for e in cancelled_events), cancelled_events
        assert drained.wait(timeout), "tool-only transport not drained"
        assert errors.empty(), list(errors.queue)
        assert len(requests) == 4
        mode["value"] = "success"
        send(child, cancelled, "after-tool-only-cancel")
        terminal(child, cancelled, "Answer")
        assert len(requests) == 5
        assert [e["kind"] for e in child.events if e["session"] == cancelled
                and e["kind"] in ("Answer", "Failure", "Cancelled")] == ["Cancelled", "Answer"]
        assert not any(e["session"] == cancelled and e["kind"] in ("ToolStarted", "ToolFinished")
                       for e in child.events), child.events
    finally:
        global_scope["key"] = original_key
        credential(original_key)
        for child in clients:
            if child.poll() is None:
                child.stdin.close()
            status = child.wait(timeout=timeout)
            stderr = child.stderr.read()
            assert status == 0, (status, "authenticated JSONL failed")
            assert not any(secret in stderr for secret in secrets_used), "secret in stderr"
            while True:
                event = child.frames.get(timeout=timeout)
                if event is None:
                    break
                child.events.append(event)
            encoded = json.dumps(child.events)
            assert not any(secret in encoded for secret in secrets_used), "secret in events"
            assert "forbidden-auth-late" not in encoded
        global_scope["controlled_post"] = None
    (scope["output"] / "selected-authenticated-jsonl.json").write_text(json.dumps({
        "actual_authenticated_http_requests": len(requests),
        "exact_account_resolver": True,
        "live_credential_rotation_revalidated": True,
        "live_revocation_denied_zero_http": True,
        "wrong_account_resolver_denied_zero_http": True,
        "tool_only_cancellation_peer_closed": True,
        "same_controller_recovered": True,
        "text_delta_before_http_completion": live_delta.is_set(),
        "secrets_redacted": True,
        "sleeps_used": False,
    }, indent=2) + "\n")
