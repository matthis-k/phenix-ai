#!/usr/bin/env python3
"""Exact-head, cold-project-cache/warm-project-cache Nix DAG benchmark evidence.

The runner uses the pinned phenix-flake-ci planner/DAG. This module only
measures results and verifies cache retention; it never schedules builds.
"""
import argparse
from datetime import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

SCHEMA = 1
STORE = re.compile(r"/nix/store/[a-z0-9]{32}-[^\s]+\Z")
COMMIT = re.compile(r"[0-9a-f]{40}\Z")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def read(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def save(path, data):
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def sha256(value):
    return hashlib.sha256(
        json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()


def paths_from_evidence(plan, graph, evidence):
    require(plan.get("schemaVersion") == 1 and graph.get("schemaVersion") == 1,
            "unsupported plan or DAG schema")
    require(plan.get("commit") == graph.get("commit") == evidence.get("commit"),
            "different source commits")
    require(plan.get("digest") == graph.get("planDigest") == evidence.get("planDigest"),
            "inconsistent plan digest")
    require(graph.get("graphDigest") == evidence.get("graphDigest"), "inconsistent DAG digest")
    require(evidence.get("mode") == "native-nix-dag", "not a native DAG result")
    checks = plan.get("checks")
    realized = evidence.get("checks")
    require(isinstance(checks, dict) and checks and isinstance(realized, dict)
            and set(checks) == set(realized), "incomplete check inventory")
    outputs = set()
    for name, drv in checks.items():
        entry = realized[name]
        require(entry.get("status") == "pass" and entry.get("drv") == drv,
                "check failed or derivation identity changed")
        values = entry.get("outputs")
        require(isinstance(values, list) and values, "missing check output")
        for path in values:
            require(isinstance(path, str) and STORE.fullmatch(path) is not None,
                    "invalid Nix store output")
            outputs.add(path)
    return sorted(outputs)


def store_closure(outputs):
    raw = subprocess.check_output(
        ["nix", "path-info", "--offline", "--recursive", *outputs], text=True
    )
    paths = sorted(set(raw.splitlines()))
    require(paths and set(outputs).issubset(paths)
            and all(STORE.fullmatch(item) for item in paths),
            "invalid or incomplete locally realized Nix closure")
    return paths


def capture(phase, directory, library_ref):
    require(phase in ("cold", "warm"), "invalid benchmark phase")
    require(COMMIT.fullmatch(library_ref) is not None, "un-pinned CI library")
    folder = Path(directory)
    plan, graph, evidence = (read(folder / filename)
                            for filename in ("plan.json", "graph.json", "evidence.json"))
    outputs = paths_from_evidence(plan, graph, evidence)
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    require(head == plan["commit"], "not the checked-out exact head")
    log = (folder / "build.log").read_bytes()
    reports = read(folder / "build-events.json")
    require(isinstance(reports.get("build_events"), int)
            and isinstance(reports.get("distinct_derivation_paths"), int)
            and isinstance(reports.get("repeated_build_events"), int),
            "missing derivation build-event evidence")
    require(reports.get("shards", {}).keys() == {"00"}, "expected exactly one local Nix store")
    seconds = float((folder / "build-seconds.txt").read_text().strip())
    require(seconds >= 0, "invalid build duration")
    closure = store_closure(outputs)
    cache_key = os.getenv("BENCH_CACHE_KEY", "")
    require(cache_key and re.fullmatch(r"[A-Za-z0-9_-]+", cache_key),
            "missing cache identity")
    payload = {
        "schemaVersion": SCHEMA, "phase": phase, "commit": head,
        "libraryRef": library_ref, "cacheKey": cache_key,
        "planDigest": plan["digest"], "graphDigest": graph["graphDigest"],
        "checkNames": sorted(plan["checks"]),
        "runnerImage": os.getenv("ImageOS", os.getenv("RUNNER_OS", "")),
        "runnerVersion": os.getenv("ImageVersion", ""),
        "nixVersion": subprocess.check_output(["nix", "--version"], text=True).strip(),
        "buildSeconds": seconds, "buildEvents": reports["build_events"],
        "uniqueBuiltDrvs": reports["distinct_derivation_paths"],
        "repeatedBuildEvents": reports["repeated_build_events"],
        "outputCount": len(outputs), "outputIdentitySha256": sha256(outputs),
        "closurePathCount": len(closure), "closureSha256": sha256(closure),
        "buildLogSha256": hashlib.sha256(log).hexdigest(),
    }
    save(folder / "metrics.json", payload)
    save(folder / "closure.json", {
        "schemaVersion": SCHEMA, "commit": head, "libraryRef": library_ref,
        "cacheKey": cache_key, "planDigest": plan["digest"],
        "outputs": outputs, "paths": closure,
    })
    print(json.dumps({"phase": phase, "buildSeconds": seconds,
                      "buildEvents": payload["buildEvents"], "closurePaths": len(closure)}))


def missing_store_paths(paths):
    """Bisect only failed offline Nix path-info batches; never build/restore."""
    missing = []

    def check(batch):
        proc = subprocess.run(
            ["nix", "path-info", "--offline", *batch],
            text=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
            check=False,
        )
        if proc.returncode == 0:
            return
        if len(batch) == 1:
            missing.append(batch[0])
        else:
            half = len(batch) // 2
            check(batch[:half])
            check(batch[half:])

    for i in range(0, len(paths), 128):
        check(paths[i:i + 128])
    return missing


def retention(cold_closure, output):
    cold = read(cold_closure)
    paths = cold.get("paths")
    roots = cold.get("outputs")
    require(cold.get("schemaVersion") == SCHEMA and isinstance(paths, list)
            and paths and paths == sorted(set(paths))
            and isinstance(roots, list) and set(roots).issubset(paths)
            and all(isinstance(p, str) and STORE.fullmatch(p) for p in paths),
            "invalid baseline closure")
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    require(head == cold.get("commit"), "retention comparison changed source commit")
    require(os.getenv("BENCH_CACHE_KEY") == cold.get("cacheKey"),
            "retention comparison changed cache key")
    missing = missing_store_paths(paths)
    result = {
        "schemaVersion": SCHEMA, "commit": head, "libraryRef": cold["libraryRef"],
        "cacheKey": cold["cacheKey"],
        "closurePaths": len(paths), "retainedPaths": len(paths) - len(missing),
        "missingCount": len(missing), "missingSamples": missing[:20],
        "retentionPercent": round(100 * (len(paths) - len(missing)) / len(paths), 2),
    }
    save(output, result)
    print(json.dumps(result, sort_keys=True))
    # Do not fail yet: the warm build still measures fallback rebuilding.


def job_seconds(job):
    def parse(value):
        require(isinstance(value, str), "missing GitHub job timestamp")
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    start, end = parse(job.get("started_at")), parse(job.get("completed_at"))
    elapsed = (end - start).total_seconds()
    require(job.get("conclusion") == "success" and elapsed >= 0,
            "GitHub benchmark job did not finish successfully")
    return elapsed, start, end


def compare(cold_dir, warm_dir, jobs_file, report_json, report_md):
    cold = read(Path(cold_dir) / "metrics.json")
    warm = read(Path(warm_dir) / "metrics.json")
    retained = read(Path(warm_dir) / "retention.json")
    require(cold.get("schemaVersion") == warm.get("schemaVersion") == SCHEMA
            and cold.get("phase") == "cold" and warm.get("phase") == "warm",
            "invalid benchmark metrics")
    fields = ("commit", "libraryRef", "cacheKey", "planDigest", "graphDigest",
              "checkNames", "runnerImage", "runnerVersion", "nixVersion",
              "outputCount", "outputIdentitySha256",
              "closurePathCount", "closureSha256")
    for field in fields:
        require(cold.get(field) == warm.get(field),
                f"cold/warm mismatch: {field}")
    require(retained.get("commit") == cold["commit"]
            and retained.get("cacheKey") == cold["cacheKey"]
            and retained.get("closurePaths") == cold["closurePathCount"],
            "cache retention evidence is stale")
    jobs = read(jobs_file).get("jobs")
    require(isinstance(jobs, list), "missing GitHub Actions job inventory")
    phases = {}
    for phase in ("cold", "warm"):
        selected = [j for j in jobs if j.get("name") == phase]
        require(len(selected) == 1, f"expected one {phase} GitHub job")
        phases[phase] = job_seconds(selected[0])
    require(phases["cold"][2] <= phases["warm"][1], "warm job started before cold cache save")
    critical = (phases["warm"][2] - phases["cold"][1]).total_seconds()
    report = {
        "schemaVersion": SCHEMA, "commit": cold["commit"],
        "libraryRef": cold["libraryRef"], "cacheKey": cold["cacheKey"],
        "checkCount": len(cold["checkNames"]),
        "cold": {"buildSeconds": cold["buildSeconds"],
                 "jobSeconds": phases["cold"][0], "buildEvents": cold["buildEvents"],
                 "uniqueBuiltDrvs": cold["uniqueBuiltDrvs"],
                 "repeatedBuildEvents": cold["repeatedBuildEvents"]},
        "warm": {"buildSeconds": warm["buildSeconds"],
                 "jobSeconds": phases["warm"][0], "buildEvents": warm["buildEvents"],
                 "uniqueBuiltDrvs": warm["uniqueBuiltDrvs"],
                 "repeatedBuildEvents": warm["repeatedBuildEvents"]},
        "cacheRetention": retained,
        "criticalPathSecondsColdThroughWarm": critical,
        "buildSpeedup": round(cold["buildSeconds"] / max(0.001, warm["buildSeconds"]), 3),
        "savedBuildEvents": cold["buildEvents"] - warm["buildEvents"],
    }
    save(report_json, report)
    table = [
        "## Exact-head Phenix CI cold/warm benchmark",
        f"Commit: \`{cold['commit']}\` · CI library: \`{cold['libraryRef']}\`",
        f"Check inventory: **{len(cold['checkNames'])}** · same Nix DAG, runner and outputs",
        "",
        "| Metric | Cold project cache | Warm project cache |",
        "| --- | ---: | ---: |",
        f"| Nix DAG build | {cold['buildSeconds']:.1f}s | {warm['buildSeconds']:.1f}s |",
        f"| Full GitHub job | {phases['cold'][0]:.1f}s | {phases['warm'][0]:.1f}s |",
        f"| Direct Nix build events | {cold['buildEvents']} | {warm['buildEvents']} |",
        f"| Repeated same-derivation events | {cold['repeatedBuildEvents']} | {warm['repeatedBuildEvents']} |",
        "",
        f"**Cache retention before warm build:** {retained['retainedPaths']}/{retained['closurePaths']} "
        f"paths ({retained['retentionPercent']}%).",
        f"**Sequential cold → warm critical path:** {critical:.1f}s (including runner startup and handoff).",
        f"**DAG build speedup:** {report['buildSpeedup']}×.",
        "",
        "Cold means no prior *Phenix Actions cache*; public binary substituters may still provide dependencies. "
        "Build events count exact Nix derivation builds, not semantically equivalent Cargo compilation units.",
    ]
    if retained["missingCount"]:
        table.append(f"**Cache retention FAIL:** {retained['missingCount']} missing store paths.")
    Path(report_md).parent.mkdir(parents=True, exist_ok=True)
    Path(report_md).write_text("\n".join(table) + "\n", encoding="utf-8")
    require(retained["missingCount"] == 0, "warm cache failed to retain complete cold closure")
    print(json.dumps(report, indent=2, sort_keys=True))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    a = sub.add_parser("capture")
    a.add_argument("--phase", choices=["cold", "warm"], required=True)
    a.add_argument("--directory", required=True)
    a.add_argument("--library-ref", required=True)
    a = sub.add_parser("retention")
    a.add_argument("--cold-closure", required=True)
    a.add_argument("--output", required=True)
    a = sub.add_parser("compare")
    a.add_argument("--cold", required=True)
    a.add_argument("--warm", required=True)
    a.add_argument("--jobs", required=True)
    a.add_argument("--json", required=True)
    a.add_argument("--markdown", required=True)
    args = parser.parse_args()
    try:
        if args.action == "capture":
            capture(args.phase, args.directory, args.library_ref)
        elif args.action == "retention":
            retention(args.cold_closure, args.output)
        else:
            compare(args.cold, args.warm, args.jobs, args.json, args.markdown)
    except (ValueError, OSError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        print(f"benchmark: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
