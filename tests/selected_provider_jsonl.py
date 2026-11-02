"""Actual selected HTTP JSONL acceptance, seeded only through ACP config APIs."""
import json
import ctypes
import os
import queue
import select
import sqlite3
import subprocess
import threading
import time


def assert_request_failure(event):
    assert event["kind"] == "Failure" and event["sequence"] == 0, event
    assert event["run"] == event["call_id"] == "", event

    def text(value):
        encoded = value.encode()
        return len(encoded).to_bytes(4, "big") + encoded

    expected = (b"PV\x02\x00\x08\x00\x00\x00\x02" + text("code") + b"\x05"
                + text("ModelFailed") + text("message") + b"\x05" + text(event["text"]))
    assert bytes(event["data"]) == expected, "request Failure must encode portable BasicError"


def exercise(fixture):
    start, call = fixture["start"], fixture["request"]
    output, database = fixture["output"], fixture["sqlite_path"]
    received, modes = fixture["received"], fixture["http_modes"]
    timeout = fixture["REQUEST_DEADLINE_SECONDS"]
    fault = fixture["release_fault"]
    children = fixture["children"]

    def seed(default_session=False):
        if default_session:
            # Establish s1 through normal JSONL ingress, then select its model
            # through ACP. This also acknowledges the unselected baseline turn.
            baseline = launch()
            send(baseline, "s1", "baseline-before-selected-release-fault")
            terminal(baseline, "s1", "Answer")
            finish(baseline)
        acp = start()
        fixture["initialize"](acp)
        created = (fixture["load"](acp, "s1", 2) if default_session else
                   call(acp, 2, "session/new", {"cwd": "/tmp", "mcpServers": []}))
        session = "s1" if default_session else created["result"]["sessionId"]
        _, offers = fixture["model_options"](created)
        selected = call(acp, 3, "session/set_config_option", {
            "sessionId": session, "configId": "model",
            "value": fixture["chosen"](offers, "fixture-a"),
        })
        fixture["confirmed"](selected, "fixture-a", 1)
        fixture["close"](acp)
        return session

    def launch(revoked=False, composition=None, extra_env=None):
        env = dict(fixture["base_env"])
        env.update(extra_env or {})
        if revoked:
            env["PHENIX_C9_TEST_REVOKED"] = "1"
        child = subprocess.Popen([
            fixture["runtime"], "jsonl", composition or fixture["http_basic"],
            "--provider-management", fixture["management"],
        ], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, bufsize=1, env=env)
        children.append(child)
        child.frames = queue.Queue()
        child.events = []

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
            assert event is not None, ("JSONL exited before terminal", child.poll(), child.events)
            child.events.append(event)
            if event["session"] == session and event["kind"] in ("Answer", "Failure", "Cancelled"):
                assert event["kind"] == kind, child.events
                return event

    def request_failure(child, session):
        event = terminal(child, session, "Failure")
        assert_request_failure(event)
        return event

    def finish(child, success=True):
        child.stdin.close()
        status = child.wait(timeout=timeout)
        diagnostic = child.stderr.read()
        while True:
            event = child.frames.get(timeout=timeout)
            if event is None:
                break
            child.events.append(event)
        assert (status == 0) == success, (status, diagnostic, child.events)
        assert "unselected-wrong-credential" not in diagnostic + json.dumps(child.events)
        assert "forbidden-late-output" not in json.dumps(child.events)
        return diagnostic

    reports = []
    if fixture.get("pending_basic"):
        session = seed()
        base = json.loads(fixture["Path"](fixture["http_basic"]).read_text())
        pending = json.loads(fixture["Path"](fixture["pending_basic"]).read_text())
        # Preserve the release-fault HTTP artifact in the fault gate.
        pending["artifacts"] = [a for a in pending["artifacts"] if "model-http" not in a]
        pending["artifacts"] += [a for a in base["artifacts"] if a not in pending["artifacts"]]
        composition = output / "selected-pending-composition.json"
        composition.write_text(json.dumps(pending))
        control = output / "selected-tool.control"
        control.write_text("hold\n")
        libc = ctypes.CDLL(None, use_errno=True)
        descriptor = libc.inotify_init1(os.O_CLOEXEC | os.O_NONBLOCK)
        assert descriptor >= 0
        assert libc.inotify_add_watch(descriptor, os.fsencode(output), 0x100 | 0x80 | 0x8) >= 0

        def witness(suffix):
            path = control.with_suffix(suffix)
            deadline = time.monotonic() + timeout
            while not path.exists():
                ready, _, _ = select.select([descriptor], [], [], max(0, deadline - time.monotonic()))
                assert ready, ("tool witness timeout", suffix, child.poll(), list(child.frames.queue))
                os.read(descriptor, 65536)
            return json.loads(path.read_text())

        try:
            child = launch(composition=str(composition), extra_env={
                "PHENIX_TOOL_A_CONTROL_FILE": str(control),
            })
            before = len(received)
            modes.put("pending-tool")
            send(child, session, "actual-pending-tool")
            invocation = witness(".pending")
            assert invocation["call"]["id"] == "selected-pending", invocation
            with sqlite3.connect(database) as db:
                (journal,) = db.execute(
                    "SELECT data FROM basic_records WHERE session=? AND name='event-journal.v1'",
                    (session,),
                ).fetchone()
            assert journal.count(b"ModelStarted") == journal.count(b"ToolStarted") == 1
            assert all(journal.count(k) == 0 for k in (b"ToolFinished", b"Answer", b"Failure", b"Cancelled"))
            assert received[before:] == ["fixture-a"]
            send(child, session)
            assert witness(".cancelled") == invocation
            assert control.with_suffix(".pending").exists()
            with sqlite3.connect(database) as db:
                (held,) = db.execute(
                    "SELECT data FROM basic_records WHERE session=? AND name='event-journal.v1'",
                    (session,),
                ).fetchone()
            assert held == journal, "cancellation settled before physical tool drain"
            # No terminal can precede physical drain. The tool's late success is
            # deliberately returned by its cancelled poll after this barrier.
            control.write_text("drain\n")
            if fault:
                assert "Unavailable" in request_failure(child, session)["text"]
                send(child, session, "quarantined-after-tool")
                assert "quarantin" in request_failure(child, session)["text"]
            else:
                terminal(child, session, "Cancelled")
                send(child, session, "unreconciled-tool-must-not-replay")
                assert "Indeterminate" in request_failure(child, session)["text"]
                fresh = seed()
                send(child, fresh, "fresh-install-after-tool")
                terminal(child, fresh, "Answer")
            diagnostic = finish(child)
            assert witness(".drained") == invocation
            assert not control.with_suffix(".pending").exists()
            assert "forbidden late success" not in json.dumps(child.events)
            assert not any(e["kind"] == "ToolFinished" for e in child.events)
            assert received[before:] == ["fixture-a"] * (1 if fault else 2)
            if fault:
                # With Model idle, both cancellation's early release and final
                # cleanup reach the fault fixture (neither is reentrancy-blocked).
                assert [line for line in diagnostic.splitlines()
                        if line.startswith("test-release-fault:")] == [
                            f"test-release-fault: {session} 1"] * 2, diagnostic
            reports.append({"actual_pending_tool": invocation, "release_fault": fault,
                            "events": child.events, "physical_drain": True})
        finally:
            os.close(descriptor)

    session = seed()
    child = launch()
    before = len(received)
    fixture["http_entered"].clear()
    fixture["http_unblock"].clear()
    fixture["http_drained"].clear()
    modes.put("cancel")
    send(child, session, "held-during-input-failure")
    assert fixture["http_entered"].wait(timeout)
    # A bounded-reader error, not a malformed JSON packet deferred until idle.
    child.stdin.write("x" * (1024 * 1024 + 1) + "\n")
    child.stdin.flush()
    failed = request_failure(child, session)
    assert "stdin frame exceeds 1 MiB" in failed["text"], failed
    fixture["http_unblock"].set()
    assert fixture["http_drained"].wait(timeout)
    diagnostic = finish(child)
    assert fixture["http_errors"].empty(), list(fixture["http_errors"].queue)
    assert not any(e["kind"] in ("Answer", "Cancelled") for e in child.events)
    assert received[before:] == ["fixture-a"]
    if fault:
        assert f"test-release-fault: {session} 1" in diagnostic
    else:
        resumed = launch()
        send(resumed, session, "after-abnormal-drain")
        recovered = terminal(resumed, session, "Failure")
        assert recovered["text"] == "run interrupted after owner process exited", recovered
        terminal(resumed, session, "Answer")
        finish(resumed)
        assert received[before:] == ["fixture-a"] * 2
    reports.append({"input_reader_failure_while_http_held": True,
                    "peer_closed": True, "release_fault": fault,
                    "events": child.events})

    for mode in ("success", "failure", "cancel"):
        session, other = seed(default_session=fault and mode == "success"), seed()
        before = len(received)
        child = launch()
        fixture["http_entered"].clear()
        fixture["http_unblock"].clear()
        fixture["http_drained"].clear()
        modes.put(mode)
        send(child, session, mode)
        assert fixture["http_entered"].wait(timeout), "selected HTTP not entered"
        if mode == "cancel":
            with sqlite3.connect(database) as db:
                (journal,) = db.execute(
                    "SELECT data FROM basic_records WHERE session=? AND name='event-journal.v1'",
                    (session,),
                ).fetchone()
            assert journal.count(b"PromptAccepted") == journal.count(b"ModelStarted") == 1
            assert all(journal.count(kind) == 0 for kind in
                       (b"Answer", b"Failure", b"Cancelled", b"ToolStarted"))
            # The server cannot complete before the observed cancellation terminal.
            send(child, session)
        if fault:
            result = request_failure(child, session)
            if mode == "failure":
                assert result["text"] == "HTTP model status 503", result
            else:
                assert "Unavailable" in result["text"], result
            assert not any(e["kind"] in ("Answer", "Cancelled") for e in child.events)
            fixture["http_unblock"].set()
            send(child, session, "must-not-recreate")
            denied = request_failure(child, session)
            assert "quarantin" in denied["text"], denied
            assert received[before:] == ["fixture-a"], received
            unselected = "unselected-" + session
            send(child, unselected, "unrelated-unselected-survives")
            terminal(child, unselected, "Answer")
            send(child, other, "unrelated-selected-has-own-fault")
            request_failure(child, other)
            send(child, other, "third-session-also-quarantined")
            request_failure(child, other)
            send(child, unselected, "still-alive-after-third-fault")
            terminal(child, unselected, "Answer")
            diagnostic = finish(child)
            assert received[before:] == ["fixture-a", "fixture-b", "fixture-a", "fixture-b"], received
        else:
            terminal(child, session, {"success": "Answer", "failure": "Failure",
                                      "cancel": "Cancelled"}[mode])
            fixture["http_unblock"].set()
            send(child, other, "independent-after-" + mode)
            terminal(child, other, "Answer")
            send(child, session, "same-session-after-" + mode)
            terminal(child, session, "Answer")
            diagnostic = finish(child)
            assert received[before:] == ["fixture-a"] * 3, received
            assert sum(e["kind"] == "Answer" for e in child.events) == (3 if mode == "success" else 2)
        fixture["http_unblock"].set()
        if mode == "cancel":
            assert fixture["http_drained"].wait(timeout), "cancelled HTTP transport did not close"
            assert fixture["http_errors"].empty(), list(fixture["http_errors"].queue)
        terminals = [e for e in child.events if e["session"] == session
                     and e["kind"] in ("Answer", "Failure", "Cancelled")]
        expected = {"success": "Answer", "failure": "Failure", "cancel": "Cancelled"}[mode]
        assert [e["kind"] for e in terminals] == (["Failure", "Failure"] if fault else
                                                  [expected, "Answer"]), terminals
        if mode == "failure":
            assert terminals[0]["text"] == "HTTP model status 503", terminals
        if fault:
            releases = [line for line in diagnostic.splitlines()
                        if line.startswith("test-release-fault:")]
            assert releases == [f"test-release-fault: {session} 1",
                                f"test-release-fault: {other} 1"], releases
            with sqlite3.connect(database) as db:
                journals = dict(db.execute(
                    "SELECT session, data FROM basic_records "
                    "WHERE session IN (?, ?) AND name='event-journal.v1'",
                    (session, other),
                ))
            # Cleanup failure suppresses observer success, not the valid record
            # of what the Model did. Do not 'fix' replay by deleting terminals.
            assert journals[session].count(expected.encode()) == (2 if session == "s1" else 1)
            assert journals[other].count(b"Answer") == 1
            restart_before = len(received)
            startup = launch()
            finish(startup)
            assert startup.events == [], (mode, "startup replayed suppressed terminal", startup.events)
            assert len(received) == restart_before, "startup observation reached HTTP"
            resumed = launch()
            send(resumed, session)
            send(resumed, other)
            finish(resumed)
            assert resumed.events == [], (mode, "idle cancel replayed suppressed terminal", resumed.events)
            assert len(received) == restart_before, "idle cancellation reached HTTP"
            with sqlite3.connect(database) as db:
                after_replay = dict(db.execute(
                    "SELECT session, data FROM basic_records "
                    "WHERE session IN (?, ?) AND name='event-journal.v1'",
                    (session, other),
                ))
            assert after_replay == journals, "observer suppression modified semantic journal"
            # Restarted observation must not quarantine unrelated selected work.
            fresh = seed()
            independent = launch()
            send(independent, session)
            send(independent, fresh, "selected-after-suppression-restart")
            assert "Unavailable" in request_failure(independent, fresh)["text"]
            finish(independent)
            assert received[restart_before:] == ["fixture-a"], received
            assert not any(e["session"] in (session, other) for e in independent.events), independent.events
            assert not any(e["kind"] in ("Answer", "Cancelled") for e in independent.events), independent.events
            reports.append({"suppressed_terminal_restart": mode,
                            "default_session_startup": session == "s1",
                            "startup_events": startup.events,
                            "idle_cancel_events": resumed.events,
                            "semantic_journals_unchanged": True,
                            "unrelated_selected_http_after_restart": True})
        reports.append({"outcome": mode, "http_requests": len(received) - before,
                        "events": child.events, "release_fault": fault})

    if not fault:
        session, other = seed(), seed()
        owner = launch()
        modes.put("cancel")
        fixture["http_entered"].clear()
        fixture["http_unblock"].clear()
        fixture["http_drained"].clear()
        before = len(received)
        send(owner, session, "held-jsonl-owner")
        assert fixture["http_entered"].wait(timeout)
        contender = launch()
        send(contender, session, "excluded-jsonl-contender")
        denied = request_failure(contender, session)
        assert "session already owned" in denied["text"], denied
        finish(contender)
        assert received[before:] == ["fixture-a"], received
        independent = launch()
        send(independent, other, "independent-while-owner-held")
        terminal(independent, other, "Answer")
        finish(independent)
        assert not fixture["http_unblock"].is_set()
        assert received[before:] == ["fixture-a", "fixture-a"], received
        send(owner, session)
        terminal(owner, session, "Cancelled")
        fixture["http_unblock"].set()
        finish(owner)
        assert fixture["http_drained"].wait(timeout)
        assert fixture["http_errors"].empty(), list(fixture["http_errors"].queue)
        resumed = launch()
        send(resumed, session, "after-owner-release")
        terminal(resumed, session, "Answer")
        # Revalidate the committed model on every turn in the same JSONL process.
        acp = start()
        fixture["initialize"](acp)
        loaded = fixture["load"](acp, session, 2)
        _, offers = fixture["model_options"](loaded)
        fixture["confirmed"](call(acp, 3, "session/set_config_option", {
            "sessionId": session, "configId": "model",
            "value": fixture["chosen"](offers, "fixture-b"),
        }), "fixture-b", 2)
        fixture["close"](acp)
        send(resumed, session, "revalidate-new-model")
        terminal(resumed, session, "Answer")
        finish(resumed)
        assert received[before:] == ["fixture-a"] * 3 + ["fixture-b"], received
        reports.append({"cross_process_exclusion": True,
                        "independent_progress_before_release": True,
                        "model_change_revalidated_in_live_process": True})
        for denial in ("revoked", "missing-identity", "corrupt-identity", "malformed-prompt"):
            session = seed()
            if denial == "missing-identity":
                with sqlite3.connect(database) as db:
                    db.execute("DELETE FROM basic_records WHERE session=? AND name=?",
                               (session, "phenix.provider.selected-model.v1"))
            if denial == "corrupt-identity":
                with sqlite3.connect(database) as db:
                    db.execute("UPDATE basic_records SET data=? WHERE session=? AND name=?",
                               (b"invalid-native-selection", session,
                                "phenix.provider.selected-model.v1"))
            before = len(received)
            child = launch(revoked=denial == "revoked")
            send(child, session, "" if denial == "malformed-prompt" else "denied")
            request_failure(child, session)
            finish(child)
            assert len(received) == before, (denial, received)
            assert not any(e["kind"] in ("ModelStarted", "Answer") for e in child.events)
            reports.append({"denial": denial, "http_requests": 0})
    (output / "selected-jsonl.json").write_text(json.dumps({
        "cases": reports, "legitimate_acp_selection": True,
        "wrong_preset_ignored": not fault, "sleeps_used": False,
    }, indent=2) + "\n")
