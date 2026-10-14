"""Reject oversized and invalid-UTF8 stdio frames at the SDK transport boundary."""
import subprocess
import sys

binary = sys.argv[1]
for name, frame in [
    ("oversized", b"x" * (1024 * 1024) + b"\n"),
    ("invalid-utf8", b"\xff\n"),
]:
    proc = subprocess.Popen(
        [binary],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    try:
        stdout, stderr = proc.communicate(input=frame, timeout=15)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.communicate()
        raise AssertionError(f"{name}: SDK host did not fail closed")
    assert proc.returncode != 0, (
        name, "bad frame was silently accepted", stdout, stderr
    )
    assert b'"result"' not in stdout, (name, stdout)
print("official-acp-sdk-v1: bounded stdin rejected invalid physical frames")
