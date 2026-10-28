# Nix CI execution policy

The flake's `checks.x86_64-linux` is the single source of the complete check
inventory, including native ABI, negative/security, product, Graph V2 and
shipping acceptance. No selective test bypass is permitted.

- **Untrusted PRs**: `minimal-kernel-flake.yml` invokes the pinned
  `phenix-flake-ci` staged DAG. The same exact-head derivation graph is
  planned, high-fanout dependencies are prebuilt once, ready checks run in
  parallel, and the final dependent/shipping join verifies all results.
  It has no Tailscale/OIDC credential or publisher authority.
- **Trusted main**: the generated CI caller retains four Tailscale Nix
  builders with two slots each, and trusted cache publication.
- **Manual diagnosis**: `minimal-kernel-dag.yml` can run the original
  single-store full Nix DAG without blocking ordinary PRs.

Historical successful runs (different inventories/cache states):
- [13 October staged PR DAG](https://github.com/matthis-k/phenix-ai/actions/runs/38379399241): 17m06s, 37/37 checks.
- [13 October trusted four-builder](https://github.com/matthis-k/phenix-ai/actions/runs/38379370033): 7m29s.
- [27 October trusted four-builder](https://github.com/matthis-k/phenix-ai/actions/runs/40503332187): 16m24s.
- [28 October single-runner full DAG](https://github.com/matthis-k/phenix-ai/actions/runs/40612620171): roughly 64m.

The target is 10–15 minutes, not a verified result for current Graph V2.
Compare exact-head plan/output coverage and observed timings; do not
claim a speedup, skip failed checks, or expose trusted secrets to PR code.

The staged PR shared prebuild additionally restores compatible verified-main
Nix outputs read-only from `remote-phenix-m1-*` before building new
PR-specific derivations, while its primary key and all leaf/join caches
remain `phenix-m1-*` and cannot publish to the trusted namespace.
The cache restoration is merged in `phenix-flake-ci` PR #35 and pinned to its immutable squash commit.

## Four-builder build-then-test benchmark

This child branch of #1191 pins the opt-in `phenix-flake-ci` #36
four-build-owner workflow. The build phase derives all pure-build input
frontiers from actual Nix derivations (not a manual package list), assigns
unique root ownership to four compilation jobs, joins verified Nix store
closures before admitting independent check workers, then executes dependent
shipping acceptance after all ready checks. The build planner reports
transitive cross-worker overlap so duplication can be quantified and reduced.

This is experimental until exact-head Nix outputs, 86-check inventory,
actual build events and elapsed time match expected acceptance. The previous
staged PR mode (#1191) remains unchanged for comparison; no privileged
Tailscale credentials or trusted cache writer are exposed to PR code.

The new backend now also discovers shared dependencies between the four build
owners and prebuilds those once (with Nix GC roots) before compile fan-out.
The planner rejects residual cross-owner graph overlap after this cut.

The first four-builders benchmark failed in the common compilation phase
because Nix derivation paths evaluated in the planning job were absent from
the shared builder's isolated Nix store. The pinned CI library revision now
re-instantiates the exact-head derivation graph locally after cache restore,
asserts its check derivation identities are unchanged, and only then invokes
Nix on direct .drv targets. Regression tests cover inventory drift.

## Trusted Tailscale coordinator benchmark (2026-10-28)

The maintainer-owned `ci/native-four-coordinator-20261028` branch runs
`.github/workflows/trusted-native-four.yml` **on push only**. It reuses the
already-proven pinned `phenix-flake-ci` remote backend and four Tailscale
ssh-ng builders. One Nix coordinator schedules the entire `flake.nix` check
DAG, so the Nix store—not a custom per-shard planner—owns shared derivations.
The same 86-check inventory, negatives and shipping acceptance are mandatory;
there is no hand-maintained reduced subset, build frontier or NAR join.

The workflow intentionally does not run for pull requests and its source-branch
admission is restricted to this owned GitHub branch. Never grant OIDC/Tailscale
to fork PR code. The existing credential-free pull-request gate remains until
this trusted native run has verified coverage, timing, and the intended
maintainer-branch policy for the final cutover.
