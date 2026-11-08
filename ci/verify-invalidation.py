#!/usr/bin/env python3
"""Probe actual check derivation paths in an isolated, disposable source copy."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile


root = Path(__file__).resolve().parents[1]


def inventory(path):
    return json.loads(subprocess.check_output([
        "nix", "eval", "--json", f"path:{path}#checks.x86_64-linux",
        "--apply", "checks: builtins.mapAttrs (_: value: value.drvPath) checks",
    ], text=True))


with tempfile.TemporaryDirectory(prefix="phenix-invalidation-", dir="/tmp/opencode") as temp:
    source = Path(temp) / "source"
    shutil.copytree(root, source, ignore=shutil.ignore_patterns(".git", "target"))
    baseline = inventory(source)
    for relative, unchanged, changed in [
        ("docs/infrastructure.md", set(baseline), set()),
        ("packages/phenix-storage/src/lib.rs", {"graph", "workflow"}, {"storage", "clippy", "format"}),
        ("packages/phenix-graphs/src/lib.rs", {"storage", "workflow"}, {"graph", "kernel", "clippy", "format"}),
        ("packages/phenix-schema/src/lib.rs", {"workflow"}, {"storage", "graph", "kernel", "integration", "clippy", "format", "kernel-smoke"}),
    ]:
        path = source / relative
        original = path.read_bytes()
        try:
            path.write_bytes(original + b"\n// invalidation probe\n")
            current = inventory(source)
            actual = {name for name in baseline if baseline[name] != current[name]}
            assert not (actual & unchanged), (relative, "unexpected rebuild", actual & unchanged)
            assert changed <= actual, (relative, "missing rebuild", changed - actual)
            print(f"{relative}: changed {', '.join(sorted(actual)) or 'none'}")
        finally:
            path.write_bytes(original)
