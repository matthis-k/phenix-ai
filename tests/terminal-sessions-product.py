"""Verify explicit Terminal session selection and isolated observer offsets."""
import json
import os
from pathlib import Path
import subprocess
import sys

binary, composition, destination = sys.argv[1:]
output = Path(destination)
output.mkdir(parents=True, exist_ok=True)
env = dict(os.environ)
env["PHENIX_BASIC_SQLITE_PATH"] = str(output / "terminal-sessions.db")


def terminal(session=None, prompt=""):
    command = [binary, "terminal", composition]
    if session is not None:
        command += ["--session", session]
    result = subprocess.run(
        command, input=prompt, text=True, capture_output=True,
        env=env, timeout=45,
    )
    assert result.returncode == 0, (command, result.stdout, result.stderr)
    return result.stdout


# The default launch and an explicit s1 selection use one durable session.
assert "answer: mock:first" in terminal(prompt="first\n")
assert terminal() == ""
assert "answer: mock:second" in terminal("s2", "second\n")
assert "answer: mock:third" in terminal("s1", "third\n")
assert terminal("s2") == "", "s2 observer replayed acknowledged events"
assert terminal("s1") == "", "s1 observer replayed acknowledged events"
assert "answer: mock:fourth" in terminal("s2", "fourth\n")
assert "answer: mock:fifth" in terminal(prompt="fifth\n")
assert terminal("s1") == ""
assert terminal("s2") == ""

# Invalid session names must never be admitted as authority resources.
invalid = subprocess.run(
    [binary, "terminal", composition, "--session", "../s2"],
    input="", text=True, capture_output=True, env=env, timeout=20,
)
assert invalid.returncode != 0
assert not any(token in invalid.stdout for token in ("answer:", "mock:"))
# The same failing-tool Model fixture used by the independent ACP SDK
# must continue through a successful tool and a final answer in Terminal.
tool_reply = terminal("s2", "tools-fail:hello\n")
assert "answer: mock:call-1:error:scripted failure|call-2:5" in tool_reply
assert terminal("s2") == "", "Terminal replayed an acknowledged failed-tool turn"

# Terminal exits on a Model domain error. Restarting the same session must
# permit a fresh prompt without re-executing its previous effects.
model_failure = subprocess.run(
    [binary, "terminal", composition, "--session", "s2"],
    input="model-error\n", text=True, capture_output=True,
    env=env, timeout=45,
)
assert model_failure.returncode != 0, (
    model_failure.stdout, model_failure.stderr
)
resumed = terminal("s2", "terminal-after-model-error\n")
assert "answer: mock:terminal-after-model-error" in resumed
assert terminal("s2") == "", "Terminal repeated acknowledged recovery output"
assert "answer: mock:terminal-isolated" in terminal(
    "s1", "terminal-isolated\n"
)
assert terminal("s1") == ""

assert (output / "terminal-sessions.db").is_file()

(output / "terminal-sessions.json").write_text(json.dumps({
    "format": "phenix.basic.terminal-sessions.v1",
    "cases": [
        "default-session-backward-compatibility",
        "explicit-session-selection",
        "independent-session-restart",
        "observer-cursor-isolation",
        "invalid-session-identity-refused",
        "failed-tool-model-continuation",
        "model-failure-process-restart",
    ],
}, indent=2, sort_keys=True))
