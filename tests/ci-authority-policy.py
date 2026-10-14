#!/usr/bin/env python3
"""Release-policy regression checks for single-authority post-P1 CI."""
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parent.parent


def require(ok, message):
    if not ok:
        raise AssertionError(message)


def verify(remote, canonical, matrix, dag, fmt, acp):
    # Only trusted main pushes and explicit trusted dispatch get remote OIDC.
    require('      - "main"' in remote, "trusted main push lacks complete remote CI")
    require("  pull_request:" not in remote, "remote OIDC granted to PRs")
    # After cutover, only the protected release branch receives automatic
    # privileged remote CI; historical staging branches must not retain OIDC.
    before = remote.split("on:\\n  push:\\n    branches:\\n", 1)
    require(len(before) == 2, "remote CI lacks a strict push allowlist")
    push = before[1].split("\\n  workflow_dispatch:", 1)[0]
    require(push.strip().splitlines() == ['- "main"'],
            "temporary CI staging branches still receive remote OIDC")

    require("  id-token: write" in remote, "remote OIDC missing")
    require("      worker_count: 4" in remote and "      slots_per_worker: 2" in remote,
            "remote pool changed")
    require("flake-nix-builders.yml@" in remote, "remote workflow is not pinned")

    # The old three Nix workflows remain manually runnable, but not automatic
    # second authorities for main. Keep one credential-free full PR matrix.
    require("branches: [main," not in canonical and "  pull_request:" not in canonical and
            "  workflow_dispatch:" in canonical, "legacy canonical full builds still automatic")
    require("branches: [main," not in matrix and "  pull_request:" in matrix and
            "  workflow_dispatch:" in matrix, "PR full-inventory matrix removed or duplicated")
    require("branches: [main," not in dag and "  pull_request:" not in dag and
            "  workflow_dispatch:" in dag, "legacy DAG still duplicates PR/main builds")
    require("id-token: write" not in matrix, "credential-free PR matrix gained OIDC write")

    # Formatting fails before expensive builds and checks external examples.
    require("  pull_request:" in fmt and "branches: [main]" in fmt,
            "cheap formatting not gated on main and PRs")
    require("cargo fmt --all --check" in fmt and
            "cargo fmt --manifest-path examples/Cargo.toml --all --check" in fmt and
            "cargo fmt --manifest-path tests/acp-sdk-client/Cargo.toml" in fmt,
            "format gate omits Rust workspace or external fixtures")
    require("P1 official ACP SDK interop" in acp and "branches: [main]" in acp and
            "  pull_request:" in acp, "independent official ACP SDK gate removed")


def files():
    def get(path):
        return (ROOT / ".github/workflows" / path).read_text()
    return [get(name) for name in (
        "phenix-flake-ci-generated.yml", "minimal-kernel.yml",
        "minimal-kernel-flake.yml", "minimal-kernel-dag.yml",
        "p1-rustfmt.yml", "p1-acp-sdk-interop.yml")]


def self_test():
    good = [
        'on:\n  push:\n    branches:\n      - "main"\npermissions:\n  id-token: write\n'
        '    uses: flake-nix-builders.yml@123\n      worker_count: 4\n      slots_per_worker: 2\n',
        "on:\n  workflow_dispatch:\n",
        "on:\n  pull_request:\n  workflow_dispatch:\npermissions:\n  contents: read\n",
        "on:\n  workflow_dispatch:\n",
        "on:\n  push:\n    branches: [main]\n  pull_request:\n"
        "cargo fmt --all --check\n"
        "cargo fmt --manifest-path examples/Cargo.toml --all --check\n"
        "cargo fmt --manifest-path tests/acp-sdk-client/Cargo.toml --all --check\n",
        "name: P1 official ACP SDK interop\non:\n  push:\n    branches: [main]\n  pull_request:\n",
    ]
    verify(*good)
    cases = [
        (0, '      - "main"', '      - "ci/sandbox"'),
        (0, '      - "main"', '      - "main"\\n      - "ci/temporary"'),
        (0, "on:\n  push:", "on:\n  pull_request:\n  push:"),
        (1, "  workflow_dispatch:", "  pull_request:\n  workflow_dispatch:"),
        (2, "permissions:\n  contents: read", "permissions:\n  id-token: write"),
        (3, "  workflow_dispatch:", "  pull_request:\n  workflow_dispatch:"),
        (4, "cargo fmt --manifest-path examples/Cargo.toml --all --check", ""),
        (5, "P1 official ACP SDK interop", "Unverified ACP client"),
    ]
    for idx, old, new in cases:
        candidate = good.copy()
        require(old in candidate[idx], "invalid adversarial fixture")
        candidate[idx] = candidate[idx].replace(old, new)
        try:
            verify(*candidate)
        except AssertionError:
            pass
        else:
            raise AssertionError("forged CI authority was accepted: " + old)


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        self_test()
    elif not sys.argv[1:]:
        verify(*files())
        print("P1 single-authority CI release policy passed")
    else:
        raise SystemExit("usage: ci-authority-policy.py [--self-test]")
