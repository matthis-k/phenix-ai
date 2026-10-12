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

The candidate runs all 24 ordinary `flake.checks.x86_64-linux` entries.
The 25th entry, `m1-shipping`, remains in the original full shipping job.
The candidate cannot be accepted as the sole gate until `m1-shipping` and
the exact-head release evidence are verified in the new workflow.

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

## Cutover sequence

1. Run the benchmark on this branch. Record observed timing and cache
   evidence in the PR. Treat cold and warm results separately.
2. Fix fixture compilation and source invalidation, then benchmark again.
3. Implement aggregate shipping verification and exact-head evidence
   parity. Make the matrix required, then remove the redundant serial gate.
4. Expand trusted shared-cache publication to the canonical branch and
   downstream read-only consumers. Add conservative affected-check planning.

Do not mark this document complete or claim a latency improvement before
the measured acceptance work passes.
