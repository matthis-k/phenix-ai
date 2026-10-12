"""Exercise the real CLI against packaged M1 native fixtures."""
import json
import subprocess
import sys
import tempfile
from pathlib import Path
kernel, fixtures, golden_path, output = sys.argv[1:]
output = Path(output)
output.mkdir(parents=True)
fixtures = Path(fixtures)
def invoke(policy, text, success=True):
    with tempfile.TemporaryDirectory() as tmp:
        deployment = Path(tmp) / "deployment.json"
        request = Path(tmp) / "request.bin"
        deployment.write_text(json.dumps(policy))
        value = text.encode()
        request.write_bytes(b"PV\x02\0" + bytes([8]) + (1).to_bytes(4, "big") + (4).to_bytes(4, "big")
                            + b"text" + bytes([5]) + len(value).to_bytes(4, "big") + value)
        result = subprocess.run([kernel, "invoke", str(deployment), str(request)], text=True, capture_output=True)
        assert result.returncode == (0 if success else 1), result.stderr
        return json.loads(result.stdout)
choices = json.loads((fixtures / "composition-policy.json").read_text())
choice = invoke(choices, "product")
assert choice["format"] == "phenix.inspect.v2"
assert choice["outcome"] == dict(kind="success", value=dict(text="PRODUCT"))
assert choice["bindings"] == sorted(choice["bindings"], key=lambda b: (b["caller"], b["operation"]))
scoped_policy = json.loads((fixtures / "scoped-policy.json").read_text())
scoped = invoke(scoped_policy, "red")
assert scoped["outcome"] == dict(kind="success", value=dict(count=1))
assert scoped["calls"] == 3
assert scoped["roots"]["completed"] == 1
for text, calls, provider in [("blue", 1, "example.relay"), ("switch", 2, "example.resources")]:
    denied = invoke(scoped_policy, text, False)
    assert denied["last_failure"]["code"] == "Denied"
    assert denied["last_failure"]["phase"] == "invocation"
    assert denied["last_failure"]["context"]["provider"] == provider
    assert denied["calls"] == calls
    assert denied["denied_operations"] == 1
    (output / (text + ".json")).write_text(json.dumps(denied, sort_keys=True, indent=2))
(output / "product.json").write_text(json.dumps(dict(choices=choice, scoped=scoped), sort_keys=True, indent=2))
print("M1 CLI product conformance passed")

def compare(actual, expected):
    if isinstance(expected, dict):
        assert isinstance(actual, dict)
        for key, value in expected.items():
            compare(actual[key], value)
    else:
        assert type(actual) is type(expected) and actual == expected, (actual, expected)
compare(choice, json.loads(Path(golden_path).read_text()))
