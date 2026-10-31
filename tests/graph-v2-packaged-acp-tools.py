"""ACP acceptance over the existing independent physical tool/receipt fixtures."""
import io
import json
import os
from pathlib import Path
import queue
import sqlite3
import subprocess
import sys
import threading
import time

runtime, composition, destination = sys.argv[1:]
output = Path(destination)
reports = {}


def portable(data):
    source = io.BytesIO(data)
    assert source.read(4) == b"PV\x02\x00"

    def take(size):
        result = source.read(size)
        assert len(result) == size
        return result

    def length():
        return int.from_bytes(take(4), "big")

    def value():
        tag = take(1)[0]
        if tag == 0:
            return None
        if tag in (1, 2):
            return tag == 2
        if tag in (3, 4):
            return int.from_bytes(take(8), "big", signed=tag == 3)
        if tag in (5, 6):
            payload = take(length())
            return payload.decode() if tag == 5 else payload
        if tag == 7:
            return [value() for _ in range(length())]
        assert tag == 8, tag
        return {take(length()).decode(): value() for _ in range(length())}

    result = value()
    assert not source.read()
    return result


for case in ("cancel-first", "partial-cancel", "completion-first", "disconnect", "crash-partial"):
    database = output / f"acp-tools-{case}.sqlite"
    controls = [output / f"acp-tools-{case}-{name}.control" for name in ("a", "b")]
    assert not database.exists()

    def control(index, text):
        temporary = controls[index].with_suffix(".tmp")
        temporary.write_text(text + "\n")
        temporary.replace(controls[index])

    def witness(index, suffix):
        return controls[index].with_suffix(suffix).exists()

    control(0, "hold")
    control(1, "hold")
    env = dict(os.environ, PHENIX_BASIC_SQLITE_PATH=str(database.resolve()),
               PHENIX_TOOL_A_CONTROL_FILE=str(controls[0].resolve()),
               PHENIX_TOOL_B_CONTROL_FILE=str(controls[1].resolve()))

    def start():
        process = subprocess.Popen([runtime, "acp", composition], stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   text=True, bufsize=1, env=env)
        frames = queue.Queue()

        def read():
            try:
                for line in process.stdout:
                    frames.put(json.loads(line))
            finally:
                frames.put(None)

        thread = threading.Thread(target=read, daemon=True)
        thread.start()
        return process, frames, thread

    proc, observed, reader = start()
    events = []
    expected_ids = []

    def send(number, method, **params):
        frame = dict(jsonrpc="2.0", method=method, params=params)
        if number is not None:
            frame["id"] = number
            expected_ids.append(number)
        proc.stdin.write(json.dumps(frame) + "\n")
        proc.stdin.flush()

    def reply(number, error=False):
        deadline = time.monotonic() + 25
        while True:
            frame = observed.get(timeout=max(0.01, deadline - time.monotonic()))
            assert frame is not None, (case, proc.poll(), events)
            events.append(frame)
            if frame.get("id") == number:
                assert ("error" in frame) == error, frame
                return frame["error" if error else "result"]
            assert time.monotonic() < deadline, (case, events)

    def wait(predicate):
        deadline = time.monotonic() + 25
        while not predicate():
            assert proc.poll() is None, (case, events)
            assert time.monotonic() < deadline, ("witness timeout", case, events)
            time.sleep(0.01)

    def records(owner):
        with sqlite3.connect(database) as db:
            return dict(db.execute("SELECT name, data FROM basic_records WHERE session = ?", (owner,)))

    def counts(owner):
        journal = records(owner).get("event-journal.v1", b"")
        return {kind: journal.count(kind.encode()) for kind in (
            "PromptAccepted", "ModelStarted", "ToolStarted", "ToolFinished", "Answer", "Cancelled", "Failure",
        )}

    def prompt(number, owner, text):
        send(number, "session/prompt", sessionId=owner, prompt=[dict(type="text", text=text)])

    def finish(code=0):
        assert proc.wait(timeout=25) == code, proc.stderr.read()
        reader.join(timeout=5)
        assert not reader.is_alive()
        while True:
            frame = observed.get(timeout=5)
            if frame is None:
                break
            events.append(frame)
        assert sorted(frame["id"] for frame in events if "id" in frame) == sorted(expected_ids), events
        assert "forbidden late success" not in json.dumps(events), events
        assert all("forbidden late success" not in repr(records(owner)) for owner in (session, other))

    try:
        send(1, "initialize", protocolVersion=1, clientCapabilities={})
        assert reply(1)["protocolVersion"] == 1
        send(2, "session/new", cwd="/tmp", mcpServers=[])
        session = reply(2)["sessionId"]
        send(3, "session/new", cwd="/tmp", mcpServers=[])
        other = reply(3)["sessionId"]
        assert session != other
        prompt(4, session, "fanout:proof")
        wait(lambda: all(witness(index, ".pending") for index in (0, 1)))
        invocations = [json.loads(path.with_suffix(".pending").read_text()) for path in controls]
        assert [item["call"]["id"] for item in invocations] == ["call-1", "call-2"]
        assert all(item["session"] == session for item in invocations)
        assert invocations[0]["run"] == invocations[1]["run"]
        before = counts(session)
        assert before == dict(PromptAccepted=1, ModelStarted=1, ToolStarted=2,
                              ToolFinished=0, Answer=0, Cancelled=0, Failure=0), before
        known_receipts = {}
        partial = case in ("partial-cancel", "crash-partial")
        if partial:
            control(1, "release")
            wait(lambda: witness(1, ".receipted"))
            known_receipts = {name: data for name, data in records(session).items()
                              if name.startswith("graph-tool-receipt.v1.")}
            assert len(known_receipts) == 1
            known = portable(next(iter(known_receipts.values())))["Ok"]
            assert known["invocation"] == invocations[1] and known["text"] == "pending-b:proof"
            assert counts(session) == before
        if case == "crash-partial":
            proc.kill()
            # The killed prompt has no terminal response, unlike a drained cancellation.
            expected_ids.remove(4)
            finish(-9)
        else:
            other_before = records(other)
            if case != "disconnect":
                prompt(5, other, "isolated-queued")
            if case == "completion-first":
                control(1, "release")
                wait(lambda: witness(1, ".receipted"))
                assert counts(session) == before
                control(0, "release")
                assert reply(4)["stopReason"] == "end_turn"
                # Completion wins before cancellation; it cannot cancel the queued other session.
                send(None, "session/cancel", sessionId=session)
            else:
                if case == "disconnect":
                    proc.stdin.close()
                    expected_ids.remove(4)
                else:
                    send(None, "session/cancel", sessionId=session)
                    send(None, "session/cancel", sessionId=session)
                wait(lambda: all(witness(index, ".cancelled") for index in ((0,) if partial else (0, 1))))
                time.sleep(0.3)
                assert counts(session) == before
                assert records(other) == other_before, "queued session ran before physical drain"
                with observed.mutex:
                    assert not any(frame is not None and frame.get("id") in (4, 5) for frame in observed.queue)
                if not partial:
                    control(1, "drain")
                    wait(lambda: witness(1, ".drained") and not witness(1, ".pending"))
                time.sleep(0.1)
                assert proc.poll() is None and witness(0, ".pending")
                assert counts(session) == before
                assert records(other) == other_before
                control(0, "drain")
                if case != "disconnect":
                    assert reply(4)["stopReason"] == "cancelled"
                wait(lambda: counts(session)["Cancelled"] == 1)
                assert counts(session) == {**before, "Cancelled": 1}
            if case != "disconnect":
                assert reply(5)["stopReason"] == "end_turn"
                assert counts(other) == dict(PromptAccepted=1, ModelStarted=1, ToolStarted=0,
                                             ToolFinished=0, Answer=1, Cancelled=0, Failure=0)
                if case != "completion-first":
                    prompt(6, session, "must-not-retry")
                    rejected = reply(6, error=True)
                    assert "unreconciled tool calls" in json.dumps(rejected), rejected
                prompt(7, other, "after-physical-drain")
                assert reply(7)["stopReason"] == "end_turn"
                proc.stdin.close()
            finish()
            assert all(not witness(index, ".pending") for index in (0, 1))
        first_events = events
        if case != "completion-first":
            # Restart via ACP, not a direct recovery API or a different frontend.
            proc.stdin.close() if not proc.stdin.closed else None
            proc.stdout.close()
            proc.stderr.close()
            proc, observed, reader = start()
            events = []
            expected_ids = []
            send(11, "initialize", protocolVersion=1, clientCapabilities={})
            reply(11)
            send(12, "session/load", sessionId=session, cwd="/tmp", mcpServers=[])
            reply(12)
            # Loading replays history; the next admitted prompt holds the recovery lock.
            if case == "crash-partial":
                prompt(14, session, "recover-without-retry")
                rejected = reply(14, error=True)
                assert "unreconciled tool calls" in json.dumps(rejected), rejected
            recovered = portable(records(session)["session.snapshot.v1"])
            assert recovered["status"] == "Indeterminate", recovered
            assert recovered["pending"] == [item["call"] for item in (invocations[:1] if partial else invocations)]
            known = [message["Tool"] for message in recovered["history"] if "Tool" in message]
            assert known == ([dict(call_id="call-2", outcome={"Succeeded": {"text": "pending-b:proof"}})] if partial else [])
            assert known_receipts == {name: data for name, data in records(session).items()
                                      if name.startswith("graph-tool-receipt.v1.")}
            saved = records(session)
            prompt(13, session, "must-not-retry-after-restart")
            rejected = reply(13, error=True)
            assert "unreconciled tool calls" in json.dumps(rejected), rejected
            assert records(session) == saved, "rejected retry mutated durable recovery state"
            assert not witness(0, ".completed") and not witness(0, ".receipted")
            proc.stdin.close()
            finish()
        all_events = first_events + (events if first_events is not events else [])
        assert sum(frame.get("id") == 4 for frame in all_events) == int(case not in ("disconnect", "crash-partial"))
        updates = [frame["params"] for frame in first_events if frame.get("method") == "session/update"]
        assert all(item["sessionId"] in (session, other) for item in updates)
        other_text = "".join(item["update"].get("content", {}).get("text", "") for item in updates
                             if item["sessionId"] == other and item["update"]["sessionUpdate"] == "agent_message_chunk")
        assert other_text == ("" if case in ("disconnect", "crash-partial") else "mock:isolated-queuedmock:after-physical-drain"), other_text
        reports[case] = dict(frames=all_events, invocations=invocations,
                             journal_counts=counts(session), other_counts=counts(other),
                             partial_receipts=len(known_receipts))
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait(timeout=10)
        if not proc.stdin.closed:
            proc.stdin.close()
        reader.join(timeout=5)
        proc.stdout.close()
        proc.stderr.close()

(output / "packaged-acp-tools.json").write_text(json.dumps(reports, indent=2) + "\n")
