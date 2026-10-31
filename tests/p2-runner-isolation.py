#!/usr/bin/env python3
"""Evaluate independent runner-source perturbations without editing the checkout."""

import json
from pathlib import Path
import shutil
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]
RUNNERS = {
    "phenix-p2-model-meter-product",
    "phenix-p2-compaction-product",
    "phenix-p2-source-evidence",
    "phenix-p2-evidence-store",
    "phenix-p2-project-storage",
}


def evaluate(snapshot):
    result = subprocess.run(
        [
            "nix", "eval", "--json", "--no-write-lock-file",
            f"path:{snapshot}#runnerIsolation.x86_64-linux",
            "--apply",
            "groups: builtins.mapAttrs (_: values: "
            "builtins.mapAttrs (_: drv: drv.drvPath) values) groups",
        ],
        check=True, capture_output=True, text=True,
    )
    return json.loads(result.stdout)


def main():
    # Include untracked, nonignored task edits when run locally; never copy build
    # outputs or .git, and use path: so Nix sees each temporary source edit.
    files = subprocess.check_output(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=ROOT,
    ).decode().split("\0")
    with tempfile.TemporaryDirectory(prefix="phenix-runner-isolation-") as temporary:
        snapshot = Path(temporary) / "source"
        snapshot.mkdir()
        for name in filter(None, files):
            source = ROOT / name
            if not source.is_file():
                continue
            destination = snapshot / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, destination)

        baseline = evaluate(snapshot)
        if set(baseline["runners"]) != RUNNERS:
            raise AssertionError("Expected exactly the five actual runner derivations")
        if set(baseline["controls"]) != {"kernel", "environment", "sdk"}:
            raise AssertionError("Missing kernel, Environment or SDK control")
        print("Baseline drvPaths:", json.dumps(baseline, indent=2), flush=True)
        for runner in sorted(RUNNERS):
            source = snapshot / "testing/products/phenix-p2-products/src/bin" / f"{runner}.rs"
            original = source.read_bytes()
            try:
                source.write_bytes(original + b"\n")
                perturbed = evaluate(snapshot)
                changed = {
                    name for name in RUNNERS
                    if perturbed["runners"][name] != baseline["runners"][name]
                }
                if changed != {runner}:
                    raise AssertionError(f"{runner}: expected only itself to change, got {sorted(changed)}")
                if perturbed["controls"] != baseline["controls"]:
                    raise AssertionError(f"{runner}: kernel/Environment/SDK control changed")
            finally:
                source.write_bytes(original)
            if evaluate(snapshot) != baseline:
                raise AssertionError(f"{runner}: restored snapshot differs from baseline")
            print(f"PASS {runner}: exactly 1/5 changed; controls unchanged; baseline restored", flush=True)


if __name__ == "__main__":
    main()
