"""Real host restart and failed-delivery checks for independent observer cursors."""
import json
import os
from pathlib import Path
import subprocess
import sys

binary, composition, output_dir = sys.argv[1:]
output = Path(output_dir)
output.mkdir(parents=True, exist_ok=True)


def run(mode, prompt=""):
    result = subprocess.run(
        [binary, mode, composition], input=prompt, text=True,
        capture_output=True, timeout=45,
    )
    assert result.returncode == 0, result.stderr
    return result.stdout


# The preceding product smoke delivered a direct prompt through Terminal,
# then a multi-tool prompt through JSONL. Terminal still has the tool turn.
pending = run("terminal")
assert "mock:call-1:HELLO|call-2:5" in pending, pending
assert "answer: mock:hello" not in pending, pending
assert run("terminal") == ""
assert run("jsonl") == ""

fresh = run("terminal", "observer-final\n")
assert "answer: mock:observer-final" in fresh, fresh
assert "mock:call-1:HELLO|call-2:5" not in fresh, fresh
independent = run("jsonl")
assert "mock:observer-final" in independent, independent
assert run("terminal") == ""
assert run("jsonl") == ""

# A real pipe with no reader makes writes fail. The host must not acknowledge
# the newly appended page, which must be delivered after a process restart.
read_fd, write_fd = os.pipe()
os.close(read_fd)
try:
    failed = subprocess.run(
        [binary, "terminal", composition], input="observer-undelivered\n",
        text=True, stdout=write_fd, stderr=subprocess.PIPE, timeout=45,
    )
finally:
    os.close(write_fd)
assert failed.returncode != 0, failed.stderr
replayed = run("terminal")
assert "answer: mock:observer-undelivered" in replayed, replayed
assert run("terminal") == ""
assert "mock:observer-undelivered" in run("jsonl")
assert run("jsonl") == ""

(output / "observer-product.json").write_text(json.dumps({
    "format": "phenix.basic.observer-product.v1",
    "cases": ["terminal-restart-offset", "jsonl-restart-offset",
              "independent-observers", "failed-pipe-write-replay"],
    "delivery": "at-least-once",
}, indent=2, sort_keys=True))
