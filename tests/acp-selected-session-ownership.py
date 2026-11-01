"""Real ACP/SQLite/selected HTTP ownership acceptance, without timing proofs.

Usage: python acp-selected-session-ownership.py RUNTIME HTTP_COMPOSITION
       LOOPBACK_MANAGEMENT OUTPUT_DIRECTORY
Timeouts bound failures; HTTP Events establish the tested ordering.
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


runtime, composition, management, destination = sys.argv[1:]
output = Path(destination)
output.mkdir(parents=True, exist_ok=True)
database = output / "ownership.sqlite"
entered = queue.Queue()
unblock = threading.Event()
children = []
TIMEOUT = 45
SELECTED = "phenix.provider.selected-model.v1"


class HTTP(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_):
        pass

    def do_POST(self):
        payload = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        assert self.path == "/completions", self.path
        assert self.headers.get("Authorization") is None
        assert payload["model"] == "fixture-a", payload
        entered.put(payload)
        assert unblock.wait(TIMEOUT), "HTTP barrier not released"
        data = ('data: ' + json.dumps({"choices": [{"index": 0, "delta": {
            "content": "ownership-ok"}, "finish_reason": "stop"}]})
            + '\r\n\r\ndata: ' + json.dumps({"choices": [], "usage": {
                "prompt_tokens": 4, "completion_tokens": 2}})
            + '\r\n\r\ndata: [DONE]\r\n\r\n').encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)
        self.wfile.flush()


server = ThreadingHTTPServer(("127.0.0.1", 0), HTTP)
threading.Thread(target=server.serve_forever, daemon=True).start()


def start():
    env = dict(os.environ)
    env.update(
        PHENIX_BASIC_SQLITE_PATH=str(database),
        PHENIX_C9_TEST_HTTP_ENDPOINT=f"http://127.0.0.1:{server.server_port}/completions",
        PHENIX_MODEL_HTTP_ENDPOINT="http://127.0.0.1:1/must-not-run",
        PHENIX_MODEL_HTTP_MODEL="must-not-run",
        PHENIX_MODEL_HTTP_API_KEY="must-not-use",
    )
    child = subprocess.Popen(
        [runtime, "acp", composition, "--provider-management", management],
        env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, text=True, bufsize=1,
    )
    children.append(child)
    child.frames = queue.Queue()

    def read():
        for line in child.stdout:
            child.frames.put(json.loads(line))
        child.frames.put(None)

    threading.Thread(target=read, daemon=True).start()
    assert call(child, 1, "initialize", {"protocolVersion": 1,
                "clientCapabilities": {}})["result"]["protocolVersion"] == 1
    return child


def send(child, number, method, params):
    child.stdin.write(json.dumps({"jsonrpc": "2.0", "id": number,
                                 "method": method, "params": params}) + "\n")
    child.stdin.flush()


def response(child, number):
    for _ in range(256):
        frame = child.frames.get(timeout=TIMEOUT)
        assert frame is not None, (child.poll(), "ACP closed")
        if frame.get("method") == "session/update":
            continue
        assert frame.get("id") == number, frame
        return frame
    raise AssertionError("too many updates")


def call(child, number, method, params):
    send(child, number, method, params)
    return response(child, number)


def new_selected(child):
    created = call(child, 2, "session/new", {"cwd": "/tmp", "mcpServers": []})
    session = created["result"]["sessionId"]
    options = created["result"]["configOptions"][0]["options"]
    choice = next(option["value"] for option in options
                  if option["value"].startswith("phenix.native-model@1:")
                  and json.loads(option["value"].split(":", 1)[1])["model_id"] == "fixture-a")
    assert "result" in call(child, 3, "session/set_config_option", {
        "sessionId": session, "configId": "model", "value": choice})
    return session


def prompt(session, text):
    return {"sessionId": session, "prompt": [{"type": "text", "text": text}]}


try:
    owner = start()
    session = new_selected(owner)
    unblock.set()
    warmup = call(owner, 3, "session/prompt", prompt(session, "checkpoint-for-load"))
    assert warmup.get("result", {}).get("stopReason") == "end_turn", warmup
    entered.get(timeout=TIMEOUT)
    unblock.clear()
    contender = start()
    loaded = call(contender, 2, "session/load", {
        "sessionId": session, "cwd": "/tmp", "mcpServers": []})
    assert "result" in loaded, loaded
    independent = new_selected(contender)
    assert independent != session

    send(owner, 4, "session/prompt", prompt(session, "held-owner"))
    entered.get(timeout=TIMEOUT)  # Owner is executing a real pending Model call.

    # Poison the persisted selection only after both clients have loaded it.
    # Lock rejection must precede even reading it, not merely graph admission.
    with sqlite3.connect(database) as db:
        original = db.execute("SELECT data FROM basic_records WHERE session=? AND name=?",
                              (session, SELECTED)).fetchone()[0]
        db.execute("UPDATE basic_records SET data=? WHERE session=? AND name=?",
                   (b"invalid-selection-barrier", session, SELECTED))
        before = db.execute(
            "SELECT name, revision, data FROM basic_records WHERE session=? ORDER BY name",
            (session,)).fetchall()
    malformed = call(contender, 4, "session/prompt", prompt(session, ""))
    assert malformed["error"]["code"] == -32602, malformed
    assert malformed["error"]["data"] == "empty prompt", malformed
    denied = call(contender, 5, "session/prompt", prompt(session, "contender"))
    assert denied["error"]["data"]["code"] == "StorageFailed", denied
    assert "session already owned" in denied["error"]["data"]["message"], denied
    assert entered.empty(), "rejected contender reached HTTP"
    with sqlite3.connect(database) as db:
        after = db.execute(
            "SELECT name, revision, data FROM basic_records WHERE session=? ORDER BY name",
            (session,)).fetchall()
        assert after == before, "contender changed the live owner's durable state"
        db.execute("UPDATE basic_records SET data=? WHERE session=? AND name=?",
                   (original, session, SELECTED))

    # The second session must reach its own real HTTP call while the first is
    # still held, proving the owner lock is session-scoped, not database-wide.
    send(contender, 6, "session/prompt", prompt(independent, "independent"))
    entered.get(timeout=TIMEOUT)
    assert not unblock.is_set()
    unblock.set()
    for child, number in ((owner, 4), (contender, 6)):
        result = response(child, number)
        assert result.get("result", {}).get("stopReason") == "end_turn", result

    # Final response includes release/drain; a different process can now use
    # the original session, including recovery and a fresh selected install.
    resumed = start()
    loaded = call(resumed, 2, "session/load", {
        "sessionId": session, "cwd": "/tmp", "mcpServers": []})
    assert "result" in loaded, loaded
    result = call(resumed, 7, "session/prompt", prompt(session, "after-release"))
    assert result.get("result", {}).get("stopReason") == "end_turn", result
    entered.get(timeout=TIMEOUT)
    assert entered.empty()
    (output / "ownership.json").write_text(json.dumps({
        "same_session_rejected_before_selection_read": True,
        "frontend_decode_precedes_ownership": True,
        "contender_did_not_recover_or_mutate_live_owner": True,
        "independent_session_entered_before_owner_unblocked": True,
        "same_session_reusable_after_completion": True,
        "sleeps_used": False,
    }, indent=2) + "\n")
finally:
    unblock.set()
    for child in children:
        child.stdin.close()
    for child in children:
        try:
            status = child.wait(timeout=TIMEOUT)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()
            raise
        diagnostic = child.stderr.read()
        child.stdout.close()
        child.stderr.close()
        assert status == 0, (status, diagnostic)
    server.shutdown()
    server.server_close()

print("Selected ACP same-session exclusion and independent-session progress passed")
