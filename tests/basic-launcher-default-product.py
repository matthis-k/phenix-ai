"""Exercise the installed Basic ACP launcher with no manual SQLite setup."""
import json
import os
from pathlib import Path
import queue
import stat
import subprocess
import sys
import threading

binary, output = sys.argv[1:]
root = Path(output)
root.mkdir(parents=True, exist_ok=True)
state_home = root / "state"
env = dict(os.environ)
env.pop("PHENIX_BASIC_SQLITE_PATH", None)
env.update(HOME=str(root), XDG_STATE_HOME=str(state_home))


def start(environment):
    proc = subprocess.Popen(
        [binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, text=True, bufsize=1, env=environment,
    )
    messages = queue.Queue()

    def read():
        for line in proc.stdout:
            messages.put(json.loads(line))
        messages.put(None)

    threading.Thread(target=read, daemon=True).start()
    return proc, messages


def rpc(proc, messages, identifier, method, params):
    proc.stdin.write(json.dumps({
        "jsonrpc": "2.0", "id": identifier, "method": method,
        "params": params,
    }) + "\n")
    proc.stdin.flush()
    for _ in range(128):
        try:
            result = messages.get(timeout=15)
        except queue.Empty as problem:
            raise AssertionError(f"ACP did not answer {method}") from problem
        assert result is not None, f"ACP exited during {method}: {proc.poll()}"
        if result.get("method") == "session/update":
            continue
        assert result.get("id") == identifier, result
        assert "error" not in result, result
        return result["result"]
    raise AssertionError(f"ACP exceeded response bound for {method}")


def initialize(proc, messages):
    assert rpc(proc, messages, 1, "initialize", {
        "protocolVersion": 1, "clientCapabilities": {},
    })["protocolVersion"] == 1


def close(proc):
    proc.stdin.close()
    assert proc.wait(timeout=15) == 0, proc.stderr.read()


first, first_messages = start(env)
initialize(first, first_messages)
session = rpc(first, first_messages, 2, "session/new", {
    "cwd": str(root), "mcpServers": [],
})["sessionId"]
assert rpc(first, first_messages, 3, "session/prompt", {
    "sessionId": session,
    "prompt": [{"type": "text", "text": "default storage"}],
})["stopReason"] == "end_turn"
close(first)

database = state_home / "phenix" / "basic.sqlite"
assert database.is_file(), "default ACP launcher never created its SQLite database"
assert stat.S_IMODE(database.stat().st_mode) == 0o600, "default database must be private"

second, second_messages = start(env)
initialize(second, second_messages)
rpc(second, second_messages, 2, "session/load", {
    "sessionId": session, "cwd": str(root), "mcpServers": [],
})
assert rpc(second, second_messages, 3, "session/prompt", {
    "sessionId": session,
    "prompt": [{"type": "text", "text": "after restart"}],
})["stopReason"] == "end_turn"
close(second)

override = root / "explicit" / "nested" / "sessions.sqlite"
explicit_env = dict(env, PHENIX_BASIC_SQLITE_PATH=str(override))
third, third_messages = start(explicit_env)
initialize(third, third_messages)
different = rpc(third, third_messages, 2, "session/new", {
    "cwd": str(root), "mcpServers": [],
})["sessionId"]
assert rpc(third, third_messages, 3, "session/prompt", {
    "sessionId": different,
    "prompt": [{"type": "text", "text": "explicit storage"}],
})["stopReason"] == "end_turn"
close(third)
assert override.is_file(), "explicit SQLite override was not initialized"

# Empty and relative XDG values are unspecified under the XDG convention.
# Exercise the actual installed launcher rather than duplicating its resolver.
for name, xdg in [("empty", ""), ("relative", "relative-state")]:
    home = root / ("home-" + name)
    fallback_env = dict(env, HOME=str(home), XDG_STATE_HOME=xdg)
    proc, messages = start(fallback_env)
    initialize(proc, messages)
    fallback_session = rpc(proc, messages, 2, "session/new", {
        "cwd": str(root), "mcpServers": [],
    })["sessionId"]
    assert rpc(proc, messages, 3, "session/prompt", {
        "sessionId": fallback_session,
        "prompt": [{"type": "text", "text": "fallback storage"}],
    })["stopReason"] == "end_turn"
    close(proc)
    fallback_database = home / ".local/state/phenix/basic.sqlite"
    assert fallback_database.is_file(), fallback_database
    assert stat.S_IMODE(fallback_database.stat().st_mode) == 0o600
assert not Path("relative-state").exists()

for missing in [dict(env, HOME="", XDG_STATE_HOME=""),
                dict(env, HOME="relative-home", XDG_STATE_HOME="")]:
    rejected = subprocess.run(
        [binary], input="", capture_output=True, text=True,
        env=missing, timeout=10,
    )
    assert rejected.returncode != 0
    assert "absolute XDG_STATE_HOME or HOME" in rejected.stderr, rejected.stderr

relative = subprocess.run(
    [binary], input="", capture_output=True, text=True,
    env=dict(env, PHENIX_BASIC_SQLITE_PATH="relative.sqlite"), timeout=10,
)
assert relative.returncode != 0
assert "must be absolute" in relative.stderr, relative.stderr
assert not Path("relative.sqlite").exists()
(root / "accepted.json").write_text(json.dumps({
    "cases": ["unset-default", "session-restart", "absolute-override",
              "reject-relative", "empty-xdg-home-fallback",
              "relative-xdg-home-fallback", "reject-missing-absolute-home"],
    "format": "phenix.basic.sqlite-launcher.v1",
}, sort_keys=True, indent=2))
