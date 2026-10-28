#!/usr/bin/env python3
"""Single-authority CI routing and independent required check regressions."""
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / ".github/workflows"

def read(filename):
    return (ROOT / filename).read_text()

def require(ok, message):
    if not ok:
        raise AssertionError(message)

def verify(trusted, pr, canonical, old_matrix, diagnostic, fmt, acp, source):
    require("  pull_request:" not in trusted and "  id-token: write" in trusted
            and "flake-nix-builders.yml@" in trusted,
            "native remote CI lost its protected push-only authority")
    require('      - "main"' in trusted and "workflow_dispatch:" not in trusted,
            "privileged workflow no longer has restricted push-only admission")
    require("  pull_request:" in pr and "id-token: write" not in pr
            and "flake-dag-staged.yml@" in pr,
            "single unprivileged PR full-Nix gate removed")
    require("branches: [main," not in canonical and "  pull_request:" not in canonical
            and "  workflow_dispatch:" in canonical,
            "legacy canonical full-Nix workflow duplicates main or PR CI")
    require("branches: [main," not in old_matrix and "  pull_request:" not in old_matrix
            and "  workflow_dispatch:" in old_matrix,
            "obsolete shard matrix duplicates the generated PR gate")
    require("branches: [main," not in diagnostic and "  pull_request:" not in diagnostic
            and "  workflow_dispatch:" in diagnostic,
            "manual-only diagnostic duplicated CI or gained PR authority")
    require("  pull_request:" in fmt and "branches: [main]" in fmt,
            "formatting no longer required on main and PR")
    for command in ("cargo fmt --all --check",
                    "cargo fmt --manifest-path examples/Cargo.toml --all --check",
                    "cargo fmt --manifest-path tests/acp-sdk-client/Cargo.toml"):
        require(command in fmt, "formatting omitted " + command)
    require("P1 official ACP SDK interop" in acp and "  pull_request:" in acp
            and "branches: [main]" in acp,
            "independent ACP gate removed")
    require("    paths:" not in fmt and "    paths:" not in acp,
            "required formatting or ACP check became path-filtered")
    require("  pull_request:" in source and "    paths:" not in source
            and "    paths-ignore:" not in source,
            "workflow regeneration check cannot run on all PRs")

def files():
    return [read(x) for x in (
        "phenix-flake-ci-generated.yml", "minimal-kernel-flake.yml",
        "minimal-kernel.yml", "minimal-kernel-matrix.yml",
        "minimal-kernel-dag.yml", "p1-rustfmt.yml",
        "p1-acp-sdk-interop.yml", "verify-generated-flake-ci.yml",
    )]

def self_test():
    good = files()
    verify(*good)
    mutations = [
        (0, '      - "main"', '      - "ci/unauthorized"'),
        (0, "  id-token: write", "  id-token: read"),
        (1, "  pull_request:", "  workflow_dispatch:"),
        (1, "flake-dag-staged.yml@", "flake-nix-builders.yml@"),
        (2, "  workflow_dispatch:", "  pull_request:\n  workflow_dispatch:"),
        (3, "  workflow_dispatch:", "  pull_request:\n  workflow_dispatch:"),
        (4, "  workflow_dispatch:", "  pull_request:\n  workflow_dispatch:"),
        (5, "cargo fmt --all --check", "echo no-format"),
        (6, "P1 official ACP SDK interop", "ACP omitted"),
        (7, "  pull_request:", "  pull_request:\n    paths:\n      - crates/**"),
    ]
    for index, old, new in mutations:
        require(old in good[index], "invalid adversarial fixture: " + old)
        altered = good.copy()
        altered[index] = altered[index].replace(old, new)
        try:
            verify(*altered)
        except AssertionError:
            pass
        else:
            raise AssertionError("authority mutation was admitted: " + old)

if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        self_test()
    elif not sys.argv[1:]:
        verify(*files())
        print("PASS: single-authority CI trust and independent required checks")
    else:
        raise SystemExit("usage: ci-authority-policy.py [--self-test]")
