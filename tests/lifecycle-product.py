"""Real CLI lifecycle and typed shutdown error propagation."""
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

kernel, fixtures, output = sys.argv[1:]
policy = Path(fixtures) / "lifecycle-policy.json"
output = Path(output)
output.mkdir(parents=True)
reports = []
with tempfile.TemporaryDirectory() as directory:
    directory = Path(directory)
    request = directory / "request.bin"
    text = b"lifecycle"
    request.write_bytes(bytes([8]) + (1).to_bytes(4, "big") + (4).to_bytes(4, "big")
                       + b"text" + bytes([5]) + len(text).to_bytes(4, "big") + text)
    trace = directory / "trace"
    environment = dict(os.environ, PHENIX_LIFECYCLE_TRACE=str(trace), PHENIX_LIFECYCLE_FAULTS="consumer:start")
    offline = subprocess.run([kernel, "inspect", str(policy)], env=environment, text=True, capture_output=True)
    assert offline.returncode == 0, offline.stderr
    assert not trace.exists(), "CLI inspection executed start"
    full = ["provider:start", "consumer:start", "provider:invoke", "consumer:stop", "consumer:drop", "provider:stop", "provider:drop"]
    for faults, fail_call in [("", False), ("consumer:start", False), ("consumer:stop", False), ("consumer:stop-panic", False), ("consumer:stop", True)]:
        text = b"fail" if fail_call else b"lifecycle"
        request.write_bytes(bytes([8]) + (1).to_bytes(4, "big") + (4).to_bytes(4, "big")
                           + b"text" + bytes([5]) + len(text).to_bytes(4, "big") + text)
        trace.unlink(missing_ok=True)
        environment["PHENIX_LIFECYCLE_FAULTS"] = faults
        result = subprocess.run([kernel, "invoke", str(policy), str(request)], env=environment, text=True, capture_output=True)
        assert result.returncode == (1 if faults else 0), result.stderr
        report = json.loads(result.stdout)
        observed = trace.read_text().splitlines()
        if faults == "consumer:start":
            assert report["last_failure"]["code"] == "PluginFailed"
            assert report["last_failure"]["phase"] == "executable_abi"
            assert observed == [item for item in full if item != "provider:invoke"]
        else:
            assert report["format"] == "phenix.inspect.v2"
            if fail_call:
                failure = report["outcome"]["failure"]
                assert report["outcome"]["kind"] == "kernel_failure"
                assert failure["message"] == "provider:invoke"
                assert failure["phase"] == "invocation"
                assert failure["context"]["provider"] == "example.lifecycle.z-provider"
                assert report["shutdown_failure"]["message"] == "consumer:stop"
            else:
                assert report["outcome"] == dict(kind="success", value=dict(text="LIFECYCLE"))
            assert report["lifecycle"]["stopped"] is True
            assert observed == full
            if faults:
                assert report["shutdown_failure"]["code"] == "PluginFailed"
                assert report["last_failure"]["phase"] == "lifecycle"
                assert report["lifecycle"]["stop_failures"][0]["provider"] == "example.lifecycle.a-consumer"
            else:
                assert report["lifecycle"]["stop_failures"] == []
                assert "shutdown_failure" not in report
        reports.append(dict(faults=faults, fail_call=fail_call, report=report, events=observed))
(output / "product.json").write_text(json.dumps(reports, sort_keys=True, indent=2))
print("M0.2 synchronous lifecycle CLI slice passed")
