#!/usr/bin/env python3
"""Prove Nix derivation identities use selected source, not unrelated root paths.

Evaluation-only: no extra Rust compilation. Work solely in a temporary snapshot.
"""
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
MEMBER = "plugins/shared/phenix-plugin-model-http"
MOVED = "plugins/basic/phenix-plugin-model-http"
CONTROLS = {"kernel", "sdk", "environment", "sqlite", "scripted",
            "basic-runtime", "p2-runner"}
EXPECTED = CONTROLS | {"http", "contract"}


def evaluate(snapshot):
    completed = subprocess.run([
        "nix", "eval", "--json", "--no-write-lock-file",
        f"path:{snapshot}#sourceIsolation.x86_64-linux",
        "--apply", "outputs: builtins.mapAttrs (_: drv: drv.drvPath) outputs",
    ], capture_output=True, text=True, check=True)
    actual = json.loads(completed.stdout)
    if set(actual) != EXPECTED or not all(p.endswith(".drv") for p in actual.values()):
        raise AssertionError("incomplete, non-derivation source identity inventory")
    return actual


def assert_unchanged(before, after, names, label):
    drift = {name for name in names if before[name] != after[name]}
    if drift:
        raise AssertionError(f"{label}: unrelated derivations changed: {sorted(drift)}")


def snapshot_git_tree(destination):
    files = subprocess.check_output(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=ROOT,
    ).decode().split("\0")
    destination.mkdir()
    for name in filter(None, files):
        source = ROOT / name
        if not source.is_file():
            continue
        target = destination / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)


def main():
    with tempfile.TemporaryDirectory(prefix="phenix-workspace-source-") as temp:
        snapshot = Path(temp) / "source"
        snapshot_git_tree(snapshot)
        manifest = snapshot / "Cargo.toml"
        flake = snapshot / "flake.nix"
        original_manifest = manifest.read_text()
        original_flake = flake.read_text()
        baseline = evaluate(snapshot)

        # An unrelated workspace member must never become part of source input
        # identities for existing isolated packages, including full check source
        # closures that do not include the unselected member's actual directory.
        match = re.search(r'(?m)^members = \[([^\n]*)\]$', original_manifest)
        if not match or MEMBER not in match.group(1):
            raise AssertionError("unexpected root workspace manifest structure")
        canary = "plugins/shared/phenix-plugin-isolation-canary"
        canary_dir = snapshot / canary
        canary_dir.mkdir(parents=True)
        (canary_dir / "Cargo.toml").write_text(
            '[package]\nname = "phenix-isolation-canary"\nversion = "0.1.0"\nedition = "2021"\n'
        )
        (canary_dir / "src").mkdir()
        (canary_dir / "src/lib.rs").write_text("pub fn canary() {}\n")
        manifest.write_text(original_manifest.replace(
            match.group(0), 'members = ["' + canary + '", ' + match.group(1) + ']',
            1))
        try:
            assert_unchanged(baseline, evaluate(snapshot), EXPECTED,
                             "unselected workspace member addition")
            print("PASS: an unrelated member changes no existing derivation", flush=True)
        finally:
            manifest.write_text(original_manifest)
            shutil.rmtree(canary_dir)

        # Selected workspace metadata must still affect the derivation graph;
        # no cache key is allowed to conceal a real profile change.
        old_profile = "codegen-units = 1"
        if original_manifest.count(old_profile) != 1:
            raise AssertionError("release profile baseline changed")
        manifest.write_text(original_manifest.replace(old_profile, "codegen-units = 2"))
        try:
            changed = evaluate(snapshot)
            if changed["kernel"] == baseline["kernel"] or changed["http"] == baseline["http"]:
                raise AssertionError("selected workspace profile change was ignored")
            print("PASS: relevant workspace build settings invalidate affected derivations", flush=True)
        finally:
            manifest.write_text(original_manifest)

        # Simulate a REAL byte-preserving path relocation, including both
        # workspace membership and Nix source declarations. The moved HTTP
        # derivation and whole-workspace contract check may change; other
        # independent build identities must remain bit-for-bit stable.
        if original_manifest.count(MEMBER) != 1 or MEMBER not in original_flake:
            raise AssertionError("HTTP plugin move fixture no longer matches tree")
        shutil.move(str(snapshot / MEMBER), str(snapshot / MOVED))
        manifest.write_text(original_manifest.replace(MEMBER, MOVED))
        flake.write_text(original_flake.replace(MEMBER, MOVED))
        try:
            moved = evaluate(snapshot)
            assert_unchanged(baseline, moved, CONTROLS,
                             "HTTP plugin physical relocation")
            if moved["http"] == baseline["http"] or moved["contract"] == baseline["contract"]:
                raise AssertionError("moved plugin or full workspace did not change")
            print("PASS: HTTP move changes only HTTP and whole-workspace contract; "
                  "7 unrelated derivations stay identical", flush=True)
        finally:
            flake.write_text(original_flake)
            manifest.write_text(original_manifest)
            shutil.move(str(snapshot / MOVED), str(snapshot / MEMBER))

        assert_unchanged(baseline, evaluate(snapshot), EXPECTED,
                         "restored workspace snapshot")
        print("PASS: original source identities restored", flush=True)


if __name__ == "__main__":
    main()
