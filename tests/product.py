"""Real CLI check of the immutable Nix deployment closure."""
import json
import os
import subprocess
import sys
from pathlib import Path

kernel, deployment, request, output = sys.argv[1:]
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
assert loaded["bindings"] == {"example.transform@1/apply": "example.provider"}
assert all(p["abi_negotiated"] for p in loaded["plugins"])
for plugin in loaded["plugins"]:
    assert len(plugin["artifact_sha256"]) == 64 and len(plugin["descriptor_sha256"]) == 64
(output / "product.json").write_text(json.dumps(loaded, sort_keys=True, indent=2))
print("M0.0 product CLI passed")
