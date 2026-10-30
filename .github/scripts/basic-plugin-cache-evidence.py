#!/usr/bin/env python3
"""Evidence-producing extension of the Basic plugin Nix cache-isolation test.

Nix remains the authority for package/check inventory and inputDrvs.
Mutations live only in detached disposable Git worktrees, never in checkout.
A derivation comparison is NOT classified as a binary cache hit.
"""
import argparse
import datetime as dt
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import time

SYSTEM = "x86_64-linux"
# Small regression assertions, NOT a parallel Nix dependency graph.
BASIC = (
    "phenix-plugin-basic-tools", "phenix-plugin-basic-agent",
    "phenix-plugin-model-scripted", "phenix-plugin-event-journal",
    "phenix-plugin-basic-session", "phenix-plugin-basic-memory",
    "phenix-plugin-storage-sqlite",
)
COMPOSITION = "phenix-basic-composition"
MODEL = "phenix-plugin-model-scripted"
TOOLS = "phenix-plugin-basic-tools"
AGENT = "phenix-plugin-basic-agent"
CASES = {
    "A": ("unchanged", None),
    "B": ("tools-source", "crates/phenix-plugin-basic-tools/src/lib.rs"),
    "C": ("agent-source", "crates/phenix-plugin-basic-agent/src/lib.rs"),
    "D": ("scripted-model-source", "crates/phenix-plugin-model-scripted/src/lib.rs"),
    "E": ("shared-basic-contracts", "crates/phenix-basic-contracts/src/lib.rs"),
    "F": ("shared-sdk", "crates/phenix-sdk/src/lib.rs"),
    "G": ("composition-configuration", "flake.nix"),
    "H": ("composition-add-artifact", "flake.nix"),
    "I": ("nix-cargo-feature-variant", "flake.nix"),
    "J": ("fixture-provider-source", "examples/provider/src/lib.rs"),
}
# Compositions and shared consumers are discovered from the actual drv graph.
GUARDS = {
    "B": {"changed": [TOOLS, COMPOSITION], "stable": [p for p in BASIC if p != TOOLS]},
    "C": {"changed": [AGENT, COMPOSITION],
          "stable": [p for p in BASIC if p != AGENT]},
    "D": {"changed": [MODEL, COMPOSITION], "stable": [p for p in BASIC if p != MODEL]},
    "E": {"changed": [TOOLS, AGENT, MODEL], "stable": ["phenix-kernel"]},
    "F": {"changed": [TOOLS, AGENT], "stable": ["phenix-kernel"]},
    "G": {"changed": [COMPOSITION], "stable": list(BASIC) + ["phenix-kernel"]},
    "H": {"changed": ["phenix-basic-verification-composition"],
          "stable": list(BASIC) + [COMPOSITION]},
    "I": {"changed": [], "stable": list(BASIC) + [COMPOSITION]},
    "J": {"changed": ["phenix-core-example-plugins"], "stable": list(BASIC) + [COMPOSITION]},
}
# C and I assert the staged feature variant using checks.x86_64-linux.
# Targets measure selected products; derivation dependencies still come from Nix.
BUILD_TARGETS = {
    "B": [TOOLS, COMPOSITION],
    "C": [AGENT, COMPOSITION],
    "D": [MODEL, COMPOSITION],
    # Test representative real consumers of the changed shared contracts/SDK.
    # This is deliberately narrower than the complete reverse dependency graph.
    "E": [TOOLS, AGENT, MODEL, COMPOSITION],
    "F": [TOOLS, AGENT, MODEL, COMPOSITION],
    "G": [COMPOSITION],
    "H": ["phenix-basic-verification-composition"],
    "J": ["phenix-core-example-plugins"],
}
# Feature variant I changes a check derivation, not a package derivation.
BUILD_CHECK_TARGETS = {"I": ["p2-basic-staged-plugin"]}
# H adds an unchanged plugin to a composition; pre-realize it so the
# post-edit build cannot misclassify its first cold build as invalidation.
BASELINE_EXTRA_TARGETS = {"H": ["phenix-plugin-model-http"]}
WARM_CONTROL = [TOOLS, AGENT, MODEL, COMPOSITION]


def call(argv, cwd, timeout=900, check=True, stdin=None):
    started = time.monotonic()
    proc = subprocess.run(argv, cwd=cwd, input=stdin, text=True,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          timeout=timeout, check=False)
    record = {"argv": argv, "seconds": round(time.monotonic() - started, 3),
              "exit_code": proc.returncode}
    if check and proc.returncode:
        raise RuntimeError("%s failed (%s): %s" %
                           (" ".join(argv), proc.returncode, proc.stderr[-5000:]))
    return proc, record


def inventory(root, kind):
    expr = "p: builtins.mapAttrs (_: v: v.drvPath) p"
    p, timing = call(["nix", "eval", "--json", "--apply", expr,
                      "%s#%s.%s" % (root, kind, SYSTEM)], root)
    return json.loads(p.stdout), timing


def dependencies(root, paths):
    # Nix emits actual inputDrvs, never a guessed crate -> plugin mapping.
    p, timing = call(["nix", "derivation", "show", "--recursive", "--stdin"],
                     root, timeout=900, stdin="\n".join(paths) + "\n")
    drvs = json.loads(p.stdout)
    if isinstance(drvs, dict) and isinstance(drvs.get("derivations"), dict):
        drvs = drvs["derivations"]
    graph = {}
    for path, data in drvs.items():
        if not path.endswith(".drv") or not isinstance(data, dict):
            continue
        # Nix >=2.33 uses derivation JSON v4: inputs.drvs (v3: inputDrvs).
        inputs = data.get("inputs", {}).get("drvs") if "inputs" in data else data.get("inputDrvs")
        if not isinstance(inputs, dict):
            raise RuntimeError("unknown Nix derivation input shape: " + path)
        graph[os.path.basename(path)] = sorted(os.path.basename(dep) for dep in inputs)
    required = {os.path.basename(path) for path in paths}
    if not graph or not required.issubset(graph):
        raise RuntimeError("Nix graph incomplete: missing %s" % sorted(required - graph.keys()))
    if not any(graph.values()):
        raise RuntimeError("Nix graph unexpectedly contains no derivation edges")
    return graph, timing


def mutate(root, case):
    label, rel = CASES[case]
    if not rel:
        return []
    path = root / rel
    before = path.read_text()
    if case in ("B", "C", "D", "E", "F", "J"):
        after = before + "\n// Phenix temporary CI derivation probe: %s.\n" % case
    elif case == "G":
        needle = "grants = basicGrants;"
        alternate = "grants = pkgs.lib.reverseList basicGrants;"
        assert before.count(needle) + before.count(alternate) == 1, "composition grant declaration drift"
        after = (before.replace(needle, alternate, 1) if needle in before
                 else before.replace(alternate, needle, 1))
    elif case == "H":
        # Test-domain selection moved out of the inline flake declaration.
        # Change only the selected history-test mix, not the product preset.
        needle = ('historyVerificationCompositionSpec = agentMixes.testing.modelHistory {\n'
                  '        base = basicCompositionSpec;\n'
                  '        artifact = "${historyTestModel}";')
        assert before.count(needle) == 1, "verification test-mix declaration drift"
        after = before.replace(
            needle, needle + '\n        extraArtifacts = [ "${httpModel}" ];', 1)
        # This adds an existing already-built optional plugin, without modifying
        # any Rust source or the selected verification Model slot.
    elif case == "I":
        needle = 'cargoBuildFlags = [ "-p" "phenix-plugin-basic-agent" "--features" "staged-graph" ];'
        assert before.count(needle) == 1, "staged feature declaration drift"
        after = before.replace(
            needle,
            'cargoBuildFlags = [ "-p" "phenix-plugin-basic-agent" "--no-default-features" "--features" "staged-graph" ];', 1)
    else:
        raise AssertionError(case)
    if after == before:
        raise AssertionError("probe did not modify input")
    path.write_text(after)
    return [rel]


def is_dependent(consumer, provider, graph):
    seen = set()
    todo = [os.path.basename(consumer)]
    provider = os.path.basename(provider)
    while todo:
        drv = todo.pop()
        if drv == provider:
            return True
        if drv in seen:
            continue
        seen.add(drv)
        todo.extend(graph.get(drv, ()))
    return False


def run_build(root, attrs, output, label, kind="packages"):
    if kind not in ("packages", "checks"):
        raise ValueError("unsupported Nix output kind: " + kind)
    output.mkdir(parents=True, exist_ok=True)
    targets = ["%s#%s.%s.%s" % (root, kind, SYSTEM, x) for x in attrs]
    dry, dry_time = call(["nix", "build", "--dry-run", "--no-link", *targets],
                         root, timeout=900, check=False)
    built, built_time = call(["nix", "build", "--no-link", "--json",
                             "--print-build-logs", "--log-format", "raw", "-v",
                             *targets], root, timeout=3600, check=False)
    log_file = output / (label + ".nix.log")
    log_file.write_text("DRY RUN\n" + dry.stdout + dry.stderr +
                        "\nREAL BUILD\n" + built.stdout + built.stderr)
    outputs = []
    if built.returncode == 0:
        for item in json.loads(built.stdout):
            outputs.extend(item.get("outputs", {}).values())
    local = sorted(set(re.findall(r"\bbuilding '([^']+\.drv)'", built.stderr)))
    copied = sorted(set(re.findall(r"\bcopying path '([^']+)'", built.stderr)))
    downloaded = sorted(set(re.findall(r"\bdownloading '([^']+)'", built.stderr)))
    # "already present" only after a prior identical build/known output,
    # not merely because its derivation path was stable.
    return {
        "targets": attrs, "target_kind": kind, "dry_run": dry_time,
        "build": built_time, "realized_outputs": outputs,
        "local_build_logs": local, "substitution_logs": copied + downloaded,
        "classification_note": "No matching log entry is UNKNOWN, not a cache hit",
        "log": str(log_file), "dry_run_stderr": dry.stderr[-4000:],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=Path("incremental-ci-evidence"))
    parser.add_argument("--cases", nargs="+", choices=sorted(CASES), default=list(CASES))
    parser.add_argument("--build", action="store_true", help="materialize selected real Nix packages")
    parser.add_argument("--no-warm", action="store_true",
                        help="omit baseline identical repeat for isolated source-edit commits")
    parser.add_argument("--build-cases", nargs="+", choices=sorted(CASES),
                        default=["B", "C", "D", "G", "H", "J"])
    args = parser.parse_args()
    if args.build:
        selected = sorted(set(args.cases) & set(args.build_cases))
        unsupported = [case for case in selected
                       if case not in BUILD_TARGETS and case not in BUILD_CHECK_TARGETS]
        if unsupported:
            parser.error("no physical build targets for cases: " + ", ".join(unsupported))
        if not selected:
            parser.error("--build requires at least one selected --build-cases case")
    repo = Path(subprocess.check_output(
        ["git", "rev-parse", "--show-toplevel"], text=True).strip()).resolve()
    sha = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip()
    target = args.output.resolve()
    target.mkdir(parents=True, exist_ok=True)
    baseline, baseline_sec = inventory(repo, "packages")
    baseline_checks, checks_sec = inventory(repo, "checks")
    if not baseline_checks or not baseline:
        raise RuntimeError("empty Nix flake inventory")
    needed = set(BASIC) | {COMPOSITION, "phenix-kernel", "phenix-core-example-plugins",
                          "phenix-basic-verification-composition",
                          "phenix-basic-http-composition", "phenix-advanced-composition",
                          "phenix-basic-runtime"}
    missing = sorted(needed - baseline.keys())
    if missing:
        raise RuntimeError("expected existing package products disappeared: %s" % missing)
    graph, graph_time = dependencies(repo, [baseline[k] for k in sorted(needed)])
    evidence = {
        "schema": "phenix.incremental-ci-evidence.v1",
        "head_sha": sha,
        "toolchain": {
            name: call([name, "--version"], repo, check=False)[0].stdout.strip()
            for name in ("nix", "cargo", "rustc")
        },
        "github_run_url": os.environ.get("GITHUB_SERVER_URL", "https://github.com")
                           + "/" + os.environ.get("GITHUB_REPOSITORY", "matthis-k/phenix-ai")
                           + "/actions/runs/" + os.environ.get("GITHUB_RUN_ID", "unknown"),
        "recorded_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "flake_lock_sha256": __import__("hashlib").sha256((repo / "flake.lock").read_bytes()).hexdigest(),
        "cargo_lock_sha256": __import__("hashlib").sha256((repo / "Cargo.lock").read_bytes()).hexdigest(),
        "example_cargo_lock_sha256": __import__("hashlib").sha256((repo / "examples/Cargo.lock").read_bytes()).hexdigest(),
        "baseline_inventory": baseline, "baseline_checks": baseline_checks,
        "baseline_eval": baseline_sec, "check_eval": checks_sec,
        "tracked_input_drvs": graph, "graph_collection": graph_time,
        "cases": {}, "builds": {},
    }
    error = None
    if args.build:
        # Real edit impact requires a realized baseline on this exact runner.
        # Otherwise ordinary cold-build work is misreported as edit rebuilds.
        baseline_targets = {
            attr for case in args.cases if case in args.build_cases
            for attr in BUILD_TARGETS.get(case, ())
        }
        baseline_check_targets = {
            attr for case in args.cases if case in args.build_cases
            for attr in BUILD_CHECK_TARGETS.get(case, ())
        }
        baseline_targets.update(
            attr for case in args.cases if case in args.build_cases
            for attr in BASELINE_EXTRA_TARGETS.get(case, ())
        )
        if not args.no_warm:
            baseline_targets.update(WARM_CONTROL)
        if baseline_targets:
            evidence["builds"]["baseline"] = run_build(
                repo, sorted(baseline_targets), target, "baseline")
            if evidence["builds"]["baseline"]["build"]["exit_code"] != 0:
                error = "baseline package materialization failed"
        if baseline_check_targets and not error:
            evidence["builds"]["baseline-checks"] = run_build(
                repo, sorted(baseline_check_targets), target, "baseline-checks",
                kind="checks")
            if evidence["builds"]["baseline-checks"]["build"]["exit_code"] != 0:
                error = "baseline check materialization failed"
    try:
        with tempfile.TemporaryDirectory(prefix="phenix-nix-isolation-") as temporary:
            # Avoid changing or restoring tracked files in the original checkout.
            for case in args.cases:
                root = Path(temporary) / ("probe-" + case)
                call(["git", "worktree", "add", "--detach", str(root), sha], repo)
                try:
                    changed_files = mutate(root, case)
                    after, elapsed = inventory(root, "packages")
                    after_checks, check_elapsed = inventory(root, "checks")
                    if sorted(after_checks) != sorted(baseline_checks):
                        raise AssertionError("case %s altered canonical check names" % case)
                    delta = sorted(k for k in baseline if baseline[k] != after.get(k))
                    new_attrs = sorted(set(after) - set(baseline))
                    guards = GUARDS.get(case, {})
                    changed_missing = sorted(set(guards.get("changed", [])) - set(delta))
                    unexpectedly_changed = sorted(set(guards.get("stable", [])) & set(delta))
                    if case == "A" and (delta or new_attrs or baseline_checks != after_checks):
                        unexpectedly_changed += delta or ["check inventory changed"]
                    # Infer changed dependents from real Nix inputDrvs.
                    consumers = {}
                    for key in delta:
                        if key not in needed:
                            continue
                        consumers[key] = sorted(
                            candidate for candidate in needed
                            if candidate != key and is_dependent(
                                baseline[candidate], baseline[key], graph))
                    if case == "C" and baseline_checks.get("p2-basic-staged-plugin") == after_checks.get("p2-basic-staged-plugin"):
                        changed_missing.append("checks.p2-basic-staged-plugin")
                    if case == "I" and baseline_checks.get("p2-basic-staged-plugin") == after_checks.get("p2-basic-staged-plugin"):
                        changed_missing.append("checks.p2-basic-staged-plugin")
                    record = {
                        "scenario": CASES[case][0], "baseline_sha": sha,
                        "probe_base_sha": sha, "changed_files": changed_files,
                        "packages_changed": delta, "new_packages": new_attrs,
                        "package_derivation_diff": {
                            key: {"baseline": baseline[key], "changed": after.get(key)}
                            for key in delta
                        },
                        "checks_changed": sorted(k for k in baseline_checks
                                                 if baseline_checks[k] != after_checks[k]),
                        "check_derivation_diff": {
                            key: {"baseline": baseline_checks[key], "changed": after_checks[key]}
                            for key in baseline_checks
                            if baseline_checks[key] != after_checks[key]
                        },
                        "must_change": guards.get("changed", []),
                        "must_stay": guards.get("stable", []),
                        "expected_changed_missing": changed_missing,
                        "unexpected_changed": unexpectedly_changed,
                        "actual_reverse_dependents": consumers,
                        "eval_seconds": elapsed["seconds"],
                        "checks_eval_seconds": check_elapsed["seconds"],
                        "status": "pass" if not (changed_missing or unexpectedly_changed) else "fail",
                    }
                    evidence["cases"][case] = record
                    if args.build and case in args.build_cases:
                        kind = "checks" if case in BUILD_CHECK_TARGETS else "packages"
                        attrs = (BUILD_CHECK_TARGETS if kind == "checks" else BUILD_TARGETS)[case]
                        identities_before = baseline_checks if kind == "checks" else baseline
                        identities_after = after_checks if kind == "checks" else after
                        if any(identities_before[a] == identities_after[a] for a in attrs):
                            raise AssertionError(
                                "physical probe %s selected an unchanged %s target" % (case, kind))
                        build_record = run_build(root, attrs, target, case, kind=kind)
                        build_record["target_derivations"] = {
                            attr: {"baseline": identities_before[attr],
                                   "edited": identities_after[attr]}
                            for attr in attrs
                        }
                        build_record["status"] = (
                            "pass" if build_record["build"]["exit_code"] == 0
                            and build_record["realized_outputs"] else "fail"
                        )
                        evidence["builds"][case] = build_record
                        if build_record["status"] != "pass":
                            record["status"] = "fail"
                finally:
                    call(["git", "worktree", "remove", "--force", str(root)],
                         repo, timeout=120)
            if args.build and not args.no_warm:
                control = WARM_CONTROL
                evidence["builds"]["K-first"] = run_build(repo, control, target, "K-first")
                evidence["builds"]["K-repeat"] = run_build(repo, control, target, "K-repeat")
                for phase in ("K-first", "K-repeat"):
                    if evidence["builds"][phase]["build"]["exit_code"] != 0:
                        error = "identical build %s failed" % phase
                first, repeat = evidence["builds"]["K-first"], evidence["builds"]["K-repeat"]
                if not error:
                    same_outputs = first["realized_outputs"] == repeat["realized_outputs"]
                    no_rebuild = not repeat["local_build_logs"]
                    no_substitution = not repeat["substitution_logs"]
                    evidence["builds"]["K-repeat"]["status"] = (
                        "pass" if same_outputs and no_rebuild and no_substitution else "fail"
                    )
                    if not (same_outputs and no_rebuild and no_substitution):
                        error = "identical second Nix build rebuilt, substituted, or changed outputs"
                # Cross-run (L) must be collected on a separate clean runner
                # with a known populated read-only cache, never inferred here.
                evidence["builds"]["L"] = {
                    "status": "unverified", "reason": "requires clean runner and recorded cache restore",
                    "classification": "not a same-run warm-build result"}
    finally:
        (target / "evidence.json").write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n")
    failed = [k for k, v in evidence["cases"].items() if v["status"] != "pass"]
    if failed or error:
        raise SystemExit("Nix isolation regression: failed %s %s" % (failed, error))
    print("Nix isolation verified for cases: " + ", ".join(evidence["cases"]))
    print("Evidence: " + str(target / "evidence.json"))


if __name__ == "__main__":
    main()
