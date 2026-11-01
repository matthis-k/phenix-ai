"""Actual selected-provider ACP cancellation, using server and observer handshakes.

Numeric loopback exercises transport after resolution, not stalled DNS/NSS.
Peer EOF witnesses socket closure, not internal kernel lease accounting.
"""
import json
import queue
import sqlite3
import threading

from model_protocol_fixture import ResponsesWire, chat_view


def exercise(proc, call, database, output, install):
    reports = {}

    def send(identifier, method, **params):
        packet = dict(jsonrpc="2.0", method=method, params=params)
        if identifier is not None:
            packet["id"] = identifier
        proc.stdin.write(json.dumps(packet) + "\n")
        proc.stdin.flush()

    def counts(session):
        with sqlite3.connect(database) as db:
            (journal,) = db.execute(
                "SELECT data FROM basic_records WHERE session = ? "
                "AND name = 'event-journal.v1'", (session,),
            ).fetchone()
        return {kind: journal.count(kind.encode()) for kind in (
            "PromptAccepted", "ModelStarted", "ToolStarted", "ToolFinished",
            "Answer", "Failure", "Cancelled",
        )}

    try:
        for index, case in enumerate(("before-headers", "first-text", "tool-assembly",
                                      "continuation", "completion-winning",
                                      "cancellation-winning")):
            admitted = threading.Event()
            drained = threading.Event()
            finished = threading.Event()
            release = threading.Event()
            errors = queue.Queue()
            requests = []
            marker = "observed-" + case
            late = "forbidden-late-" + case

            def handle(http, packet):
                held = False
                try:
                    messages = chat_view(packet)["messages"]
                    requests.append(messages)
                    text = [m["content"] for m in messages if m["role"] == "user"][-1]
                    parts = []
                    wire = ResponsesWire(parts.append)
                    if text == "subsequent-" + case:
                        wire.chunk({"choices": [{"delta": {"content": "recovered-" + case}}]})
                        wire.chunk({"choices": [], "usage": {"prompt_tokens": 5, "completion_tokens": 4}})
                        wire.done()
                        payload = b"".join(parts)
                        http.send_response(200)
                        http.send_header("Content-Length", str(len(payload)))
                        http.send_header("Content-Type", "text/event-stream")
                        http.end_headers()
                        http.wfile.write(payload)
                        http.wfile.flush()
                        return
                    assert text == case, messages
                    if case == "continuation" and len(requests) == 1:
                        wire.chunk({"choices": [{"delta": {"tool_calls": [{
                            "index": 0, "id": "cancel-tool", "function": {
                                "name": "uppercase", "arguments": '{"value":"drain"}',
                            },
                        }]}}]})
                        wire.chunk({"choices": [], "usage": {"prompt_tokens": 5, "completion_tokens": 4}})
                        wire.done()
                        payload = b"".join(parts)
                        http.send_response(200)
                        http.send_header("Content-Type", "text/event-stream")
                        http.send_header("Content-Length", str(len(payload)))
                        http.end_headers()
                        http.wfile.write(payload)
                        http.wfile.flush()
                        return
                    if case == "continuation":
                        assert [m for m in messages if m["role"] == "tool"] == [{
                            "role": "tool", "tool_call_id": "cancel-tool", "content": "DRAIN",
                        }], messages
                    if case == "tool-assembly":
                        wire.chunk({"choices": [{"delta": {"tool_calls": [{
                            "index": 0, "id": "incomplete-tool", "function": {
                                "name": "uppercase", "arguments": '{"value":"',
                            },
                        }]}}]})
                    if case != "before-headers":
                        wire.chunk({"choices": [{"delta": {"content": marker}}]})
                        prefix = b"".join(parts)
                        parts.clear()
                        tail = b""
                        if case in ("completion-winning", "cancellation-winning"):
                            wire.chunk({"choices": [], "usage": {"prompt_tokens": 5, "completion_tokens": 4}})
                            wire.done()
                            tail = b"".join(parts)
                            parts.clear()
                        http.send_response(200)
                        http.send_header("Content-Type", "text/event-stream")
                        http.send_header("Content-Length", str(len(prefix) + len(tail))
                                         if case in ("completion-winning", "cancellation-winning") else "1000000")
                        http.end_headers()
                        http.wfile.write(prefix)
                        http.wfile.flush()
                        parts.clear()
                    held = True
                    admitted.set()
                    if case in ("completion-winning", "cancellation-winning"):
                        assert release.wait(25), "race release missing"
                        try:
                            http.wfile.write(tail)
                            http.wfile.flush()
                        except (BrokenPipeError, ConnectionResetError):
                            pass
                    http.connection.settimeout(25)
                    try:
                        assert http.connection.recv(1) == b"", "unexpected extra request bytes"
                    except ConnectionResetError:
                        pass
                    drained.set()
                    # Attempt a late success only after the peer has closed.
                    try:
                        http.wfile.write(("data: " + late + "\n\n").encode())
                        http.wfile.flush()
                    except (BrokenPipeError, ConnectionResetError):
                        pass
                    http.close_connection = True
                except Exception as error:
                    errors.put(repr(error))
                    admitted.set()
                    drained.set()
                finally:
                    if held:
                        finished.set()

            install(handle)
            created = call(proc, 200, "session/new", {"cwd": "/tmp", "mcpServers": []})
            session = created["result"]["sessionId"]
            choices = created["result"]["configOptions"][0]["options"]
            selected = [o["value"] for o in choices if o["value"].startswith("phenix.native-model@1:")]
            assert len(selected) == 1, created
            assert "result" in call(proc, 201, "session/set_config_option", {
                "sessionId": session, "configId": "model", "value": selected[0],
            })
            events = []
            responses = []

            def receive():
                raw = proc.responses.get(timeout=40)
                assert raw is not None, (case, proc.poll())
                event = json.loads(raw)
                if event.get("method") == "session/update":
                    assert event["params"]["sessionId"] == session, event
                    events.append(event["params"]["update"])
                else:
                    responses.append(event)
                return event

            send(202, "session/prompt", sessionId=session,
                 prompt=[dict(type="text", text=case)])
            assert admitted.wait(25), (case, "request never admitted")
            assert errors.empty(), list(errors.queue)
            if case != "before-headers":
                while not any(e.get("sessionUpdate") == "agent_message_chunk" and
                              e.get("content", {}).get("text") == marker for e in events):
                    receive()
            before = counts(session)
            tools = int(case == "continuation")
            assert before == dict(PromptAccepted=1, ModelStarted=1 + tools,
                                  ToolStarted=tools, ToolFinished=tools,
                                  Answer=0, Cancelled=0, Failure=0), before
            if case == "completion-winning":
                release.set()
                while not responses:
                    receive()
            send(None, "session/cancel", sessionId=session)
            send(None, "session/cancel", sessionId=session)
            while not responses:
                receive()
            # Cancellation-winning cannot receive the withheld completion bytes
            # until cancellation has already produced its terminal response.
            release.set()
            assert len(responses) == 1 and responses[0].get("id") == 202, responses
            reason = responses[0].get("result", {}).get("stopReason")
            assert reason == ("end_turn" if case == "completion-winning"
                              else "cancelled"), responses
            assert drained.wait(25), (case, "peer did not close transport")
            assert finished.wait(25), (case, "controlled handler did not finish")
            assert errors.empty(), list(errors.queue)
            cancelled_events = list(events)
            terminal = counts(session)
            assert terminal == {**before, "Cancelled": int(reason == "cancelled"),
                                "Answer": int(reason == "end_turn")}, (before, terminal)
            assert len(requests) == (2 if case == "continuation" else 1), requests
            send(203, "session/prompt", sessionId=session,
                 prompt=[dict(type="text", text="subsequent-" + case)])
            while len(responses) < 2:
                receive()
            assert responses[-1].get("id") == 203, responses
            assert responses[-1].get("result", {}).get("stopReason") == "end_turn", responses
            # An ACP round-trip is the output barrier, not an arbitrary quiet sleep.
            send(204, "session/set_config_option", sessionId=session,
                 configId="model", value=selected[0])
            while len(responses) < 3:
                receive()
            assert responses[-1].get("id") == 204 and "result" in responses[-1], responses
            after = counts(session)
            assert after == {**terminal, "PromptAccepted": 2,
                             "ModelStarted": terminal["ModelStarted"] + 1,
                             "Answer": terminal["Answer"] + 1}, after
            assert len(requests) == (3 if case == "continuation" else 2), requests
            assert late not in json.dumps(events), events
            assert [e["content"]["text"] for e in events[len(cancelled_events):]
                    if e.get("sessionUpdate") == "agent_message_chunk"] == ["recovered-" + case], events
            assert not any(e.get("sessionUpdate") in ("tool_call", "tool_call_update")
                           for e in events[len(cancelled_events):]), events
            reports[case] = dict(before=before, cancelled=terminal, after=after,
                                 peer_closed=True, requests=len(requests),
                                 responses=responses, updates=events)
    finally:
        install(None)
    (output / "selected-provider-cancellation.json").write_text(json.dumps({
        "cases": reports,
        "scope": "numeric loopback transport; no NSS or internal lease observation",
    }, indent=2) + "\n")
