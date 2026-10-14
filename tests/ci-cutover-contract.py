"""Tests-first contract for the post-P1 Phenix CI cutover.

Keep exactly one expensive Nix checks pipeline for each event: the staged
DAG on PRs, native Nix four remote builders on trusted main pushes.
The official ACP SDK remains independent. No arbitrary fork can receive
tailnet builder access. The obsolete workflows stay manual for fallback.
"""
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WORKFLOWS = ROOT / ".github" / "workflows"


def read(name):
    return (WORKFLOWS / name).read_text()


def events(source):
    return source.split("\non:\n", 1)[1].split("\npermissions:", 1)[0]


def immutable_pin(source, filename):
    match = re.search(
        r"uses: matthis-k/phenix-flake-ci/\.github/workflows/"
        + re.escape(filename) + r"@([0-9a-f]{40})", source,
    )
    assert match, "Missing pinned reusable workflow: " + filename
    sha = match.group(1)
    assert re.search(r"library_ref:\s*[\"']?" + sha + r"\b", source), (
        "Reusable code pin and workflow pin differ: " + filename
    )
    return sha


def verify():
    legacy = read("minimal-kernel.yml")
    matrix = read("minimal-kernel-flake.yml")
    dag = read("minimal-kernel-dag.yml")
    remote = read("phenix-flake-ci-generated.yml")
    acp = read("p1-acp-sdk-interop.yml")
    policy = (ROOT / "ci" / "flake-check-config.nix").read_text()

    # Obsolete full-build pipelines remain available for explicit diagnostic
    # dispatch only. They must not duplicate production/PR Nix builds.
    for name, text in [("legacy canonical", legacy), ("old matrix", matrix)]:
        event = events(text)
        assert "pull_request:" not in event, name + " still runs full PR builds"
        assert not re.search(r"(?m)^\s+[- ]+main\b", event), (
            name + " still runs full builds on main"
        )
        assert "workflow_dispatch:" in event, name + " has no manual fallback"

    # Normal fork/PR execution never asks for Tailscale secrets. One reusable
    # DAG owns all evaluated flake checks, plus cheap independent formatting.
    dag_events = events(dag)
    assert "pull_request:" in dag_events, "PRs lack required Nix coverage"
    assert "main" in dag_events, "main needs fast formatting verification"
    assert "cargo fmt --all --check" in dag, "workspace formatting was lost"
    assert "cargo fmt --manifest-path examples/Cargo.toml --all --check" in dag, (
        "external SDK/example formatting was lost"
    )
    assert re.search(r"(?m)^  quality:\s*$", dag), "fast quality job missing"
    assert "github.event_name == 'pull_request'" in dag, (
        "DAG must not run expensive checks on trusted main pushes"
    )
    assert "id-token: write" not in dag, "PR jobs must not gain OIDC privilege"
    assert "TS_OAUTH" not in dag and "TS_AUTHKEY" not in dag, (
        "PR DAG must not see Tailscale credentials"
    )
    dag_pin = immutable_pin(dag, "flake-dag-staged.yml")

    # Main is trusted, so the same check inventory may be built by one Nix
    # coordinator over four ephemeral Tailscale SSH workers.
    remote_events = events(remote)
    assert "pull_request:" not in remote_events, (
        "untrusted PR code could reach Tailscale remote builders"
    )
    assert "push:" in remote_events and "main" in remote_events, (
        "main does not run authoritative remote Nix shipping"
    )
    assert "id-token: write" in remote, "OIDC federation disabled"
    assert "worker_count: 4" in remote, "benchmarked default is 4 workers"
    assert "slots_per_worker: 2" in remote, "use measured 2 slots per worker"
    assert "TS_OAUTH_CLIENT_ID:" in remote and "TS_AUDIENCE:" in remote, (
        "OIDC credentials must be passed via Actions secrets/variables"
    )
    assert "id-token: write" not in matrix, "old matrix should stay unprivileged"
    remote_pin = immutable_pin(remote, "flake-nix-builders.yml")
    assert dag_pin == remote_pin, "CI planners use inconsistent library revisions"

    # The consumer policy and generated caller may not silently diverge.
    assert 'backend = "nix-remote";' in policy, "remote policy missing"
    assert 'credentials = "oidc";' in policy, "credential policy missing"

    # Protocol verification must still exercise PRs and the resulting main.
    acp_events = events(acp)
    assert "pull_request:" in acp_events and "main" in acp_events, (
        "official ACP SDK verification lost release/PR coverage"
    )
    assert "Exercise host through official stable-v1 SDK" in acp

    return {"backend": "nix-remote", "remote_workers": 4, "slots": 2,
            "library_ref": remote_pin}


if __name__ == "__main__":
    print("CI cutover contract:", verify())
