#!/usr/bin/env python3
"""Standalone, dependency-free Cargo oracle for workspace membership semantics."""

import json
from pathlib import Path
import subprocess
import tempfile
import sys


def validate_metadata(expected):
    before = Path("Cargo.lock").read_bytes()
    metadata = json.loads(
        subprocess.check_output(
            [
                "cargo",
                "metadata",
                "--offline",
                "--locked",
                "--all-features",
                "--format-version=1",
            ],
            text=True,
        )
    )
    local = [p for p in metadata["packages"] if p["source"] is None]
    inventory = {
        p["name"]: str(Path(p["manifest_path"]).parent.relative_to(Path.cwd()))
        for p in local
    }
    edges = {
        p["name"]: sorted(
            set(
                inventory[d["name"]]
                for d in p["dependencies"]
                if d.get("path") is not None
            )
        )
        for p in local
    }
    assert inventory == expected["inventory"], (inventory, expected)
    assert edges == expected["edges"], (edges, expected)
    assert before == Path("Cargo.lock").read_bytes()
    print("Full offline locked Cargo metadata inventory and local edges match Nix")


def dependency_oracle():
    repository = Path(__file__).resolve().parents[1]
    with tempfile.TemporaryDirectory(prefix="cargo-closure-oracle-") as tmp:
        root = Path(tmp)
        names = [
            "seed",
            "normal",
            "builder",
            "development",
            "targeted",
            "optional",
            "macro",
            "target-builder",
            "target-development",
        ]
        for name in names:
            package = root / name
            (package / "src").mkdir(parents=True)
            (package / "src/lib.rs").write_text("")
            (package / "Cargo.toml").write_text(
                f'[package]\nname = "{name}"\nversion = "0.1.0"\nedition = "2021"\n'
                + ("\n[lib]\nproc-macro = true\n" if name == "macro" else "")
            )
        (root / "Cargo.toml").write_text(
            '[workspace]\nresolver = "2"\nmembers = '
            + json.dumps(names)
            + '\n[workspace.dependencies]\nalias = { package = "normal", path = "normal" }\n'
        )
        with (root / "seed/Cargo.toml").open("a") as manifest:
            manifest.write("""
[dependencies]
alias.workspace = true
optional = { path = "../optional", optional = true }
macro = { path = "../macro" }
[build-dependencies]
builder = { path = "../builder" }
[dev-dependencies]
development = { path = "../development" }
[target.'cfg(windows)'.dependencies]
targeted = { path = "../targeted" }
[target.'cfg(windows)'.build-dependencies]
target-builder = { path = "../target-builder" }
[target.'cfg(windows)'.dev-dependencies]
target-development = { path = "../target-development" }
""")
        with (root / "normal/Cargo.toml").open("a") as manifest:
            manifest.write('\n[dependencies]\nmacro = { path = "../macro" }\n')
        subprocess.run(
            ["cargo", "generate-lockfile", "--offline"], cwd=root, check=True
        )
        before = (root / "Cargo.lock").read_bytes()
        metadata = json.loads(
            subprocess.check_output(
                [
                    "cargo",
                    "metadata",
                    "--offline",
                    "--locked",
                    "--all-features",
                    "--format-version=1",
                ],
                cwd=root,
                text=True,
            )
        )
        expression = f"""let
          flake = builtins.getFlake {json.dumps(str(repository))};
          helper = import {repository}/ci/cargo-workspace.nix {{
            lib = flake.inputs.nixpkgs.lib; root = {tmp};
          }};
        in {{ inventory = helper.packages; closure = helper.closure "seed";
          edges = builtins.mapAttrs (_: path: builtins.sort builtins.lessThan (helper.localDependencies path)) helper.packages; }}"""
        actual = json.loads(
            subprocess.check_output(
                ["nix", "eval", "--impure", "--json", "--expr", expression], text=True
            )
        )
        expected = {
            p["name"]: sorted(d["name"] for d in p["dependencies"])
            for p in metadata["packages"]
        }
        assert actual["edges"] == expected, (actual, expected)
        assert sorted(actual["inventory"]) == sorted(names)
        assert actual["closure"] == sorted(names)
        assert before == (root / "Cargo.lock").read_bytes()
        print(
            "ORACLE full offline locked metadata: inherited alias, build/dev/target/optional/proc-macro edges"
        )
        seed_manifest = root / "seed/Cargo.toml"
        original = seed_manifest.read_text()
        for modern, legacy in [
            ("[dev-dependencies]", "[dev_dependencies]"),
            ("[build-dependencies]", "[build_dependencies]"),
            (
                "[target.'cfg(windows)'.dev-dependencies]",
                "[target.'cfg(windows)'.dev_dependencies]",
            ),
            (
                "[target.'cfg(windows)'.build-dependencies]",
                "[target.'cfg(windows)'.build_dependencies]",
            ),
        ]:
            seed_manifest.write_text(original.replace(modern, legacy))
            legacy_metadata = json.loads(
                subprocess.check_output(
                    [
                        "cargo",
                        "metadata",
                        "--offline",
                        "--locked",
                        "--all-features",
                        "--format-version=1",
                    ],
                    cwd=root,
                    text=True,
                )
            )
            assert {
                p["name"]: sorted(d["name"] for d in p["dependencies"])
                for p in legacy_metadata["packages"]
            } == expected
            rejected = subprocess.run(
                ["nix", "eval", "--impure", "--json", "--expr", expression],
                capture_output=True,
                text=True,
            )
            assert (
                rejected.returncode
                and "unsupported legacy dependency table" in rejected.stderr
            ), rejected.stderr
            assert before == (root / "Cargo.lock").read_bytes()
        seed_manifest.write_text(original)
        print(
            "ORACLE Cargo accepts legacy package/target tables; helper explicitly rejects all four"
        )


def main():
    dependency_oracle()
    for members, excludes in [
        (["packages/*"], ["missing"]),
        (["packages/*"], ["empty"]),
        (["packages/*"], ["packages/a*"]),
        (["packages/*"], ["packages/a"]),
        (["packages/*"], ["packages"]),
        (["packages/*", "empty"], ["empty"]),
        (["packages/**"], []),
    ]:
        with tempfile.TemporaryDirectory(prefix="cargo-workspace-oracle-") as tmp:
            root = Path(tmp)
            (root / "empty").mkdir()
            for name in ("a", "b"):
                package = root / "packages" / name
                (package / "src").mkdir(parents=True)
                (package / "src/lib.rs").touch()
                (package / "Cargo.toml").write_text(
                    f'[package]\nname = "{name}"\nversion = "0.1.0"\n'
                )
            (root / "Cargo.toml").write_text(
                '[workspace]\nresolver = "2"\nmembers = ["packages/*"]\n'
            )
            subprocess.run(
                ["cargo", "generate-lockfile", "--offline"],
                cwd=root,
                capture_output=True,
                text=True,
                check=True,
            )
            (root / "Cargo.toml").write_text(
                '[workspace]\nresolver = "2"\nmembers = '
                + json.dumps(members)
                + "\nexclude = "
                + json.dumps(excludes)
                + "\n"
            )
            lock = subprocess.run(
                ["cargo", "generate-lockfile", "--offline"],
                cwd=root,
                capture_output=True,
                text=True,
            )
            if lock.returncode:
                expected_missing = (
                    "src/Cargo.toml"
                    if members == ["packages/**"]
                    else "empty/Cargo.toml"
                )
                assert (
                    members == ["packages/**"] or "empty" in members
                ) and expected_missing in lock.stderr, lock.stderr
                before = (root / "Cargo.lock").read_bytes()
                result = subprocess.run(
                    [
                        "cargo",
                        "metadata",
                        "--offline",
                        "--locked",
                        "--no-deps",
                        "--format-version=1",
                    ],
                    cwd=root,
                    capture_output=True,
                    text=True,
                )
                assert result.returncode and expected_missing in result.stderr, (
                    result.stderr
                )
                assert before == (root / "Cargo.lock").read_bytes()
                print(
                    f"ORACLE {members=} {excludes=}: rejected missing {expected_missing}"
                )
                continue
            before = (root / "Cargo.lock").read_bytes()
            result = subprocess.run(
                [
                    "cargo",
                    "metadata",
                    "--offline",
                    "--locked",
                    "--no-deps",
                    "--format-version=1",
                ],
                cwd=root,
                capture_output=True,
                text=True,
                check=True,
            )
            data = json.loads(result.stdout)
            names = sorted(
                p["name"]
                for p in data["packages"]
                if p["id"] in data["workspace_members"]
            )
            expected = (
                []
                if excludes == ["packages"]
                else ["b"]
                if excludes == ["packages/a"]
                else ["a", "b"]
            )
            assert names == expected, (members, excludes, names)
            assert before == (root / "Cargo.lock").read_bytes()
            print(f"ORACLE {members=} {excludes=}: {names}")


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--metadata":
        validate_metadata(json.loads(Path(sys.argv[2]).read_text()))
    else:
        main()
