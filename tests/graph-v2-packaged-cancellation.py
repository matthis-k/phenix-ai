"""Controlled physical drain through the packaged V2 JSONL host, not a test runner."""
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

for case in ("initial", "continuation", "initial-eof-after-cancel",
             "continuation-eof-after-cancel"):
    mode = case.split("-", 1)[0]
    eof_after_cancel = case.endswith("-eof-after-cancel")
    database = output / f"cancel-{case}.sqlite"
    control = output / f"cancel-{case}.control"
    pending = control.with_suffix(".pending")
    assert not database.exists()

    def set_control(value):
        temporary = control.with_suffix(".tmp")
        temporary.write_text(value + "\n")
        temporary.replace(control)

    set_control("hold" if mode == "initial" else "release-initial")
    env = dict(os.environ, PHENIX_BASIC_SQLITE_PATH=str(database.resolve()),
               PHENIX_SCRIPTED_MODEL_CONTROL_FILE=str(control.resolve()))
    proc = subprocess.Popen(
        [runtime, "jsonl", composition], stdin=subprocess.PIPE,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1, env=env,
    )
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
        value = dict(type=kind, session="s1")
        if text is not None:
            value["text"] = text
        proc.stdin.write(json.dumps(value) + "\n")
        proc.stdin.flush()

    def until(predicate):
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            try:
                event = observed.get(timeout=0.2)
            except queue.Empty:
                continue
            assert event is not None, (proc.poll(), events)
            events.append(event)
            if predicate(event):
                return
        raise AssertionError(("timed out", case, events))

    def counts():
        with sqlite3.connect(database) as db:
            (data,) = db.execute(
                "SELECT data FROM basic_records WHERE session = 's1' "
                "AND name = 'event-journal.v1'"
            ).fetchone()
        return {kind: data.count(kind.encode()) for kind in (
            "PromptAccepted", "ModelStarted", "ToolStarted", "ToolFinished",
            "Answer", "Failure", "Cancelled",
        )}

    try:
        packet("prompt", "tool:cancel-proof")
        models = 1 if mode == "initial" else 2
        until(lambda e: sum(item["kind"] == "ModelStarted" for item in events) == models)
        # ModelStarted may precede the deferred Model's physical admission.
        # Wait for the fixture's worker-startup handshake, not journal intent.
        deadline = time.monotonic() + 25
        while True:
            try:
                phase = pending.read_text()
            except FileNotFoundError:
                phase = None
            if phase == mode:
                break
            assert proc.poll() is None, (case, events)
            assert time.monotonic() < deadline, ("pending witness timeout", case, phase)
            time.sleep(0.01)
        before = counts()
        assert before == dict(PromptAccepted=1, ModelStarted=models,
                              ToolStarted=models - 1, ToolFinished=models - 1,
                              Answer=0, Failure=0, Cancelled=0), before
        # Repeated cancellation must not finish before physical work drains.
        packet("cancel")
        packet("cancel")
        if eof_after_cancel:
            # Shutdown request means JSONL stdin EOF after normal cancellation,
            # not a direct kernel.shutdown() call while work is retained.
            proc.stdin.close()
        time.sleep(0.3)
        assert proc.poll() is None, events
        assert pending.read_text() == mode
        assert counts() == before, (before, counts())
        while True:
            try:
                event = observed.get_nowait()
            except queue.Empty:
                break
            assert event is not None, events
            events.append(event)
        assert not any(e["kind"] in ("Cancelled", "Answer", "Failure") for e in events), events

        # The fixture deliberately returns success on cancelled drain. The host
        # must discard it and append exactly one durable Cancelled event.
        set_control("drain")
        until(lambda e: e["kind"] == "Cancelled")
        assert not pending.exists(), "worker witness survived physical drain"
        assert counts() == {**before, "Cancelled": 1}, counts()
        if not eof_after_cancel:
            set_control("release")
            packet("prompt", "after-drain")
            until(lambda e: e["kind"] == "Answer")
            proc.stdin.close()
        assert proc.wait(timeout=25) == 0, proc.stderr.read()
        reader.join(timeout=5)
        assert not reader.is_alive()
        while not observed.empty():
            event = observed.get_nowait()
            if event is not None:
                events.append(event)
        expected_answers = [] if eof_after_cancel else ["mock:after-drain"]
        assert [e["text"] for e in events if e["kind"] == "Answer"] == expected_answers, events
        assert sum(e["kind"] == "Cancelled" for e in events) == 1, events
        assert "forbidden late answer" not in json.dumps(events), events
        expected_counts = {**before, "Cancelled": 1}
        if not eof_after_cancel:
            expected_counts.update(PromptAccepted=2, ModelStarted=models + 1, Answer=1)
        assert counts() == expected_counts, counts()
        reports[case] = dict(events=events, journal_counts=counts())
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait(timeout=10)
        if not proc.stdin.closed:
            proc.stdin.close()
        reader.join(timeout=5)
        proc.stdout.close()
        proc.stderr.close()
        control.unlink()
        pending.unlink(missing_ok=True)
        pending.with_suffix(".pending.tmp").unlink(missing_ok=True)

(output / "packaged-cancellation.json").write_text(json.dumps({
    "runtime": runtime, "composition": composition, "cases": reports,
}, indent=2) + "\n")
