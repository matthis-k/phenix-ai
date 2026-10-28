"""Fail-closed guard for privileged native Nix CI on owned push branches."""
from pathlib import Path
import sys

WORKFLOW = Path(__file__).resolve().parents[1] / ".github/workflows/trusted-native-nix.yml"
PIN = "c4467d7dc233ada85ea49823561ee8be57089591"

def require(ok, message):
    if not ok:
        raise AssertionError(message)

def verify(text):
    # This independent workflow extends trusted push coverage; it does not
    # weaken the generated main-only workflow or the read-only PR workflow.
    require("on:\n  push:\n    branches:\n      - \"ci/**\"\n      - \"review/**\"\n  workflow_dispatch:" in text,
            "must restrict privileged trigger to owned branch families")
    require("  pull_request:" not in text and "  pull_request_target:" not in text,
            "untrusted PR event admitted")
    require("  workflow_run:" not in text, "untrusted workflow artifact admitted")
    require("permissions:\n  contents: read\n  id-token: write" in text,
            "missing narrow OAuth permission policy")
    require('test "$ACTUAL_REPO" = "matthis-k/phenix-ai"' in text,
            "missing exact repository authorization")
    require('case "$ACTUAL_EVENT" in push|workflow_dispatch)' in text,
            "untrusted event bypasses authorization")
    require("refs/heads/ci/*|refs/heads/review/*" in text,
            "missing branch-ref authorization")
    require("    needs: authorize\n    if: needs.authorize.result == 'success'\n" in text,
            "privileged worker bypasses authorization job")
    require(text.count("flake-nix-builders.yml@" + PIN) == 1,
            "distributed Nix caller is not pinned to trusted library")
    require(text.count("library_ref: " + PIN) == 1,
            "library checkout pin does not match callable")
    require("      worker_count: 4\n      slots_per_worker: 2" in text,
            "four-worker/two-slot native scheduler lost")
    require("      config: ci/flake-check-config.nix" in text,
            "not running complete canonical check inventory")
    require("TS_OAUTH_CLIENT_ID: ${{ secrets.TS_OAUTH_CLIENT_ID || vars.TS_OAUTH_CLIENT_ID }}" in text,
            "trusted Tailscale federation missing")
    require("TS_AUDIENCE: ${{ format('api.tailscale.com/{0}', secrets.TS_OAUTH_CLIENT_ID || vars.TS_OAUTH_CLIENT_ID) }}" in text,
            "OAuth audience differs from authorized client ID")
    require("TS_AUTHKEY:" not in text and "secrets: inherit" not in text,
            "mixed broad secret delegation")
    require("actions/checkout@" not in text, "unreviewed checkout in privileged caller")
    require("    runs-on: ubuntu-24.04\n    permissions:\n      contents: read" in text,
            "authorization job must have no OIDC permission")

def self_test():
    good = WORKFLOW.read_text()
    verify(good)
    mutations = [
        ('on:\n  push:', 'on:\n  pull_request:\n  push:'),
        ('      - "review/**"', '      - "*"'),
        ("refs/heads/ci/*|refs/heads/review/*", "refs/heads/*"),
        ('test "$ACTUAL_REPO" = "matthis-k/phenix-ai"', 'true'),
        ("    needs: authorize\n", ""),
        ("    if: needs.authorize.result == 'success'\n", ""),
        ("  id-token: write", "  id-token: read"),
        ("flake-nix-builders.yml@" + PIN, "flake-nix-builders.yml@" + "f"*40),
        ("library_ref: " + PIN, "library_ref: " + "f"*40),
        ("      worker_count: 4", "      worker_count: 8"),
        ("      config: ci/flake-check-config.nix", "      config: ci/reduced-checks.nix"),
        ("    secrets:\n", "    secrets: inherit\n"),
        ("  workflow_dispatch:", "  workflow_run:"),
    ]
    for before, after in mutations:
        require(before in good, "bad adversarial fixture: " + before)
        try:
            verify(good.replace(before, after))
        except AssertionError:
            pass
        else:
            raise AssertionError("unsafe privileged workflow mutation accepted: " + before)

if __name__ == "__main__":
    verify(WORKFLOW.read_text())
    if sys.argv[1:] == ["--self-test"]:
        self_test()
    elif sys.argv[1:]:
        raise SystemExit("unknown test arguments")
    print("PASS: native 4-builder Tailscale admission is restricted to trusted pushes")
