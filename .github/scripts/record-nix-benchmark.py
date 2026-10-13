#!/usr/bin/env python3
"""Convert the exact-head full-Nix-check runner's log into auditable metrics."""
import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess


def command(*args):
    return subprocess.check_output(args, text=True, stderr=subprocess.PIPE).strip()


def write_metrics(mode, directory):
    base = Path(directory)
    output = base / "full-check.log"
    raw = output.read_text(errors="replace")
    # Nix prefixes mixed Cargo output with '<drv-name>'; this is a
    # per-compilation count, not merely the number of unique Rust crates.
    compilation = Counter()
    by_derivation = Counter()
    drv_paths = []
    for line in raw.splitlines():
        match = re.search(r"\bCompiling ([A-Za-z0-9_-]+) v\d", line)
        if match:
            compilation[match.group(1)] += 1
        match = re.search(r"\bbuilding '(/nix/store/[^\s']+\.drv)'", line)
        if match:
            drv_paths.append(match.group(1))
            by_derivation[match.group(1).rsplit("/", 1)[-1].split("-", 1)[-1].removesuffix(".drv")] += 1

    check_names = json.loads(command(
        "nix", "eval", "--json", "--apply", "builtins.attrNames",
        ".#checks.x86_64-linux"))
    if not isinstance(check_names, list) or not check_names:
        raise ValueError("missing evaluated Nix checks")
    check_names = sorted(check_names)
    checkout = command("git", "rev-parse", "HEAD")
    status = int((base / "exit.txt").read_text().strip())
    elapsed = int((base / "elapsed.txt").read_text().strip())
    data = {
        "schemaVersion": 1, "mode": mode, "commit": checkout,
        "checkedAtUtc": datetime.now(timezone.utc).isoformat(),
        "runnerImage": os.environ.get("ImageOS", os.environ.get("RUNNER_OS", "")),
        "checks": check_names,
        "checkInventorySha256": hashlib.sha256(
            json.dumps(check_names, separators=(",", ":")).encode()).hexdigest(),
        "status": "pass" if status == 0 else "fail",
        "exitCode": status,
        "elapsedSeconds": elapsed,
        "cargoCompilations": dict(sorted(compilation.items())),
        "cargoCompileTotal": sum(compilation.values()),
        "rustCrateUniqueCount": len(compilation),
        "nixBuildInvocations": len(drv_paths),
        "nixDerivationUniqueCount": len(set(drv_paths)),
        "nixDerivations": sorted(set(drv_paths)),
        "nixDerivationBuildByName": dict(sorted(by_derivation.items())),
        "sourceLogSha256": hashlib.sha256(output.read_bytes()).hexdigest(),
    }
    (base / "metrics.json").write_text(json.dumps(data, indent=2, sort_keys=True) + "\n")
    print(json.dumps({
        "commit": checkout, "mode": mode, "elapsedSeconds": elapsed,
        "nixDerivations": len(set(drv_paths)),
        "cargoCompilations": sum(compilation.values()),
        "status": data["status"],
    }, sort_keys=True))


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("--mode", choices=["cold", "warm"], required=True)
    p.add_argument("--directory", required=True)
    opts = p.parse_args()
    write_metrics(opts.mode, opts.directory)
