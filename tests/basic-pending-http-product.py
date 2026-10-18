"""End-to-end Basic graph with a real pending HTTP Model and two tool calls."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
import queue
import sqlite3
from pathlib import Path
import subprocess
import sys
import threading
import time

binary, base_composition, http_model, destination, *options = sys.argv[1:]
assert not options or options == ["--advanced-adapter"], options
advanced_adapter = options == ["--advanced-adapter"]
protocol = os.environ.get("PHENIX_MODEL_HTTP_PROTOCOL", "openai-chat")
responses = protocol == "openai-responses"
anthropic = protocol == "anthropic-messages"
if responses or anthropic:
    from model_protocol_fixture import AnthropicWire, ResponsesWire, anthropic_view, chat_view
output = Path(destination)
output.mkdir(parents=True, exist_ok=True)

composition = json.loads(Path(base_composition).read_text())
composition["artifacts"] = [
    item for item in composition["artifacts"] if "phenix-plugin-model-scripted" not in item
] + [http_model]
target_model_caller, target_model_slot = (
    ("phenix.advanced.model-adapter", "Upstream")
    if advanced_adapter else ("phenix.basic.agent", "Model")
)
model_slots = [
    item for item in composition["slots"]
    if item["caller"] == target_model_caller and item["slot"] == target_model_slot
]
assert len(model_slots) == 1
model_slots[0]["provider"] = "phenix.model.http"
composition["slots"].append({
    "caller": "phenix.model.http", "slot": "Journal", "index": 0,
    "provider": "phenix.basic.event-journal", "export": "Journal",
})
composition["grants"].append({
    "caller": "phenix.model.http", "interface": "phenix.basic.journal@1",
    "operation": "append", "resources": ["phenix.session.s1"],
})
composition_file = output / "http-basic.json"
composition_file.write_text(json.dumps(composition, indent=2))

calls = []
lock = threading.Lock()
release_cancel_stream = threading.Event()
release_crash_stream = threading.Event()


class Server(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_):
        pass

    def chunk(self, content):
        if responses or anthropic:
            self.wire.chunk(content)
            return
        payload = f"data: {json.dumps(content, ensure_ascii=False)}\r\n\r\n".encode()
        self.fragment(payload)

    def fragment(self, payload):
        # Deliberately split SSE frames and UTF-8 across HTTP chunks.
        for offset in range(0, len(payload), 2):
            data = payload[offset:offset + 2]
            self.wfile.write(f"{len(data):x}\r\n".encode() + data + b"\r\n")
        self.wfile.flush()

    def do_POST(self):
        assert self.path == "/completions"
        if anthropic:
            assert self.headers["x-api-key"] == "pending-fixture"
            assert self.headers["anthropic-version"] == "2023-06-01"
        else:
            assert self.headers["Authorization"] == "Bearer pending-fixture"
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if responses:
            request = chat_view(request)
            self.wire = ResponsesWire(self.fragment)
        elif anthropic:
            request = anthropic_view(request)
            self.wire = AnthropicWire(self.fragment)
        assert request["stream"] is True
        assert request["stream_options"]["include_usage"] is True
        assert request["model"] == "pending-fixture"
        messages = request["messages"]
        with lock:
            calls.append(request)
            turn = len(calls)
        assert turn in (1, 2, 3, 4, 5, 6, 7), "unexpected model continuation"
        if turn == 1:
            assert messages[-1] == {"role": "user", "content": "two tools"}
        elif turn == 2:
            assert messages[-1]["role"] == "tool"
            assert len([x for x in messages if x["role"] == "tool"]) == 2
            assert {x["tool_call_id"] for x in messages if x["role"] == "tool"} == {"c1", "c2"}
            assert any(x["role"] == "assistant" and len(x.get("tool_calls", [])) == 2 for x in messages)
        elif turn == 3:
            assert messages[-1] == {"role": "user", "content": "cancel during stream"}
        elif turn == 4:
            assert messages[-1] == {"role": "user", "content": "after cancel"}
        elif turn == 5:
            assert messages[-1] == {"role": "user", "content": "second-session"}
        elif turn == 6:
            assert messages[-1] == {"role": "user", "content": "crash model"}
        else:
            assert messages[-1] == {"role": "user", "content": "after crash"}
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Transfer-Encoding", "chunked")
        self.end_headers()
        if turn == 1:
            self.chunk({"choices": [{"index": 0, "delta": {"content": "phase1-"}, "finish_reason": None}]})
            time.sleep(0.03)
            self.chunk({"choices": [{"index": 0, "delta": {"tool_calls": [
                {"index": 0, "id": "c1", "type": "function",
                 "function": {"name": "uppercase", "arguments": "{\"value\":\"hi\"}"}},
                {"index": 1, "id": "c2", "type": "function",
                 "function": {"name": "length", "arguments": "{\"value\":\"hi\"}"}},
            ]}, "finish_reason": "tool_calls"}]})
        elif turn == 2:
            self.chunk({"choices": [{"index": 0, "delta": {"content": "final-"}, "finish_reason": None}]})
            time.sleep(0.03)
            self.chunk({"choices": [{"index": 0, "delta": {"content": "ä"}, "finish_reason": "stop"}]})
        elif turn == 3:
            self.chunk({"choices": [{"index": 0, "delta": {"content": "cancel-start"}, "finish_reason": None}]})
            # Keep the first model stream pending while the second process
            # attempts to enter the same durable session. The main test
            # releases the HTTP handler once cancellation has drained.
            release_cancel_stream.wait(timeout=20.0)
            # The client should have cancelled the transport before the next chunk.
            try:
                self.chunk({"choices": [{"index": 0, "delta": {"content": "too-late"}, "finish_reason": "stop"}]})
            except (BrokenPipeError, ConnectionResetError):
                return
        elif turn == 4:
            self.chunk({"choices": [{"index": 0, "delta": {"content": "resumed"}, "finish_reason": "stop"}]})
        elif turn == 5:
            self.chunk({"choices": [{"index": 0, "delta": {"content": "other-session"}, "finish_reason": "stop"}]})
        elif turn == 6:
            self.chunk({"choices": [{"index": 0, "delta": {"content": "crash-start"}, "finish_reason": None}]})
            release_crash_stream.wait(timeout=20.0)
            try:
                self.chunk({"choices": [{"index": 0, "delta": {"content": "too-late"}, "finish_reason": "stop"}]})
            except (BrokenPipeError, ConnectionResetError):
                return
        else:
            self.chunk({"choices": [{"index": 0, "delta": {"content": "recovered"}, "finish_reason": "stop"}]})
        self.chunk({"choices": [], "usage": {"prompt_tokens": 15, "completion_tokens": 3}})
        if responses or anthropic:
            self.wire.done()
        else:
            done = b"data: [DONE]\r\n\r\n"
            self.wfile.write(f"{len(done):x}\r\n".encode() + done + b"\r\n")
        self.wfile.write(b"0\r\n\r\n")
        self.wfile.flush()


http = ThreadingHTTPServer(("127.0.0.1", 0), Server)
server = threading.Thread(target=http.serve_forever, daemon=True)
server.start()

try:
    env = dict(os.environ)
    env.update(
        PHENIX_MODEL_HTTP_ENDPOINT=f"http://127.0.0.1:{http.server_port}/completions",
        PHENIX_MODEL_HTTP_MODEL="pending-fixture",
        PHENIX_MODEL_HTTP_API_KEY="pending-fixture",
        PHENIX_BASIC_SQLITE_PATH=str(output / "basic-http.db"),
    )
    packet = {"type": "prompt", "session": "s1", "text": "two tools",
              "limits": {"model_turns": 2, "tool_calls": 2, "model_tokens": 20}}
    result = subprocess.run(
        [binary, "jsonl", str(composition_file)],
        input=json.dumps(packet) + "\n", text=True, capture_output=True,
        env=env, timeout=35,
    )
    assert result.returncode == 0, (result.returncode, result.stdout, result.stderr)
    events = [json.loads(line) for line in result.stdout.splitlines()]
    assert len(calls) == 2, f"expected one tool continuation: {len(calls)}"
    assert "".join(e["text"] for e in events if e["kind"] == "TextDelta") == "phase1-final-ä", events
    assert [e["kind"] for e in events].count("ToolStarted") == 2
    assert [e["kind"] for e in events].count("ToolFinished") == 2
    assert [e["text"] for e in events if e["kind"] == "Answer"] == ["final-ä"]
    assert (output / "basic-http.db").exists()

    # Keep stdin open while the first delta arrives, then cancel the live
    # GraphExecution. The next prompt must use the same durable session.
    proc = subprocess.Popen(
        [binary, "jsonl", str(composition_file)],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, bufsize=1, env=env,
    )
    observed = queue.Queue()

    def read_events():
        for item in proc.stdout:
            observed.put(json.loads(item))
        observed.put(None)

    reader = threading.Thread(target=read_events, daemon=True)
    reader.start()

    def write_packet(kind, text=None, session="s1"):
        packet = {"type": kind, "session": session}
        if text is not None:
            packet["text"] = text
        proc.stdin.write(json.dumps(packet) + "\n")
        proc.stdin.flush()

    def until(predicate, label):
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            try:
                event = observed.get(timeout=min(2, max(0.01, deadline - time.monotonic())))
            except queue.Empty:
                continue
            assert event is not None, (
                f"JSONL host exited before {label}: {proc.poll()}; "
                f"stderr: {proc.stderr.read()}"
            )
            if predicate(event):
                return event
        raise AssertionError(f"timed out waiting for {label}")

    try:
        write_packet("prompt", "cancel during stream")
        until(lambda e: e["kind"] == "TextDelta" and e["text"] == "cancel-start",
              "live streamed text")
        # A separate host has no authority to infer that the original
        # process died. It must refuse a conflicting session prompt without
        # changing the active run or issuing another Model request.
        concurrent = subprocess.run(
            [binary, "jsonl", str(composition_file)],
            input=json.dumps({
                "type": "prompt", "session": "s1", "text": "must reject"
            }) + "\n",
            text=True, capture_output=True, env=env, timeout=12,
        )
        assert concurrent.returncode != 0, (
            concurrent.stdout, concurrent.stderr
        )
        assert "session already owned by another host" in concurrent.stderr, concurrent.stderr
        assert len(calls) == 3, "concurrent host dispatched a second Model"
        # The next turn is submitted before cancelling the current run.
        # The controller must preserve it across cancellation and draining.
        write_packet("prompt", "after cancel")
        write_packet("prompt", "second-session", session="s2")
        write_packet("cancel", session="s2")
        write_packet("cancel")
        # Give the live graph time to observe the cancellation before
        # releasing HTTP input. This also prevents a stalled curl request
        # from waiting indefinitely for another transport chunk.
        time.sleep(1.0)
        release_cancel_stream.set()
        until(lambda e: e["kind"] == "Cancelled" and e["session"] == "s1",
              "durable cancellation")
        until(lambda e: e["kind"] == "Answer" and e["text"] == "resumed"
              and e["session"] == "s1", "continuation after cancellation")
        until(lambda e: e["kind"] == "Answer" and e["text"] == "other-session"
              and e["session"] == "s2", "isolated second-session answer")
        proc.stdin.close()
        assert proc.wait(timeout=25) == 0, proc.stderr.read()
        assert len(calls) == 5, f"unexpected model calls after cancellation: {len(calls)}"
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait(timeout=10)
        proc.stdin.close() if proc.stdin and not proc.stdin.closed else None
        proc.stdout.close()
        proc.stderr.close()
        reader.join(timeout=5)

    # Simulate a process crash after a real HTTP Model delta. The file
    # lock must be released by the OS, while its run marker remains durable.
    # A new host with the same database may then fail the interrupted
    # Model-only turn and complete the subsequent prompt without replay.
    crashed = subprocess.Popen(
        [binary, "jsonl", str(composition_file)],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, bufsize=1, env=env,
    )
    crash_events = queue.Queue()

    def read_crashed():
        for item in crashed.stdout:
            crash_events.put(json.loads(item))
        crash_events.put(None)

    crashed_reader = threading.Thread(target=read_crashed, daemon=True)
    crashed_reader.start()
    try:
        crashed.stdin.write(json.dumps({
            "type": "prompt", "session": "s1", "text": "crash model"
        }) + "\n")
        crashed.stdin.flush()
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            event = crash_events.get(timeout=3)
            assert event is not None, "model host exited before crash-start"
            if event["kind"] == "TextDelta" and event["text"] == "crash-start":
                break
        else:
            raise AssertionError("missing live delta before process crash")

        marker = output / ("basic-http.db.phenix-session-" + "s1".encode().hex() + ".lock")
        assert marker.read_text().startswith("phenix.basic.owner.v1\nrun-"), (
            "live model has no durable owner marker"
        )
        crashed.kill()
        assert crashed.wait(timeout=10) != 0
        release_crash_stream.set()
    finally:
        if crashed.poll() is None:
            crashed.kill()
            crashed.wait(timeout=10)
        release_crash_stream.set()
        crashed.stdin.close()
        crashed.stdout.close()
        crashed.stderr.close()
        crashed_reader.join(timeout=5)

    # The lock alone is not proof. A tampered run marker must leave the
    # durable Running snapshot unchanged and dispatch no Model request.
    original_marker = marker.read_text()
    marker.write_text("phenix.basic.owner.v1\nwrong-run")
    denied = subprocess.run(
        [binary, "jsonl", str(composition_file)],
        input=json.dumps({
            "type": "prompt", "session": "s1", "text": "unmarked must reject"
        }) + "\n",
        text=True, capture_output=True, env=env, timeout=15,
    )
    assert denied.returncode != 0, (denied.stdout, denied.stderr)
    assert "no matching fenced owner marker" in denied.stderr, denied.stderr
    assert len(calls) == 6, "unverified recovery dispatched a new model"
    # Force a failure *between* two durable writes. The Session checkpoint
    # must persist while SQLite rejects only the recovery Failure journal
    # append. The restart must repair that missing event without Model replay.
    marker.write_text(original_marker)
    with sqlite3.connect(env["PHENIX_BASIC_SQLITE_PATH"]) as db:
        db.executescript("""
            CREATE TRIGGER block_recovery_journal
            BEFORE UPDATE OF data ON basic_records
            WHEN NEW.session = 's1' AND NEW.name = 'event-journal.v1'
            BEGIN
                SELECT RAISE(ABORT, 'test interrupted journal');
            END;
        """)
    try:
        incomplete = subprocess.run(
            [binary, "jsonl", str(composition_file)],
            input=json.dumps({
                "type": "prompt", "session": "s1", "text": "after crash"
            }) + "\n",
            text=True, capture_output=True, env=env, timeout=20,
        )
        assert incomplete.returncode != 0, (incomplete.stdout, incomplete.stderr)
        assert len(calls) == 6, "failed recovery issued the interrupted Model"
        assert marker.read_text() == original_marker.replace(
            "phenix.basic.owner.v1\n", "phenix.basic.recovery.v1\n", 1,
        ), "recovery phase marker did not survive the failed journal append"
    finally:
        with sqlite3.connect(env["PHENIX_BASIC_SQLITE_PATH"]) as db:
            db.execute("DROP TRIGGER block_recovery_journal")

    # The snapshot is already Failed, but its recovery event was not
    # committed. Block the *next* session checkpoint so that recovery can
    # append the missing Failure event without dispatching another Model.
    with sqlite3.connect(env["PHENIX_BASIC_SQLITE_PATH"]) as db:
        db.executescript("""
            CREATE TRIGGER block_followup_checkpoint
            BEFORE UPDATE OF data ON basic_records
            WHEN NEW.session = 's1' AND NEW.name = 'session.snapshot.v1'
            BEGIN
                SELECT RAISE(ABORT, 'test followup checkpoint failure');
            END;
        """)
    try:
        journal_only = subprocess.run(
            [binary, "jsonl", str(composition_file)],
            input=json.dumps({
                "type": "prompt", "session": "s1", "text": "after crash"
            }) + "\n",
            text=True, capture_output=True, env=env, timeout=20,
        )
        assert journal_only.returncode != 0, (
            journal_only.stdout, journal_only.stderr
        )
        assert len(calls) == 6, "failed followup dispatched a Model"
        assert marker.read_text() == "", (
            "recovery did not clear the marker after committing Failure"
        )
    finally:
        with sqlite3.connect(env["PHENIX_BASIC_SQLITE_PATH"]) as db:
            db.execute("DROP TRIGGER block_followup_checkpoint")

    # Replay the persisted recovery marker as if the owner exited after the
    # journal append and before clearing its marker. The next owner must
    # find the event and avoid writing a duplicate.
    marker.write_text(original_marker.replace(
        "phenix.basic.owner.v1\n", "phenix.basic.recovery.v1\n", 1,
    ))

    recovered = subprocess.run(
        [binary, "jsonl", str(composition_file)],
        input=json.dumps({
            "type": "prompt", "session": "s1", "text": "after crash"
        }) + "\n",
        text=True, capture_output=True, env=env, timeout=35,
    )
    assert recovered.returncode == 0, (recovered.stdout, recovered.stderr)
    resumed_events = [json.loads(line) for line in recovered.stdout.splitlines()]
    recovered_failures = [
        e for e in resumed_events
        if e["kind"] == "Failure" and "interrupted" in e["text"]
    ]
    journal_only_failures = [
        json.loads(line) for line in journal_only.stdout.splitlines()
        if json.loads(line)["kind"] == "Failure"
        and "interrupted" in json.loads(line)["text"]
    ]
    assert len(recovered_failures) + len(journal_only_failures) == 1, (
        journal_only.stdout, resumed_events
    )
    assert any(e["kind"] == "Answer" and e["text"] == "recovered"
               for e in resumed_events), resumed_events
    assert len(calls) == 7, "recovery retried the interrupted model call"
    assert marker.read_text() == "", "completed run left stale ownership"

    (output / "pending-basic-http.json").write_text(json.dumps({
        "format": "phenix.basic.pending-http-product.v1",
        "model_requests": len(calls),
        "tool_calls": 2,
        "streamed_text_exactly_once": True,
        "pending_basic_agent": True,
        "active_cancellation_tested": True,
        "durable_resume_after_cancel": True,
        "live_jsonl_delta": True,
        "queued_followup_survives_cancel": True,
        "queued_other_session_isolated": True,
        "wrong_session_cancel_ignored": True,
        "second_process_running_session_rejected": True,
        "session_file_lock_enforced": True,
        "crashed_model_run_recovered": True,
        "durable_owner_marker_matched": True,
        "no_interrupted_model_replay": True,
        "tampered_marker_recovery_rejected": True,
        "recovery_phase_marker_restart_tested": True,
        "interrupted_recovery_journal_write_repaired": True,
        "post_journal_marker_replay_deduplicated": True,
    }, indent=2, sort_keys=True))
finally:
    release_cancel_stream.set()
    release_crash_stream.set()
    http.shutdown()
    http.server_close()
