"""Black-box trace of a submitted message across frontend, agent, model and output.

The tested model is phenix.model.scripted, a deterministic implementation of
the production Basic Model interface. Never uses network auth or user secrets.
"""
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import threading

runtime, composition, mode, destination = sys.argv[1:]
assert mode in ("jsonl", "acp"), mode
out = Path(destination)
out.mkdir(parents=True, exist_ok=True)
env = dict(os.environ)
for key in (
    "OPENAI_API_KEY", "ANTHROPIC_API_KEY", "OPENROUTER_API_KEY",
    "PHENIX_MODEL_HTTP_API_KEY", "PHENIX_CREDENTIAL_API_KEY_ENV_NAME",
):
    env.pop(key, None)
env["PHENIX_BASIC_SQLITE_PATH"] = str(out / "diagnostic.sqlite")
probe = "phenix-hop-diagnostic-v1"
observed = []


def verify(stage, condition, *, detail=None):
    observed.append({"stage": stage, "passed": bool(condition)})
    if not condition:
        raise AssertionError(f"{mode}: {stage}: {detail!r}")


def journal_events(session, items):
    return [item for item in items if item["session"] == session]


def trace_order(session, events, expected_reply, *, run=None):
    events = journal_events(session, events)
    if run is None:
        accepted = [e for e in events
                    if e["kind"] == "PromptAccepted" and e["text"] == probe]
        verify("prompt_persisted", len(accepted) == 1, detail=events)
        run = accepted[0]["run"]
    by_run = [e for e in events if e["run"] == run]
    kinds = [e["kind"] for e in by_run]
    verify("agent_prompt_accepted", kinds.count("PromptAccepted") == 1, detail=kinds)
    verify("agent_model_dispatched", kinds.count("ModelStarted") == 1, detail=kinds)
    verify("model_returned_expected_text", [e["text"] for e in by_run
           if e["kind"] == "Answer"] == [expected_reply], detail=by_run)
    verify("model_text_journaled", "".join(e["text"] for e in by_run
           if e["kind"] == "TextDelta") == expected_reply, detail=by_run)
    verify("journal_order", [kinds.index(k) for k in
           ("PromptAccepted", "ModelStarted", "TextDelta", "Answer")]
           == sorted(kinds.index(k) for k in
           ("PromptAccepted", "ModelStarted", "TextDelta", "Answer")), detail=kinds)
    verify("journal_monotonic", all(a["sequence"] < b["sequence"]
           for a, b in zip(by_run, by_run[1:])), detail=by_run)
    return run


def jsonl(*packets):
    result = subprocess.run(
        [runtime, "jsonl", composition],
        input="".join(json.dumps(item) + "\n" for item in packets),
        capture_output=True, text=True, env=env, timeout=45,
    )
    verify("jsonl_process_exited", result.returncode == 0,
           detail=result.stderr[-1200:])
    return [json.loads(line) for line in result.stdout.splitlines()]


def acp():
    child = subprocess.Popen(
        [runtime, "acp", composition], env=env, stdin=subprocess.PIPE,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1,
    )
    received = queue.Queue()
    updates = []

    def reader():
        for line in child.stdout:
            received.put(line)
        received.put(None)

    threading.Thread(target=reader, daemon=True).start()

    def rpc(number, method, params):
        child.stdin.write(json.dumps({
            "jsonrpc": "2.0", "id": number, "method": method, "params": params,
        }) + "\n")
        child.stdin.flush()
        for _ in range(128):
            try:
                raw = received.get(timeout=30)
            except queue.Empty as cause:
                raise AssertionError(f"ACP {method} timed out") from cause
            verify("acp_process_alive", raw is not None, detail=method)
            frame = json.loads(raw)
            if frame.get("method") == "session/update":
                updates.append(frame["params"])
                continue
            verify("acp_reply_correlated", frame.get("id") == number, detail=frame)
            verify("acp_rpc_accepted_" + method, "result" in frame, detail=frame)
            return frame["result"]
        raise AssertionError("ACP sent too many notifications")

    try:
        init = rpc(1, "initialize", {
            "protocolVersion": 1, "clientCapabilities": {},
        })
        verify("acp_initialized", init["protocolVersion"] == 1)
        created = rpc(2, "session/new", {
            "cwd": str(out), "mcpServers": [],
        })
        session = created["sessionId"]
        verify("acp_session_created", bool(session))

        start = len(updates)
        answer = rpc(3, "session/prompt", {
            "sessionId": session,
            "prompt": [{"type": "text", "text": probe}],
        })
        verify("acp_prompt_completed", answer["stopReason"] == "end_turn", detail=answer)
        messages = [x["update"] for x in updates[start:]
                    if x["sessionId"] == session]
        user = [x.get("content", {}).get("text", "") for x in messages
                if x.get("sessionUpdate") == "user_message_chunk"]
        assistant = [x.get("content", {}).get("text", "") for x in messages
                     if x.get("sessionUpdate") == "agent_message_chunk"]
        verify("acp_frontend_input_echo", "".join(user) == probe, detail=messages)
        verify("acp_frontend_model_output", "".join(assistant) == "mock:" + probe,
               detail=messages)
        verify("acp_delivery_order", next(i for i, x in enumerate(messages)
               if x.get("sessionUpdate") == "user_message_chunk") <
               next(i for i, x in enumerate(messages)
               if x.get("sessionUpdate") == "agent_message_chunk"))
        return session
    finally:
        if child.poll() is None:
            child.stdin.close()
            code = child.wait(timeout=30)
            stderr = child.stderr.read()
            verify("acp_process_exited", code == 0, detail=stderr[-1200:])
        child.stdout.close()
        child.stderr.close()


try:
    if mode == "jsonl":
        session = "hop-jsonl"
        events = jsonl({"type": "prompt", "session": session, "text": probe})
        trace_order(session, events, "mock:" + probe)
        tool_events = jsonl({"type": "prompt", "session": session, "text": "tools:hello"})
        started = [e for e in tool_events if e["kind"] == "ToolStarted"]
        finished = [e for e in tool_events if e["kind"] == "ToolFinished"]
        verify("model_tool_calls_requested", [e["text"] for e in started]
               == ["uppercase", "length"], detail=started)
        verify("tools_invoked_and_completed", [e["call_id"] for e in started]
               == [e["call_id"] for e in finished] and len(finished) == 2,
               detail=finished)
        verify("model_tool_continuation", [e["text"] for e in tool_events
               if e["kind"] == "Answer"] == ["mock:call-1:HELLO|call-2:5"],
               detail=tool_events)
        # A Model failure must leave an observable failed journal outcome and
        # permit an independent later prompt; no model reply can be invented.
        error_events = jsonl({"type": "prompt", "session": session, "text": "model-error"})
        kinds = [e["kind"] for e in error_events]
        verify("model_failure_attributed", kinds.count("PromptAccepted") == 1
               and kinds.count("ModelStarted") == 1
               and kinds.count("Failure") == 1
               and "Answer" not in kinds, detail=error_events)
        recovered = jsonl({"type": "prompt", "session": session, "text": "after-error"})
        verify("subsequent_turn_works", [e["text"] for e in recovered
               if e["kind"] == "Answer"] == ["mock:after-error"], detail=recovered)
    else:
        session = acp()
        # Independent JSONL observer reads the persisted journal of the
        # previously completed ACP session. This separates graph/model effects
        # from ACP notification delivery and does not submit another prompt.
        events = jsonl({"type": "cancel", "session": session})
        trace_order(session, events, "mock:" + probe)
        verify("acp_observation_independent", all(e["session"] == session
               for e in events), detail=events)
except Exception as error:
    (out / "trace.json").write_text(json.dumps({
        "format": "phenix.p1.prompt-hop.v1", "mode": mode,
        "passed": False, "checks": observed,
        "failed_at": observed[-1]["stage"] if observed else "test_harness",
        "error": type(error).__name__,
    }, indent=2))
    raise
else:
    (out / "trace.json").write_text(json.dumps({
        "format": "phenix.p1.prompt-hop.v1", "mode": mode,
        "passed": True, "checks": observed,
    }, indent=2))
    print(f"ACP/Basic prompt hop: {mode} verified {len(observed)} boundaries")
