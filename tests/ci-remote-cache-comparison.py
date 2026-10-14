#!/usr/bin/env python3
"""Fail-closed comparator for a sequential same-head remote Nix benchmark."""
import copy
from datetime import datetime
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent
PIN = "1a733ae1daae2cb99989b735b171bc5dedd80d63"
REQUIRED = {"m1-shipping", "p1-release-preflight", "basic-host-smoke",
            "basic-pending-http-product", "m1-imports", "m1-public-sdk"}


def need(test, reason):
    if not test:
        raise ValueError(reason)


def policy():
    s = (ROOT / ".github/workflows/remote-cold-warm-benchmark.yml").read_text()
    need("  pull_request:" not in s, "untrusted benchmark trigger")
    need("    branches: [ci/m1-p1-remote-cold-warm]" in s, "missing trusted branch")
    need(s.count("flake-nix-builders.yml@" + PIN) == 2, "backend pin drift")
    need(s.count("library_ref: " + PIN) == 2, "library pin drift")
    need("needs: cold" in s, "warm must follow cold")
    need(s.count("worker_count: 4") == 2 and s.count("slots_per_worker: 2") == 2,
         "incorrect worker/slot pool")
    need(s.count("cache_namespace:") == 2 and "artifact_namespace: cold" in s and
         "artifact_namespace: warm" in s, "benchmark cache/artifact isolation absent")
    need("remote-nix-evidence-4-cold" in s and "remote-nix-evidence-4-warm" in s,
         "missing phase evidence")
    need("tests/ci-remote-cache-comparison.py --compare" in s and
         "actions/jobs/" in s, "benchmark results are not independently checked")


def compare(cold, warm, cold_log, warm_log, jobs):
    for phase, data in (("cold", cold), ("warm", warm)):
        evidence, timing = data
        checks = evidence.get("checks")
        need(evidence.get("mode") == "native-nix-dag" and
             re.fullmatch(r"[0-9a-f]{40}", evidence.get("commit", "")) and
             isinstance(checks, dict) and len(checks) >= 37 and REQUIRED <= set(checks),
             phase + ": invalid exact-head release inventory")
        need(all(v.get("status") == "pass" and v.get("drv") and v.get("outputs")
                 for v in checks.values()), phase + ": missing successful check output")
        need(all(evidence.get(k) for k in ("system", "planDigest", "graphDigest")),
             phase + ": missing plan or graph digest")
        need(timing.get("mode") == "remote-nix-ssh-ng" and
             timing.get("workerCount") == 4 and
             type(timing.get("wallSeconds")) in (int, float) and
             0 < timing["wallSeconds"] < 86400, phase + ": invalid Nix timing")
    for key in ("commit", "system", "planDigest", "graphDigest", "checks"):
        need(cold[0][key] == warm[0][key], "mismatched " + key)
    keys = [re.findall(r"primary-key: (remote-[a-zA-Z0-9_-]+)", log)
            for log in (cold_log, warm_log)]
    need(len(keys[0]) == len(keys[1]) == 1 and keys[0] == keys[1] and
         "-bench-" in keys[0][0], "cache keys are not isolated and identical")
    need("Could not find a cache." in cold_log and "Saved the new cache." in cold_log,
         "cold phase did not miss and save")
    need("Cache hit for: " + keys[0][0] in warm_log and
         "Cache restored successfully" in warm_log, "warm phase did not restore cold cache")
    entries = jobs.get("jobs")
    need(isinstance(entries, list) and jobs.get("total_count") == len(entries),
         "truncated GitHub job inventory")
    summary = {}
    for phase in ("cold", "warm"):
        selected = [j for j in entries if j.get("name", "").startswith(phase + " / ")]
        need(len(selected) == 7, phase + ": missing 4 workers or 3 coordinating jobs")
        periods = []
        for j in selected:
            need(j.get("status") == "completed" and j.get("conclusion") == "success",
                 phase + ": failed or unfinished job")
            first = datetime.fromisoformat(j["started_at"].replace("Z", "+00:00"))
            last = datetime.fromisoformat(j["completed_at"].replace("Z", "+00:00"))
            need(last > first, phase + ": invalid job time")
            periods.append((first, last))
        summary[phase] = (
            (max(t for _, t in periods) - min(t for t, _ in periods)).total_seconds(),
            sum((b-a).total_seconds() for a, b in periods))
    c, w = cold[1]["wallSeconds"], warm[1]["wallSeconds"]
    return "\n".join([
        "## Same-head four-builder Nix cache benchmark", "",
        "Commit: " + cold[0]["commit"] + "; checks: " + str(len(cold[0]["checks"])) + ".",
        "Both phases passed all checks; cold miss/save and exact warm restore verified.",
        "", "| Phase | Nix build | Pool wall | Runner-minutes |",
        "| --- | ---: | ---: | ---: |",
        f"| Cold | {c:.1f}s | {summary['cold'][0]/60:.2f}m | {summary['cold'][1]/60:.2f}m |",
        f"| Warm | {w:.1f}s | {summary['warm'][0]/60:.2f}m | {summary['warm'][1]/60:.2f}m |",
        "", f"Nix build ratio cold/warm: {c/w:.2f}x.", "",
    ])


def self_test():
    checks = {key: {"status": "pass", "drv": "drv", "outputs": ["store"]}
              for key in REQUIRED | {"case-" + str(i) for i in range(31)}}
    ev = {"mode": "native-nix-dag", "commit": "a"*40, "system": "x86_64-linux",
          "planDigest": "plan", "graphDigest": "graph", "checks": checks}
    ti = {"mode": "remote-nix-ssh-ng", "workerCount": 4, "wallSeconds": 50}
    logkey = "remote-phenix-m1-test-bench-123-1-" + "a"*40
    cl = "primary-key: " + logkey + "\nCould not find a cache.\nSaved the new cache."
    wl = "primary-key: " + logkey + "\nCache hit for: " + logkey + "\nCache restored successfully"
    jobs = {"total_count": 14, "jobs": [
        {"name": phase+" / job-"+str(i), "status": "completed", "conclusion": "success",
         "started_at": "2026-10-14T16:00:00Z", "completed_at": "2026-10-14T16:01:00Z"}
        for phase in ("cold", "warm") for i in range(7)]}
    compare((ev, ti), (ev, ti), cl, wl, jobs)
    for kind in ("commit", "shipping", "worker", "jobs", "cache"):
        e, t, j = copy.deepcopy(ev), copy.deepcopy(ti), copy.deepcopy(jobs)
        altered = wl
        if kind == "commit": e["commit"] = "b"*40
        if kind == "shipping": e["checks"].pop("m1-shipping")
        if kind == "worker": t["workerCount"] = 8
        if kind == "jobs": j["total_count"] = 13
        if kind == "cache": altered = wl.replace("Cache hit for: "+logkey, "Cache hit for: wrong")
        try:
            compare((ev, ti), (e, t), cl, altered, j)
        except ValueError:
            continue
        raise AssertionError("forged " + kind + " passed")


if __name__ == "__main__":
    try:
        if sys.argv[1:] == ["--self-test"]:
            self_test()
        elif sys.argv[1:] == ["--policy"]:
            policy()
        elif len(sys.argv) == 8 and sys.argv[1] == "--compare":
            cdir, wdir, clog, wlog, jobsfile, out = sys.argv[2:]
            def load(directory):
                p = Path(directory)
                return (json.loads((p/"evidence.json").read_text()),
                        json.loads((p/"timing.json").read_text()))
            report = compare(load(cdir), load(wdir), Path(clog).read_text(),
                             Path(wlog).read_text(), json.loads(Path(jobsfile).read_text()))
            Path(out).write_text(report)
            print(report)
        else:
            raise ValueError("usage: --self-test | --policy | --compare COLD WARM COLDLOG WARMLOG JOBS OUTPUT")
    except (ValueError, OSError, KeyError, TypeError) as error:
        raise SystemExit(str(error)) from error
