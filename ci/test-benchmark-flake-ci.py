#!/usr/bin/env python3
"""Pure benchmark tests: strict identity, cache retention, and critical path."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location("bench", Path(__file__).with_name("benchmark-flake-ci.py"))
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)

COMMIT = "a" * 40
LIBRARY = "b" * 40
A = "/nix/store/" + "a" * 32 + "-root"
B = "/nix/store/" + "b" * 32 + "-dependency"


def metrics(phase):
    return {"schemaVersion": 1, "phase": phase,
            "commit": COMMIT, "libraryRef": LIBRARY, "cacheKey": "bench-1",
            "planDigest": "plan", "graphDigest": "graph",
            "checkNames": ["a", "b"], "runnerImage": "ubuntu24",
            "runnerVersion": "1", "nixVersion": "nix 2.33",
            "buildSeconds": 22 if phase == "cold" else 2,
            "buildEvents": 12 if phase == "cold" else 0,
            "uniqueBuiltDrvs": 11 if phase == "cold" else 0,
            "repeatedBuildEvents": 1 if phase == "cold" else 0,
            "outputCount": 1, "outputIdentitySha256": "same",
            "closurePathCount": 2, "closureSha256": "closure"}


def jobs():
    return {"jobs": [
        {"name": "cold", "conclusion": "success",
         "started_at": "2026-11-08T11:00:00Z",
         "completed_at": "2026-11-08T11:01:00Z"},
        {"name": "warm", "conclusion": "success",
         "started_at": "2026-11-08T11:01:05Z",
         "completed_at": "2026-11-08T11:01:20Z"}]}


class BenchmarkTests(unittest.TestCase):
    def test_exact_evidence_and_output_identities(self):
        plan = {"schemaVersion": 1, "commit": COMMIT, "digest": "plan",
                "checks": {"one": A + ".drv"}}
        graph = {"schemaVersion": 1, "commit": COMMIT,
                 "planDigest": "plan", "graphDigest": "graph"}
        evidence = {"commit": COMMIT, "planDigest": "plan", "graphDigest": "graph",
                    "mode": "native-nix-dag", "checks": {
                        "one": {"drv": A + ".drv", "status": "pass", "outputs": [A]}}}
        self.assertEqual(bench.paths_from_evidence(plan, graph, evidence), [A])
        with self.assertRaisesRegex(ValueError, "derivation"):
            bench.paths_from_evidence(plan, graph, {
                **evidence, "checks": {"one": {**evidence["checks"]["one"],
                                              "drv": B + ".drv"}}})
        with self.assertRaisesRegex(ValueError, "incomplete"):
            bench.paths_from_evidence(plan, graph, {**evidence, "checks": {}})

    def test_bisects_partial_cache_retention_without_building(self):
        seen = []
        def run(args, **kwargs):
            seen.append(args)
            return type("P", (), {"returncode": 1 if B in args else 0})()
        with mock.patch.object(bench.subprocess, "run", side_effect=run):
            self.assertEqual(bench.missing_store_paths([A, B]), [B])
        self.assertTrue(all(args[:3] == ["nix", "path-info", "--offline"] for args in seen))

    def test_compare_requires_identical_inventory_and_complete_cache(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for phase in ("cold", "warm"):
                bench.save(root / phase / "metrics.json", metrics(phase))
            bench.save(root / "warm" / "retention.json", {
                "schemaVersion": 1, "commit": COMMIT, "cacheKey": "bench-1",
                "closurePaths": 2, "retainedPaths": 2, "missingCount": 0,
                "retentionPercent": 100})
            bench.save(root / "jobs.json", jobs())
            args = (root / "cold", root / "warm", root / "jobs.json",
                    root / "report.json", root / "report.md")
            bench.compare(*args)
            report = bench.read(root / "report.json")
            self.assertEqual(report["criticalPathSecondsColdThroughWarm"], 80)
            self.assertEqual(report["savedBuildEvents"], 12)
            self.assertEqual(report["buildSpeedup"], 11)
            self.assertIn("100%", (root / "report.md").read_text())
            changed = metrics("warm")
            changed["graphDigest"] = "wrong"
            bench.save(root / "warm" / "metrics.json", changed)
            with self.assertRaisesRegex(ValueError, "graphDigest"):
                bench.compare(*args)
            bench.save(root / "warm" / "metrics.json", metrics("warm"))
            retention = bench.read(root / "warm" / "retention.json")
            retention["missingCount"] = 1
            bench.save(root / "warm" / "retention.json", retention)
            with self.assertRaisesRegex(ValueError, "cache failed"):
                bench.compare(*args)


if __name__ == "__main__":
    unittest.main()
