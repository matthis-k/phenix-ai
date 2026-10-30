"""Real packaged V2 host, selected distinct native tool workers and durable joins."""
import json
import io
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
    """Decode the existing canonical Value wire for durable-state assertions only."""
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


def assert_partial_recovery(snapshot, pending_call):
    assert snapshot["status"] == "Indeterminate", snapshot
    assert snapshot["pending"] == [pending_call], snapshot["pending"]
    assert [call["id"] for call in snapshot["pending"]] == ["call-1"]
    known = [message["Tool"] for message in snapshot["history"] if "Tool" in message]
    assert known == [{"call_id": "call-2", "outcome": {
        "Succeeded": {"text": "pending-b:proof"},
    }}], known
    return known


for invalid in ("invalid-tool-json", "invalid-tool-type"):
    database = output / f"{invalid}.sqlite"
    result = subprocess.run([runtime, "terminal", composition], input=invalid + "\n", text=True,
                            capture_output=True, timeout=25,
                            env=dict(os.environ, PHENIX_BASIC_SQLITE_PATH=str(database.resolve())))
    assert result.returncode != 0 and "InvalidInput" in result.stderr, (result.stdout, result.stderr)
    with sqlite3.connect(database) as db:
        saved = dict(db.execute("SELECT name, data FROM basic_records WHERE session = 's1'"))
    snapshot = portable(saved["session.snapshot.v1"])
    assert not snapshot["pending"]
    assert not any("Assistant" in message for message in snapshot["history"])
    assert not any(name.startswith("graph-tool-") for name in saved), saved.keys()
    assert b"ToolStarted" not in saved["event-journal.v1"]
    reports[invalid] = dict(stderr=result.stderr, pending=snapshot["pending"])

for case in ("reverse", "mixed", "cancel", "partial-cancel", "partial-infrastructure",
             "partial-receipt-write-failure"):
    database = output / f"tools-{case}.sqlite"
    controls = [output / f"tools-{case}-{name}.control" for name in ("a", "b")]
    assert not database.exists()

    def control(index, value):
        temporary = controls[index].with_suffix(".tmp")
        temporary.write_text(value + "\n")
        temporary.replace(controls[index])

    control(0, "hold")
    control(1, "hold")
    env = dict(os.environ, PHENIX_BASIC_SQLITE_PATH=str(database.resolve()),
               PHENIX_TOOL_A_CONTROL_FILE=str(controls[0].resolve()),
               PHENIX_TOOL_B_CONTROL_FILE=str(controls[1].resolve()))
    proc = subprocess.Popen([runtime, "jsonl", composition], stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            text=True, bufsize=1, env=env)
    observed = queue.Queue()
    events = []

    def read_events():
        try:
            for line in proc.stdout:
                observed.put(json.loads(line))
        finally:
            observed.put(None)

    reader = threading.Thread(target=read_events, daemon=True)
    reader.start()

    def packet(kind, text=None):
        frame = dict(type=kind, session="s1")
        if text is not None:
            frame["text"] = text
        proc.stdin.write(json.dumps(frame) + "\n")
        proc.stdin.flush()

    def wait(predicate):
        deadline = time.monotonic() + 25
        while not predicate():
            assert proc.poll() is None, (case, proc.returncode, events)
            assert time.monotonic() < deadline, ("tool witness timeout", case, events)
            time.sleep(0.01)

    def until(kind):
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            try:
                event = observed.get(timeout=0.2)
            except queue.Empty:
                continue
            assert event is not None, (case, proc.poll(), events)
            events.append(event)
            if event["kind"] == kind:
                return
        raise AssertionError(("event timeout", kind, events))

    def records():
        with sqlite3.connect(database) as db:
            return dict(db.execute("SELECT name, data FROM basic_records WHERE session = 's1'"))

    def counts():
        journal = records()["event-journal.v1"]
        return {kind: journal.count(kind.encode()) for kind in (
            "ModelStarted", "ToolStarted", "ToolFinished", "Answer", "Cancelled", "Failure",
        )}

    try:
        packet("prompt", "fanout-fail:proof" if case == "mixed" else "fanout:proof")
        # These files are written only after independent physical worker handshakes.
        wait(lambda: all(path.with_suffix(".pending").exists() for path in controls))
        invocations = [json.loads(path.with_suffix(".pending").read_text()) for path in controls]
        assert [item["call"]["id"] for item in invocations] == ["call-1", "call-2"]
        assert invocations[0]["run"] == invocations[1]["run"]
        assert invocations[0]["batch"] == invocations[1]["batch"] == 1
        before = counts()
        assert before == dict(ModelStarted=1, ToolStarted=2, ToolFinished=0,
                              Answer=0, Cancelled=0, Failure=0), before
        saved = records()["session.snapshot.v1"]
        assert b"call-1" in saved and b"call-2" in saved and b"Running" in saved
        partial = case.startswith("partial-")
        known_receipts = {}
        if partial:
            control(1, "release")
            wait(lambda: controls[1].with_suffix(".receipted").exists())
            known_receipts = {name: data for name, data in records().items() if name.startswith("graph-tool-receipt.v1.")}
            assert len(known_receipts) == 1
            known = portable(next(iter(known_receipts.values())))["Ok"]
            assert known["invocation"] == invocations[1] and known["text"] == "pending-b:proof"
            assert counts() == before, "receipt persistence must not advance Model or publish the join"
        if case in ("partial-infrastructure", "partial-receipt-write-failure"):
            receipt_write_failure = case == "partial-receipt-write-failure"
            if receipt_write_failure:
                with sqlite3.connect(database) as db:
                    manifest_before = db.execute(
                        "SELECT revision, data FROM basic_records "
                        "WHERE session = 's1' AND name = 'graph-tool-batch.v1'"
                    ).fetchone()
                    assert manifest_before is not None
                    db.execute("""CREATE TRIGGER reject_tool_receipt BEFORE INSERT ON basic_records
                        WHEN NEW.session = 's1' AND NEW.name GLOB 'graph-tool-receipt.v1.*'
                        BEGIN SELECT RAISE(ABORT, 'injected receipt persistence failure'); END""")
            # The injected case runs the normal completion/record path, not fail control.
            control(0, "release" if receipt_write_failure else "fail")
            proc.stdin.close()
            assert proc.wait(timeout=25) != 0
            failure = proc.stderr.read()
            assert "PluginFailed" in failure, failure
            assert counts() == before, "failed receipt must not publish collection or advance Model"
            assert portable(records()["session.snapshot.v1"])["pending"] == [
                invocation["call"] for invocation in invocations
            ]
            if receipt_write_failure:
                assert "tool receipt failed" in failure, failure
                assert controls[0].with_suffix(".completed").exists(), "A never completed its worker"
                assert not controls[0].with_suffix(".pending").exists(), "A worker did not drain"
                assert not controls[0].with_suffix(".failed").exists(), "used pre-record fail control"
                assert not controls[0].with_suffix(".receipted").exists(), "returned terminal outcome without receipt"
                with sqlite3.connect(database) as db:
                    manifest_after = db.execute(
                        "SELECT revision, data FROM basic_records "
                        "WHERE session = 's1' AND name = 'graph-tool-batch.v1'"
                    ).fetchone()
                assert manifest_after == manifest_before, "receipt failure did not roll back the atomic fence edit"
                assert known_receipts == {name: data for name, data in records().items()
                                          if name.startswith("graph-tool-receipt.v1.")}
            restarted = subprocess.run([runtime, "terminal", composition], input="must-not-retry\n", text=True,
                                       capture_output=True, timeout=25, env=env)
            assert restarted.returncode != 0 and "Indeterminate" in restarted.stderr, restarted.stderr
            recovered = portable(records()["session.snapshot.v1"])
            known = assert_partial_recovery(recovered, invocations[0]["call"])
            assert counts() == {**before, "Failure": 1}
            assert known_receipts == {name: data for name, data in records().items() if name.startswith("graph-tool-receipt.v1.")}
            assert all(not path.with_suffix(".pending").exists() for path in controls)
            if receipt_write_failure:
                assert not controls[0].with_suffix(".receipted").exists(), "recovery retried or fabricated A"
            reports[case] = dict(invocations=invocations, pending=recovered["pending"], known=known,
                                 counts=counts(), failure=failure, receipt_write_failure=receipt_write_failure)
            continue
        if case in ("cancel", "partial-cancel"):
            packet("cancel")
            packet("cancel")
            proc.stdin.close()
            wait(lambda: all(path.with_suffix(".cancelled").exists() for path in (controls[:1] if partial else controls)))
            assert counts() == before
            if not partial:
                control(1, "drain")
            wait(lambda: (partial or controls[1].with_suffix(".drained").exists())
                 and not controls[1].with_suffix(".pending").exists())
            time.sleep(0.1)
            assert proc.poll() is None, "host exited before A physically drained"
            assert controls[0].with_suffix(".pending").exists()
            assert counts() == before
            control(0, "drain")
            until("Cancelled")
            assert counts() == {**before, "Cancelled": 1}, counts()
            saved = records()["session.snapshot.v1"]
            assert b"Indeterminate" in saved and b"call-1" in saved and b"call-2" in saved
            assert b"forbidden late success" not in saved
            recovered = portable(saved)
            assert [call["id"] for call in recovered["pending"]] == (["call-1"] if partial else ["call-1", "call-2"])
            if partial:
                assert_partial_recovery(recovered, invocations[0]["call"])
                assert known_receipts == {name: data for name, data in records().items() if name.startswith("graph-tool-receipt.v1.")}
        else:
            control(1, "release")
            wait(lambda: controls[1].with_suffix(".completed").exists()
                 and not controls[1].with_suffix(".pending").exists())
            assert controls[0].with_suffix(".pending").exists()
            assert counts() == before, "partial tool outcomes escaped the required join"
            control(0, "release")
            until("Answer")
            answers = [event["text"] for event in events if event["kind"] == "Answer"]
            expected = ("mock:call-1:pending-a:proof|call-2:error:controlled domain failure"
                        if case == "mixed" else "mock:call-1:pending-a:proof|call-2:pending-b:proof")
            assert answers == [expected], events
            assert [event["call_id"] for event in events if event["kind"] == "ToolFinished"] == ["call-1", "call-2"]
            assert counts() == {**before, "ModelStarted": 2, "ToolFinished": 2, "Answer": 1}, counts()
            packet("prompt", "after-tool-join")
            until("Answer")
            assert events[-1]["text"] == "mock:after-tool-join", events
            proc.stdin.close()
        assert proc.wait(timeout=25) == 0, proc.stderr.read()
        reader.join(timeout=5)
        assert not reader.is_alive()
        assert "forbidden late success" not in json.dumps(events)
        assert all(not path.with_suffix(".pending").exists() for path in controls)
        if case in ("cancel", "partial-cancel"):
            # A fresh process must not automatically retry unresolved physical effects.
            restarted = subprocess.run([runtime, "terminal", composition], input="must-not-retry\n",
                                       text=True, capture_output=True, timeout=25, env=env)
            assert restarted.returncode != 0, restarted.stdout
            assert "Indeterminate" in restarted.stderr or "unreconciled" in restarted.stderr, restarted.stderr
            assert counts() == {**before, "Cancelled": 1}
            assert all(not path.with_suffix(".pending").exists() for path in controls)
            if partial:
                assert_partial_recovery(portable(records()["session.snapshot.v1"]), invocations[0]["call"])
                assert known_receipts == {name: data for name, data in records().items()
                                          if name.startswith("graph-tool-receipt.v1.")}
        final = portable(records()["session.snapshot.v1"])
        reports[case] = dict(events=events, invocations=invocations, counts=counts(),
                             pending=final["pending"], known_tools=[message["Tool"] for message in final["history"] if "Tool" in message])
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait(timeout=10)
        if not proc.stdin.closed:
            proc.stdin.close()
        reader.join(timeout=5)
        proc.stdout.close()
        proc.stderr.close()

(output / "packaged-tools.json").write_text(json.dumps(reports, indent=2) + "\n")
