"""Packaged ACP launcher and controlled V2 pending/drain through the SDK adapter."""
import json
import os
from pathlib import Path
import queue
import sqlite3
import subprocess
import sys
import threading
import time

launcher, runtime, controlled, destination = sys.argv[1:]
output = Path(destination)
reports = {}

for case in ("launcher", "resume", "cancel-initial", "cancel-continuation", "disconnect"):
    database = output / f"acp-{case}.sqlite"
    control = output / f"acp-{case}.control"
    pending = control.with_suffix(".pending")
    cancelled_witness = control.with_suffix(".cancelled")
    assert not database.exists()
    assert not pending.exists() and not cancelled_witness.exists()
    cancellation_observed = False

    def set_control(value):
        temporary = control.with_suffix(".tmp")
        temporary.write_text(value + "\n")
        temporary.replace(control)

    set_control("release-initial" if case == "cancel-continuation" else "hold")
    env = dict(os.environ, PHENIX_BASIC_SQLITE_PATH=str(database.resolve()),
               PHENIX_SCRIPTED_MODEL_CONTROL_FILE=str(control.resolve()))
    command = [launcher] if case == "launcher" else [runtime, "acp", controlled]
    proc = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, text=True, bufsize=1, env=env)
    observed = queue.Queue()
    events = []

    def read_frames():
        try:
            for line in proc.stdout:
                observed.put(json.loads(line))
        finally:
            observed.put(None)

    reader = threading.Thread(target=read_frames, daemon=True)
    reader.start()

    def send(number, method, params):
        frame = dict(jsonrpc="2.0", method=method, params=params)
        if number is not None:
            frame["id"] = number
        proc.stdin.write(json.dumps(frame) + "\n")
        proc.stdin.flush()

    def reply(number):
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            frame = observed.get(timeout=max(0.01, deadline - time.monotonic()))
            assert frame is not None, (case, proc.poll(), events)
            events.append(frame)
            if frame.get("id") == number:
                assert "error" not in frame, frame
                return frame["result"]
        raise AssertionError(("reply timeout", case, events))

    def counts():
        with sqlite3.connect(database) as db:
            (data,) = db.execute(
                "SELECT data FROM basic_records WHERE session = ? AND name = 'event-journal.v1'",
                (session,),
            ).fetchone()
        return {kind: data.count(kind.encode()) for kind in (
            "PromptAccepted", "ModelStarted", "ToolStarted", "ToolFinished",
            "Answer", "Cancelled",
        )}

    try:
        send(1, "initialize", dict(protocolVersion=1, clientCapabilities={}))
        assert reply(1)["protocolVersion"] == 1
        send(2, "session/new", dict(cwd="/tmp", mcpServers=[]))
        created = reply(2)
        session = created["sessionId"]
        assert created["configOptions"][0]["currentValue"] == "phenix.model.scripted"
        send(3, "session/prompt", dict(sessionId=session,
                                     prompt=[dict(type="text", text="tool:acp-proof")]))
        if case != "launcher":
            phase = "continuation" if case == "cancel-continuation" else "initial"
            deadline = time.monotonic() + 25
            while not pending.exists() or pending.read_text() != phase:
                assert proc.poll() is None, (case, events)
                assert time.monotonic() < deadline, ("pending witness timeout", case)
                time.sleep(0.01)
            before = counts()
            tools = int(phase == "continuation")
            assert before == dict(PromptAccepted=1, ModelStarted=1 + tools,
                                  ToolStarted=tools, ToolFinished=tools,
                                  Answer=0, Cancelled=0), before
            if case == "resume":
                set_control("release")
            else:
                if case == "disconnect":
                    proc.stdin.close()
                else:
                    send(None, "session/cancel", dict(sessionId=session))
                    send(None, "session/cancel", dict(sessionId=session))
                deadline = time.monotonic() + 25
                while not cancelled_witness.exists():
                    assert proc.poll() is None, ("exited before cancellation callback", case)
                    assert time.monotonic() < deadline, ("cancellation witness timeout", case)
                    time.sleep(0.01)
                assert cancelled_witness.read_text() == phase, case
                cancellation_observed = True
                # Retain the bounded no-completion window, now after the
                # fixture has witnessed actual cancellation callback delivery.
                time.sleep(0.3)
                assert proc.poll() is None and pending.exists(), case
                assert counts() == before, (before, counts())
                with observed.mutex:
                    assert not any(frame.get("id") == 3 for frame in observed.queue)
                set_control("drain")
        if case != "disconnect":
            result = reply(3)
            cancelled = case.startswith("cancel-")
            assert result["stopReason"] == ("cancelled" if cancelled else "end_turn"), result
            assert counts()["Cancelled"] == int(cancelled), counts()
            assert counts()["Answer"] == int(not cancelled), counts()
            if not cancelled:
                updates = [f["params"]["update"] for f in events if f.get("method") == "session/update"]
                assert [u["toolCallId"] for u in updates if u["sessionUpdate"] == "tool_call"] == ["call-1"]
                assert [u["toolCallId"] for u in updates if u["sessionUpdate"] == "tool_call_update"] == ["call-1"]
                assert "".join(u.get("content", {}).get("text", "") for u in updates
                               if u["sessionUpdate"] == "agent_message_chunk") == "mock:call-1:ACP-PROOF"
            set_control("release")
            send(4, "session/prompt", dict(sessionId=session,
                                         prompt=[dict(type="text", text="after-drain")]))
            assert reply(4)["stopReason"] == "end_turn"
            assert counts()["ToolStarted"] == counts()["ToolFinished"] == (tools if cancelled else 1)
            proc.stdin.close()
        assert proc.wait(timeout=25) == 0, proc.stderr.read()
        assert not pending.exists(), case
        assert not cancelled_witness.exists(), case
        if case == "disconnect":
            assert counts() == {**before, "Cancelled": 1}, counts()
        reports[case] = dict(journal_counts=counts(), frames=events,
                             cancellation_callback_observed=cancellation_observed)
        assert "forbidden late answer" not in json.dumps(events)
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait(timeout=10)
        if not proc.stdin.closed:
            proc.stdin.close()
        reader.join(timeout=5)
        proc.stdout.close()
        proc.stderr.close()
        control.unlink(missing_ok=True)
        pending.unlink(missing_ok=True)
        cancelled_witness.unlink(missing_ok=True)
        control.with_suffix(".cancelled.tmp").unlink(missing_ok=True)

(output / "packaged-acp.json").write_text(json.dumps(reports, indent=2) + "\n")
