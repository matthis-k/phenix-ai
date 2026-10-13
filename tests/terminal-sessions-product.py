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
assert (output / "terminal-sessions.db").is_file()

(output / "terminal-sessions.json").write_text(json.dumps({
    "format": "phenix.basic.terminal-sessions.v1",
    "cases": [
        "default-session-backward-compatibility",
        "explicit-session-selection",
        "independent-session-restart",
        "observer-cursor-isolation",
        "invalid-session-identity-refused",
    ],
}, indent=2, sort_keys=True))
