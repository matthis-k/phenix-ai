#!/usr/bin/env python3
"""Strictly compare cold and warm Nix checks and write a machine-readable report."""
import json
import sys
from pathlib import Path

def compare(a, b):
    cold, warm = json.loads(Path(a).read_text()), json.loads(Path(b).read_text())
    for data, mode in ((cold, "cold"), (warm, "warm")):
        if data.get("mode") != mode or data.get("schemaVersion") != 1:
            raise ValueError(f"invalid {mode} benchmark document")
    for k in ("commit", "checks", "checkInventorySha256", "runnerImage"):
        if cold.get(k) != warm.get(k):
            raise ValueError(f"cold/warm {k} differs")
    for mode, data in (("cold", cold), ("warm", warm)):
        if data["status"] != "pass" or data["exitCode"] != 0:
            raise ValueError(f"{mode} checks failed; no valid performance comparison")
        if data["elapsedSeconds"] < 0:
            raise ValueError("negative elapsed time")
    report = {
        "schemaVersion": 1,
        "commit": cold["commit"],
        "checkCount": len(cold["checks"]),
        "cold": {k: cold[k] for k in ("elapsedSeconds", "nixDerivationUniqueCount", "cargoCompileTotal")},
        "warm": {k: warm[k] for k in ("elapsedSeconds", "nixDerivationUniqueCount", "cargoCompileTotal")},
        "speedup": round(cold["elapsedSeconds"] / max(1, warm["elapsedSeconds"]), 3),
        "cargoCompileSaved": cold["cargoCompileTotal"] - warm["cargoCompileTotal"],
        "nixDerivationsSaved": cold["nixDerivationUniqueCount"] - warm["nixDerivationUniqueCount"],
    }
    target = Path(a).parent.parent / "summary.json"
    target.write_text(json.dumps(report, sort_keys=True, indent=2) + "\n")
    print(json.dumps(report, sort_keys=True, indent=2))

if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit("usage: compare-nix-benchmarks.py COLD.json WARM.json")
    try:
        compare(*sys.argv[1:])
    except (ValueError, KeyError, OSError) as e:
        print(f"benchmark comparison invalid: {e}", file=sys.stderr)
        raise SystemExit(1)
