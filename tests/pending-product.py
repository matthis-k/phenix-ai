"""Native pending execution through the installed trusted CLI."""
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path
kernel, fixtures, output = sys.argv[1:]
output = Path(output)
output.mkdir(parents=True)
policy = Path(fixtures) / "composition.json"
reports = []
with tempfile.TemporaryDirectory() as directory:
    directory = Path(directory)
    trace = directory / "trace"
    env = dict(os.environ, PHENIX_PENDING_TRACE=str(trace))
    offline = subprocess.run([kernel, "inspect", str(policy)], env=env, text=True, capture_output=True)
    assert offline.returncode == 0, offline.stderr
    assert not trace.exists(), "offline CLI inspection ran plugin code"
    for text, code in [(b"ready", 0), (b"error", 1), (b"panic", 1)]:
        request = directory / "request.bin"
        request.write_bytes(b"PV\x02\0" + bytes([8]) + (1).to_bytes(4, "big") + (4).to_bytes(4, "big") + b"text" + bytes([5]) + len(text).to_bytes(4, "big") + text)
        result = subprocess.run([kernel, "invoke", str(policy), str(request)], env=env, text=True, capture_output=True)
        assert result.returncode == code, result.stderr
        report = json.loads(result.stdout)
        assert report["pending"]["physical_leases"] == 0
        assert report["pending"]["tickets"] == []
        assert report["lifecycle"]["stopped"] is True
        if code == 0:
            assert report["outcome"] == dict(kind="success", value=dict(text="READY"))
        else:
            assert report["outcome"]["kind"] == "kernel_failure"
            assert report["outcome"]["failure"]["code"] == "PluginFailed"
            assert report["outcome"]["failure"]["phase"] == "invocation"
        reports.append(report)
(output / "evidence.json").write_text(json.dumps(dict(format="phenix.m1.pending.product.v1", reports=reports), sort_keys=True, indent=2))
print("M1 pending CLI conformance passed")
