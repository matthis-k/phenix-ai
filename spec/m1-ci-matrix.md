# M1 CI matrix rollout

## Scope

Restore the earlier dependency-oriented CI layout for the M1-only kernel.
The goal is lower critical-path latency without replacing or weakening the
canonical Nix checks. This plan uses existing Nix derivations and GitHub Actions.
It introduces no second build description and no alternate source of truth for
test expectations.

The baseline from 12 October 2026 is 15m20s for G1-G3 exact-head shipping
run 38231650549 and 24m35s for G4 run 38233478913. G4 compiled
`phenix-contract` 54 times, `phenix-sdk` 48 times, and
`phenix-macros` 48 times. The build layout causes repeated Cargo work,
and runner speed also varies. A matrix alone may increase billed runner time
and will not fix intra-derivation recompilation.

## Candidate already implemented

`.github/workflows/minimal-kernel-matrix.yml` is a separately named
push/dispatch benchmark workflow. The existing
`.github/workflows/minimal-kernel.yml` remains the sole shipping gate.

- Shared job: check out the exact branch head, install pinned Nix, restore the
  prior branch cache, then build the existing kernel, artifact descriptor tool,
  and conformance runner. Only this job saves the Nix-store cache.
- Parallel jobs: four suites, `contracts`, `composition`, `coordination`,
  and `graph-agent`, restore the completed shared cache read-only. Each uses
  one multi-target `nix build` invocation, failing if any constituent check
  fails. Each uploads its checked-out commit, selected check names, Nix store
  paths, and Nix version.
- Aggregate job: fails if the shared job or any shard fails. No job suppresses
  failure because another suite passed.
- Cache key: Linux, authoritative Cargo/Nix locks, and exact revision.
  Previous matching snapshots can be restored; only the shared prebuild job
  saves new snapshots. The store budget is 4 GiB for the candidate.

At the original #790 benchmark the candidate had 24 ordinary checks, with
`m1-shipping` in the serial acceptance job. On the later
`impl/m1-basic-acp` head, the flake defines 35 checks: all 34 ordinary checks
are listed in the matrix and `m1-shipping` remains in the serial gate.
The HTTP model slice adds `b4-http`, bringing its exact-head inventory to 36
checks: 35 ordinary checks in the matrix plus serial `m1-shipping`.
The matrix list is still maintained by hand. The generated planner must
derive check inventory from the exact-head flake and reject omissions.
The candidate cannot replace the serial gate until `m1-shipping` and the
exact-head release evidence are verified in the generated workflow.

## Flake-aware candidate in #806

The new `.github/workflows/minimal-kernel-flake.yml` calls the pinned,
reusable workflow from `phenix-flake-ci` #17. This is separate from the older
hardcoded matrix and the canonical serial shipping gate.

- `ci/flake-check-config.nix` injects the Rust strategy into generic Nix
  planning. It names reusable package outputs, shard weights, the shipping
  isolation hint and lockfiles for cache identity.
- The planner evaluates `checks.x86_64-linux` on the checked-out commit.
  Every discovered check, including `m1-shipping`, enters exactly one shard.
- One prebuild writer populates the Nix store cache. Parallel shards restore
  it read-only and upload commit-bound check results and store paths.
- The aggregate verifies the manifest hash, complete shard inventory,
  reported check derivations and matching prebuild artifacts. Skipped or
  canceled shards do not count as passing checks.

The existing `minimal-kernel.yml` also runs for #806, so both workflows can
be compared at the same head. The old manual matrix remains a benchmark.
Exact-head success, optimized shipping report parity, cold/warm timing, and
fixture-source consolidation are still open. No selective PR skipping is active.

## Acceptance criteria

1. Compare the matrix's check names with the evaluated Nix check attribute
   set on its exact head. Changes to the flake must fail review unless new
   checks are accounted for. Eliminate this manual list by deriving shards
   from a single Nix-owned manifest during the final cutover.
2. Verify `contracts`, `composition`, `coordination`, and `graph-agent`
   all complete successfully on the same commit. Confirm the summary fails on
   an individual failing shard and does not accept a missing or canceled job.
3. Measure a cold run and a warm run on the same runner type. Record wall-clock
   time, total billed runner-minutes, cache download/upload size, hit rate and
   shard-level durations. Use the existing 15m20s and 24m35s runs for
   comparison, but account for runner variance.
4. Prove a source-only change to one graph fixture does not rebuild unrelated
   fixtures after narrowing their derivation sources. Current
   `pluginSource` includes all examples, which invalidates every fixture.
5. Consolidate common fixture compilation into shared Cargo outputs, while
   retaining isolated public-SDK checks and independent builds for fat/off
   LTO, alternate linker flags, repeatability, and explicit security-negative
   cases. Do not hide independent compilation assumptions behind copied
   products. Preserve native descriptor and provenance checks.
6. Move `m1-shipping` and the exact-head evidence artifact into a required
   aggregate with verified dependencies on all matrix shards. No aggregate
   may silently recompile the entire old suite or trust unauthenticated
   uploaded results. Keep golden report names and checksums unchanged.
7. Compare the full realized output set with the legacy exact-head workflow,
   including semantic negative cases and the production closure. Retire the
   legacy single job only after equivalent successful runs.
8. Integrate conservative dependency selection after equivalence. Derive
   changed crates and reverse path dependents from Cargo metadata. Unknown
   inputs, changed shared contracts, toolchain/configuration changes, and
   main/release verification run all required checks. No skipped check may be
   interpreted as passed without an explicit unaffected proof.

## Cache ownership and trust

GitHub Actions caches created by `pull_request` are scoped to that PR's
merge ref. They are not generally available to sibling PRs in a stacked
series. The benchmark publishes from a branch `push` instead. Production
CI should publish reusable caches from a trusted base or protected branch,
and PR matrix jobs should restore without write access. Do not use
`pull_request_target` to run untrusted pull-request code with a write token.

A public, read-only Nix binary cache such as the separately configured
Cachix distribution can extend reuse across repositories. Only a trusted
post-validation publisher should hold a write token. Publishing should be
based on the validated exact revision, not on a mutable PR branch name.

The cache is an optimization, never an authority source. Restored store
paths must still be addressed and verified by Nix. Caches must be keyed by
the pinned inputs and compilation settings. Keep branch-specific and
platform-specific outputs isolated.

## Flake-aware CI redesign

[Phenix AI's M1 roadmap](m1-roadmap.md) now tracks AI-CI1 to AI-CI4 and
cross-repository FC1 to FC4.
[The generic CI-library design](https://github.com/matthis-k/phenix-flake-ci/pull/17)
is owned by `phenix-flake-ci`; this file retains the measured matrix baseline
and consumer acceptance rules. The generic planner and consumer integration are implemented as candidates; parity and cutover remain planned.

### Consumer and library boundaries

- Phenix AI owns Nix checks and derivations, compatible Cargo build sharing,
  fixture source narrowing, native package identity, fast profiles and
  optimized shipping tests.
- `phenix-flake-ci` owns the optional exact-head check inventory, conservative
  Cargo/Nix impact planner, deterministic suite grouping, cache policy,
  complete result gate and workflow generation.
- Nix decides which derivations are reusable. Sharding cannot itself eliminate
  compilation repeated by independent Cargo target directories.
- Keep one conformance definition in the consumer flake. Generate the CI job
  plan from that definition; never clone it into a second YAML checklist.

### Implementation order

1. AI-CI1: consolidate compatible fixture compilation and narrow source
   inputs. Retain independent builds for ABI/adversarial/linker cases. Benchmark
   the same head before and after.
2. AI-CI2: test a fast semantic compilation profile while keeping the
   optimized ABI, LTO, shipping and closure gates.
3. FC1-FC3: implement a check-discovery manifest, provable impacted-check
   planning and a generated exact-head evidence gate using the existing
   `mkCi` DAG. FC4 prepares trusted cache policy and metrics in parallel.
4. AI-CI3: prove full check coverage and shipping parity before the generated
   workflow becomes required. Remove the parallel serial workflow only after
   that proof.
5. AI-CI4: activate selective PR checks and read-only shared-cache reuse.
   Full main/release/manual validation always includes all checks.

Benchmark cold and warm runs, compile counts, realized versus substituted Nix
derivations, cache bytes, wall time and total runner time. Record skip proofs
and negative/ABI test coverage alongside faster times. Initial warm-cache
targets are 2-4 minutes for a small plugin PR and 5-8 minutes for a complete
gate; these are unverified targets, not a claim of achieved speed.

## Cutover criteria

Maintain the old shipping workflow until AI-CI3 proves the complete check
inventory, exact-head evidence and production closure match. Introduce
selective PR skipping under AI-CI4 only after the full generated gate works.
Mark milestones complete only after their measured acceptance work passes.
