"""Offline descriptor retention and canonical identity proof. No ctypes/dlopen."""
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

describe, *packages = sys.argv[1:]
with tempfile.TemporaryDirectory() as work:
    work = Path(work)
    canonical = None
    records = []
    stripped_records = []
    for package in packages:
        package = Path(package)
        record = json.loads((package / "package.json").read_text())
        artifact = package / record["artifact"]
        descriptor = package / record["descriptor"]
        assert hashlib.sha256(artifact.read_bytes()).hexdigest() == record["artifact_sha256"]
        assert hashlib.sha256(descriptor.read_bytes()).hexdigest() == record["descriptor_sha256"]
        if canonical is None:
            canonical = descriptor.read_bytes()
        assert descriptor.read_bytes() == canonical
        records.append(dict(package_path=str(package), **record))
        for mode in ["--strip-debug", "--strip-unneeded", "--strip-all"]:
            stripped = work / (mode + ".so")
            subprocess.run(["objcopy", mode, str(artifact), str(stripped)], check=True)
            output = work / "descriptor.bin"
            probe = json.loads(subprocess.check_output([describe, str(stripped), str(output)], text=True))
            assert output.read_bytes() == canonical
            assert probe["descriptor_sha256"] == record["descriptor_sha256"]
            assert probe["artifact_sha256"] == hashlib.sha256(stripped.read_bytes()).hexdigest()
            stripped_records.append(dict(package_path=str(package), strip_mode=mode, **probe))
        broken = work / "without-section.so"
        subprocess.run(["objcopy", "--remove-section=.phenix", str(artifact), str(broken)], check=True)
        assert subprocess.run([describe, str(broken), str(work / "bad.bin")], capture_output=True).returncode != 0
    bad = work / "bad.so"
    bad.write_bytes(b"\x7fELF truncated")
    assert subprocess.run([describe, str(bad), str(work / "bad.bin")], capture_output=True).returncode != 0
    print(json.dumps(dict(format="phenix.p0-evidence.v1", packages=records, stripped_artifacts=stripped_records, binutils=subprocess.check_output(["ld", "--version"], text=True).splitlines()[0], configurations=["thin-gnu", "fat-gnu", "off-gnu", "thin-lld-gc", "thin-gnu-repeat"], strip_modes=["debug", "unneeded", "all"])))
