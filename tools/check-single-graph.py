#!/usr/bin/env python3
"""Fail CI if a second graph implementation or Basic-agent variant appears.

This protects source topology, not semantic correctness of graph execution.
ACP/JSONL are two protocol adapters over the same Basic native graph host;
independent native provider and product-test binaries are not graph engines.
"""
from pathlib import Path
import re
import sys

root = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()

required = [
    "crates/phenix-graph/src/lib.rs",
    "crates/phenix-graph/src/session.rs",
    "crates/phenix-kernel/src/graph/native_run.rs",
    "crates/phenix-contract/src/graph.rs",
]
obsolete = [
    "crates/phenix-kernel/src/graph/v2.rs",
    "crates/phenix-contract/src/graph/v2.rs",
    "crates/phenix-macros/src/graph.rs",
    "crates/phenix-conformance/src/bin/phenix-m1-graph.rs",
]
errors = []
for path in required:
    if not (root / path).is_file():
        errors.append(f"missing sole graph component: {path}")
for path in obsolete:
    if (root / path).exists():
        errors.append(f"obsolete parallel graph implementation: {path}")

legacy_tokens = re.compile(
    r"\b(?:GraphContributionV2|StaticGraphContribution|StaticGraphNode|"
    r"GraphExecution|GraphStatus|GraphCompletion|prepare_graph_v2|"
    r"compiled_graph_v2|basicStagedAgent)\b"
)
for base in ("crates", "examples"):
    for path in (root / base).rglob("*.rs"):
        if "/target/" in path.as_posix():
            continue
        for i, line in enumerate(path.read_text().splitlines(), 1):
            if legacy_tokens.search(line):
                errors.append(f"{path.relative_to(root)}:{i}: obsolete graph symbol")

for config in (
    root / "crates/phenix-plugin-basic-agent/Cargo.toml",
    root / "flake.nix",
):
    text = config.read_text()
    if "staged-graph" in text or "basicStagedAgent" in text:
        errors.append(f"{config.relative_to(root)}: duplicated Basic agent build")

if errors:
    print("\n".join(errors), file=sys.stderr)
    raise SystemExit(1)
print("Single graph implementation and single Basic-agent artifact verified.")
