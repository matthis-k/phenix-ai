#!/usr/bin/env python3
"""Standalone, dependency-free Cargo oracle for workspace membership semantics."""
import json
from pathlib import Path
import subprocess
import tempfile


def main():
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
                    f'[package]\nname = "{name}"\nversion = "0.1.0"\n')
            (root / "Cargo.toml").write_text('[workspace]\nresolver = "2"\nmembers = ["packages/*"]\n')
            subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=root,
                           capture_output=True, text=True, check=True)
            (root / "Cargo.toml").write_text(
                '[workspace]\nresolver = "2"\nmembers = ' + json.dumps(members)
                + '\nexclude = ' + json.dumps(excludes) + '\n')
            lock = subprocess.run(["cargo", "generate-lockfile", "--offline"],
                                  cwd=root, capture_output=True, text=True)
            if lock.returncode:
                expected_missing = "src/Cargo.toml" if members == ["packages/**"] else "empty/Cargo.toml"
                assert (members == ["packages/**"] or "empty" in members) and expected_missing in lock.stderr, lock.stderr
                before = (root / "Cargo.lock").read_bytes()
                result = subprocess.run(
                    ["cargo", "metadata", "--offline", "--locked", "--no-deps", "--format-version=1"],
                    cwd=root, capture_output=True, text=True)
                assert result.returncode and expected_missing in result.stderr, result.stderr
                assert before == (root / "Cargo.lock").read_bytes()
                print(f"ORACLE {members=} {excludes=}: rejected missing {expected_missing}")
                continue
            before = (root / "Cargo.lock").read_bytes()
            result = subprocess.run(
                ["cargo", "metadata", "--offline", "--locked", "--no-deps", "--format-version=1"],
                cwd=root, capture_output=True, text=True, check=True)
            data = json.loads(result.stdout)
            names = sorted(p["name"] for p in data["packages"]
                           if p["id"] in data["workspace_members"])
            expected = ([] if excludes == ["packages"] else
                        ["b"] if excludes == ["packages/a"] else ["a", "b"])
            assert names == expected, (members, excludes, names)
            assert before == (root / "Cargo.lock").read_bytes()
            print(f"ORACLE {members=} {excludes=}: {names}")


if __name__ == "__main__":
    main()
