"""Real CLI check of the immutable Nix deployment closure."""
import json
import os
import subprocess
import sys
from pathlib import Path
import tempfile

kernel, deployment, request, golden_path, output = sys.argv[1:]
output = Path(output)
output.mkdir(parents=True, exist_ok=True)
witness = output / "constructor-witness"
environment = dict(os.environ, PHENIX_CONSTRUCTOR_WITNESS=str(witness))
offline = json.loads(subprocess.check_output([kernel, "inspect", deployment], env=environment, text=True))
assert offline["format"] == "phenix.inspect.v1"
assert not witness.exists()
assert all(p["offline_verified"] and not p["abi_negotiated"] for p in offline["plugins"])
loaded = json.loads(subprocess.check_output([kernel, "invoke", deployment, request], env=environment, text=True))
assert witness.exists()
assert loaded["outcome"] == dict(kind="success", value=dict(text="PHENIX"))
assert loaded["calls"] == 2
assert loaded["roots"]["completed"] == 1
assert loaded["bindings"] == {"example.transform.apply@1/apply": "example.provider"}
assert all(p["abi_negotiated"] for p in loaded["plugins"])
for plugin in loaded["plugins"]:
    assert len(plugin["artifact_sha256"]) == 64 and len(plugin["descriptor_sha256"]) == 64

def invoke_cli(policy, value, expected_exit=1):
    """Parse a real invocation, preserving its process exit and JSON."""
    with tempfile.TemporaryDirectory() as tmp:
        policy_path = Path(tmp) / "deployment.json"
        request_path = Path(tmp) / "request.bin"
        policy_path.write_text(json.dumps(policy))
        request_path.write_bytes(value)
        result = subprocess.run([kernel, "invoke", str(policy_path), str(request_path)],
                                env=environment, text=True, capture_output=True)
        assert result.returncode == expected_exit, result.stderr
        return json.loads(result.stdout)

policy = json.loads(Path(deployment).read_text())
denied_policy = dict(policy, grants=[g for g in policy["grants"] if g["caller"] == "root"])
denied = invoke_cli(denied_policy, Path(request).read_bytes())
failure = denied["outcome"]["failure"]
assert denied["outcome"]["kind"] == "kernel_failure"
assert failure["code"] == "Denied" and failure["phase"] == "invocation"
assert failure["context"] == dict(caller="example.consumer", provider="example.provider",
                                  operation="example.transform.apply@1/apply")
assert failure == denied["last_failure"]
assert denied["calls"] == 1 and denied["denied_operations"] == 1
assert denied["roots"]["completed"] == 1

# Rejected preparation must not run any native constructor.
witness.unlink()
offline_failure = invoke_cli(dict(policy, grants=[]), Path(request).read_bytes())
assert offline_failure["last_failure"]["code"] == "Denied"
assert offline_failure["last_failure"]["phase"] == "offline_preparation"
assert not witness.exists()
malformed = invoke_cli(policy, bytes([255]))
assert malformed["last_failure"]["code"] == "SchemaRejected"
assert malformed["last_failure"]["phase"] == "offline_preparation"
assert not witness.exists()

def text_request(text):
    payload = text.encode()
    return (bytes([8]) + (1).to_bytes(4, "big") + (4).to_bytes(4, "big") + b"text"
            + bytes([5]) + len(payload).to_bytes(4, "big") + payload)

handled = invoke_cli(policy, text_request("handled-denial"), expected_exit=0)
assert handled["outcome"] == dict(kind="success", value=dict(text="handled"))
assert handled["last_failure"]["code"] == "Denied"
assert handled["last_failure"]["phase"] == "invocation"
assert handled["last_failure"]["context"] == dict(caller="example.consumer", provider=None,
                                                    operation="example.transform.count@1/count")
assert handled["calls"] == 1 and handled["denied_operations"] == 1
replaced = invoke_cli(policy, text_request("replace-failure"))
assert replaced["outcome"]["failure"]["message"] == "consumer replacement failure"
assert replaced["outcome"]["failure"]["context"] == dict(caller="root", provider="example.consumer",
                                                         operation="example.consumer@1/run")

def compare_golden(actual, expected):
    # Additive fields are allowed within v1. Frozen fields must preserve types
    # and values; incompatible changes require a new inspection format version.
    if isinstance(expected, dict):
        assert isinstance(actual, dict)
        for key, value in expected.items():
            compare_golden(actual[key], value)
    else:
        assert type(actual) is type(expected) and actual == expected, (actual, expected)

golden = json.loads(Path(golden_path).read_text())
compare_golden(loaded, golden["success"])
compare_golden(denied, golden["denied"])
compare_golden(handled, golden["handled"])
(output / "product.json").write_text(json.dumps(loaded, sort_keys=True, indent=2))
(output / "failures.json").write_text(json.dumps(dict(denied=denied, offline=offline_failure,
                                                     malformed=malformed, handled=handled,
                                                     replaced=replaced), sort_keys=True, indent=2))
print("M0.0 product CLI passed")
