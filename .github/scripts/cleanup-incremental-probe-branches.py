#!/usr/bin/env python3
"""One-shot cleanup for exact historical CI probes. No broad prefix deletion.

Dry validation is safe for untrusted pull requests. Deletion requires a
trusted, main-only push job and a GitHub-scoped token. All refs are preflighted
before any mutation; remote branch history remains in existing PR/run records.
"""
import argparse
import json
import os
import re
import urllib.error
import urllib.parse
import urllib.request

REPO = "matthis-k/phenix-ai"
EXPECTED = {
    "ci/verify-incremental-nix-agent-probe-20261019": "4cfcc130a72b52c17bb47a3687b5d5aa45566226",
    "ci/verify-incremental-nix-artifact-probe-20261019": "6d2bc107354c46409e4ec44ac4026be93248f680",
    "ci/verify-incremental-nix-config-probe-20261019": "2f42d4de97182a96453d23aa949d59567ec0c3af",
    "ci/verify-incremental-nix-fixture-probe-20261019": "e60de0b07cb7ffebea11bf82837c73a603623057",
    "ci/verify-incremental-nix-model-probe-20261019": "06ef6dbd771decd48970cfbf4d37320aab62b47d",
    "ci/verify-incremental-nix-tools-probe-20261019": "323e36592db314eb48fccd359ba8e828e5b3275a",
    "ci/verify-incremental-nix-warm-probe-20261019": "8f83836723596db23ff92d3a515ede2bb6c71077",
    "ci/verify-incremental-nix-probes-20261019": "fe98d124146f7443f088a95b4a41be0d6803d6ab",
    "ci/experiment-basic-composition-hotset-20261020": "e1b0e52afbf432bc7d521f692b9f6987a8b7c51c",
}


def validate():
    assert len(EXPECTED) == 9
    assert len(set(EXPECTED)) == len(EXPECTED)
    for name, sha in EXPECTED.items():
        assert name.startswith("ci/") and name.endswith("-20261019") or (
            name == "ci/experiment-basic-composition-hotset-20261020"
        ), name
        assert re.fullmatch(r"[0-9a-f]{40}", sha), name
        assert ".." not in name and "//" not in name and " " not in name


def request(method, url):
    token = os.environ["GITHUB_TOKEN"]
    req = urllib.request.Request(
        "https://api.github.com/repos/" + REPO + url,
        method=method,
        headers={
            "Authorization": "Bearer " + token,
            "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28",
            "User-Agent": "phenix-one-shot-ci-ref-cleanup",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=25) as response:
            data = response.read()
            return response.status, json.loads(data) if data else None
    except urllib.error.HTTPError as exc:
        if exc.code == 404:
            return 404, None
        raise


def execute():
    if (
        os.getenv("GITHUB_REPOSITORY") != REPO
        or os.getenv("GITHUB_EVENT_NAME") != "push"
        or os.getenv("GITHUB_REF") != "refs/heads/main"
    ):
        raise RuntimeError("branch cleanup requires trusted main-only push")
    all_prs = request("GET", "/pulls?state=open&per_page=100")[1]
    if len(all_prs) >= 100:
        raise RuntimeError("cannot establish full open-PR inventory")
    active_heads = {
        item["head"]["ref"] for item in all_prs
        if item["head"]["repo"] and item["head"]["repo"]["full_name"] == REPO
    }
    if active_heads.intersection(EXPECTED):
        raise RuntimeError("a historical probe is an active pull-request head")
    present = []
    # Complete read-only preflight before the first deletion.
    for name, expected in EXPECTED.items():
        path = "/git/ref/heads/" + urllib.parse.quote(name, safe="/")
        status, ref = request("GET", path)
        if status == 404:
            print("Already absent:", name)
            continue
        if ref["object"]["sha"] != expected:
            raise RuntimeError("probe branch moved; refusing deletion: " + name)
        present.append((name, path))
    for name, path in present:
        # Narrow the preflight/delete race: an agent may have resumed work
        # after our first inventory. This is not a general branch GC.
        status, ref = request("GET", path)
        if status == 404 or ref["object"]["sha"] != EXPECTED[name]:
            raise RuntimeError("historical ref moved before deletion: " + name)
        status, _ = request("DELETE", path)
        if status not in (200, 204):
            raise RuntimeError("unexpected deletion response for " + name)
        if request("GET", path)[0] != 404:
            raise RuntimeError("branch still exists after delete: " + name)
        print("Deleted and verified absent:", name)
    print("Exact historical probe cleanup complete:", len(present))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--execute", action="store_true")
    args = parser.parse_args()
    validate()
    if args.execute:
        execute()
    else:
        print("Validated", len(EXPECTED), "exact archived refs; no deletion performed")
