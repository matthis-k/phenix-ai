"""P2-3k: exercise selected Model and token-meter exports through real M1 roots.

The fixture tokenizer is exact for its own synthetic model, not an HTTP model.
Both calls must use the same loaded provider and source ModelRequest.
"""
import json
from pathlib import Path
import subprocess
import sys

runner, fixture, basic_defs, advanced_defs, outdir = sys.argv[1:]
out = Path(outdir)
out.mkdir(parents=True, exist_ok=True)
process = subprocess.run(
    [runner, fixture, basic_defs, advanced_defs, str(out)],
    capture_output=True, text=True, timeout=90,
)
assert process.returncode == 0, (process.stdout, process.stderr)
report = json.loads((out / "model-meter-product.json").read_text())
assert report["model"] == "phenix.fixture.portable-byte-model@1", report
assert report["tokenizer"] == "portable-byte-fixture-v1", report
assert report["response"] == "fixture:hello", report
assert report["full_request_measurement"] is True, report
assert report["model_and_meter_share_provider"] is True, report
assert report["input_tokens_match_model_usage"] is True, report
assert report["changed_tool_schema_counted"] is True, report
assert report["changed_context_counted"] is True, report
assert report["request_change_denied"] is True, report
assert report["cross_session_denied"] is True, report
(out / "accepted.json").write_text(json.dumps({
    "format": "phenix.p2.model-token-meter.v1",
    "accepted": True,
    "model": report["model"],
    "cases": [
        "real-native-kernel-root",
        "same-model-and-meter-provider",
        "complete-request-token-count",
        "tool-schema-and-context-changes-counted",
        "cross-session-denied",
        "portable-request-fingerprint-validated",
    ],
}, indent=2, sort_keys=True))
