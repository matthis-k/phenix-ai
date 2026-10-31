#!/usr/bin/env python3
"""Evaluate workspace and runner source isolation without editing the checkout."""

import json
import argparse
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import tomllib


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


def evaluate_workspace(snapshot):
    result = evaluate(snapshot)
    result["http"] = subprocess.check_output(
        ["nix", "eval", "--raw", "--no-write-lock-file",
         f"path:{snapshot}#packages.x86_64-linux.phenix-plugin-model-http.drvPath"],
        text=True,
    )
    return result


def workspace_experiments(snapshot):
    baseline = evaluate_workspace(snapshot)
    manifest = snapshot / "Cargo.toml"
    flake = snapshot / "flake.nix"
    original_manifest = manifest.read_bytes()
    original_flake = flake.read_bytes()
    lock = (snapshot / "Cargo.lock").read_bytes()
    old = "plugins/shared/phenix-plugin-model-http"
    new = old + "-relocated"
    plugin = snapshot / old
    plugin_bytes = {str(p.relative_to(plugin)): p.read_bytes()
                    for p in plugin.rglob("*") if p.is_file()}
    text = original_manifest.decode()
    members = tomllib.loads(text)["workspace"]["members"]
    members_entry = re.search(r"(?m)^members\s*=\s*\[[^\]]*\]", text)
    if members_entry is None:
        raise AssertionError("Root workspace members entry not found")
    if old not in members:
        raise AssertionError("HTTP must be an explicit root workspace member")
    # The extra glob matches the already explicit HTTP member: membership is unchanged.
    irrelevant_members = list(reversed(members)) + ["plugins/shared/phenix-plugin-model-*"]
    experiments = [
        ("root membership formatting/reorder and unrelated overlapping glob", text.replace(
            members_entry.group(), "members = [\n  " + ",\n  ".join(map(json.dumps, irrelevant_members)) + ",\n]")),
        ("real HTTP relocation", text.replace(old, new)),
        ("release profile positive control", text.replace("codegen-units = 1", "codegen-units = 2")),
    ]
    for label, changed_manifest in experiments:
        if changed_manifest == text:
            raise AssertionError(f"{label}: mutation did not apply")
        try:
            manifest.write_text(changed_manifest)
            if label == "real HTTP relocation":
                plugin.rename(snapshot / new)
                flake.write_bytes(original_flake.replace(old.encode(), new.encode()))
                if old.encode() in flake.read_bytes().replace(new.encode(), b""):
                    raise AssertionError("Old HTTP path remains in temporary flake")
                moved_bytes = {str(p.relative_to(snapshot / new)): p.read_bytes()
                               for p in (snapshot / new).rglob("*") if p.is_file()}
                if moved_bytes != plugin_bytes:
                    raise AssertionError("Relocation changed plugin source bytes")
            perturbed = evaluate_workspace(snapshot)
            if label == "release profile positive control":
                for group in ("runners", "controls"):
                    for name, drv in baseline[group].items():
                        if perturbed[group][name] == drv:
                            raise AssertionError(f"{label}: {name} did not change")
            elif any(perturbed[group] != baseline[group] for group in ("runners", "controls")):
                raise AssertionError(f"{label}: unrelated runner/kernel/Environment/SDK changed")
            http_changed = perturbed["http"] != baseline["http"]
            if http_changed != (label != experiments[0][0]):
                raise AssertionError(f"{label}: unexpected HTTP derivation identity")
        finally:
            manifest.write_bytes(original_manifest)
            flake.write_bytes(original_flake)
            if (snapshot / new).exists():
                (snapshot / new).rename(plugin)
            if (snapshot / "Cargo.lock").read_bytes() != lock:
                raise AssertionError(f"{label}: Cargo.lock changed")
            if evaluate_workspace(snapshot) != baseline:
                raise AssertionError(f"{label}: restored snapshot differs from baseline")
        print(f"PASS {label}: all nine identities checked; baseline restored", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--subset-from", metavar="REV",
                        help="negative control: use only REV's subset implementation in the snapshot")
    args = parser.parse_args()
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

        if args.subset_from:
            previous = subprocess.check_output(
                ["git", "show", f"{args.subset_from}:flake.nix"], cwd=ROOT, text=True,
            )
            flake = snapshot / "flake.nix"
            current = flake.read_text()
            start = "      subset ="
            end = "      # Production Rust SDK sources"
            flake.write_text(current[:current.index(start)]
                             + previous[previous.index(start):previous.index(end)]
                             + current[current.index(end):])
        workspace_experiments(snapshot)
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
