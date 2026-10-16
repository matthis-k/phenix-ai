"""Black-box acceptance of an additive Advanced Model provider.

Every answer crosses the real Basic graph -> Advanced Model adapter ->
selected upstream Model. There is no second agent loop or model reply cache.
The verification backend has no hidden history: only the persisted Session
and actual forwarded ModelRequest can explain its results.
"""
import json
import os
from pathlib import Path
import subprocess
import sys

basic_exe, advanced_exe, verify_exe, basic_json, advanced_json, verify_json, out_dir = sys.argv[1:]
out = Path(out_dir)
out.mkdir(parents=True, exist_ok=True)
basic = json.loads(Path(basic_json).read_text())
advanced = json.loads(Path(advanced_json).read_text())
verify = json.loads(Path(verify_json).read_text())

def bound(spec, caller, slot):
    xs = [x for x in spec["slots"] if x["caller"] == caller and x["slot"] == slot]
    assert len(xs) == 1, (caller, slot, xs)
    return xs[0]["provider"]

adapter = "phenix.advanced.model-adapter"
assert bound(basic, "phenix.basic.agent", "Model") == "phenix.model.scripted"
assert bound(advanced, "phenix.basic.agent", "Model") == adapter
assert bound(verify, "phenix.basic.agent", "Model") == adapter
assert bound(advanced, adapter, "Upstream") == "phenix.model.scripted"
assert bound(verify, adapter, "Upstream") == "phenix.model.history-test"
assert advanced["grants"] == verify["grants"]
assert advanced["authority"] == verify["authority"]
assert any("phenix-plugin-advanced-model-adapter" in str(x)
           for x in advanced["artifacts"])
assert not any("phenix-plugin-advanced-model-adapter" in str(x)
               for x in basic["artifacts"])

def run(binary, db, prompts):
    env = dict(os.environ)
    env["PHENIX_BASIC_SQLITE_PATH"] = str(out / db)
    payload = "".join(json.dumps({"type": "prompt", "session": sid, "text": text}) + "\n"
                      for sid, text in prompts)
    proc = subprocess.run([binary, "jsonl"], input=payload, text=True,
                          capture_output=True, env=env, timeout=90)
    assert proc.returncode == 0, (proc.stdout, proc.stderr)
    events = [json.loads(line) for line in proc.stdout.splitlines()]
    return events, [event["text"] for event in events if event["kind"] == "Answer"]

# Basic is untouched; Advanced wraps the exact same scripted model.
events, answers = run(advanced_exe, "advanced.sqlite",
                      [("s1", "hello"), ("s1", "tools:hello"), ("s2", "hello")])
assert answers == ["mock:hello", "mock:call-1:HELLO|call-2:5", "mock:hello"], (answers, events)
assert [e["kind"] for e in events].count("ToolStarted") == 2
assert [e["kind"] for e in events].count("ToolFinished") == 2

# Model requests must preserve the session ID, full transcript, tool calls,
# tool results, and durable state across a new process.
_, answers = run(verify_exe, "verification.sqlite",
                 [("s1", "remember:color=violet"),
                  ("s2", "remember:color=amber"),
                  ("s1", "verify-assistant:color"),
                  ("s2", "recall:color")])
assert answers == ["stored:color=violet", "stored:color=amber",
                   "verified:color=violet", "recalled:color=amber"], answers
_, answers = run(verify_exe, "verification.sqlite",
                 [("s1", "tools:hello"), ("s2", "recall:color")])
assert answers == ["tool-result:HELLO", "recalled:color=amber"], answers
_, answers = run(verify_exe, "verification.sqlite",
                 [("s1", "recall-tool"), ("s1", "recall:color")])
assert answers == ["tool-recalled:HELLO", "recalled:color=violet"], answers

(out / "p2-model-adapter-product.json").write_text(json.dumps({
    "accepted": True,
    "adapter": adapter,
    "cases": [
        "basic-unchanged",
        "advanced-model-bound-through-adapter",
        "verification-upstream-selected-in-runtime",
        "tool-call-and-response-continuation",
        "cross-session-isolation",
        "source-transcript-survives-process-restart",
        "no-test-provider-in-public-product",
    ]
}, indent=2, sort_keys=True))
