# M1 CI matrix rollout

## Incremental Nix isolation verification (19 October 2026)

This section extends the current CI acceptance evidence; it is **not** another
source of truth for Nix dependencies or check names. The canonical dependency
and check inventory remain the exact-head `flake.nix`. All distributed-build
orchestration, worker credentials and shared-cache publication remain in the
pinned `phenix-flake-ci` library.

### Baseline and scope

- Baseline `main`: `45d77ae6508a2fc8b8fa39bbc0f6d08f12e825c4`.
- `flake.lock` pins Nixpkgs `e7a3ca8092b61ff85b6a45bf863ea2b2d6a661b3`.
  Cargo lockfiles: `Cargo.lock`, `examples/Cargo.lock`.
- Trusted main remote Nix check workflow:
  [39201091225](https://github.com/matthis-k/phenix-ai/actions/runs/39201091225)
  passed, 7m54s end-to-end.
  Prior trusted main run
  [39197830049](https://github.com/matthis-k/phenix-ai/actions/runs/39197830049)
  passed, 5m43s end-to-end.
  Worker topology: 4 ephemeral Tailscale `ssh-ng` builders, 2 slots each.
- Latest main independent official ACP interop:
  [39201091104](https://github.com/matthis-k/phenix-ai/actions/runs/39201091104)
  passed, 6m47s end-to-end.
- Trusted shared-store publication was **skipped** on run 39201091225;
  its passing result is not proof of an external binary cache upload.
  The GitHub Actions cache save and remote-worker transfers are separate
  mechanisms from public signed binary-cache substitution.

### Reproducible diagnostics

```sh
# Fast diagnostic: canonical package/check drv identities and Nix inputDrvs
bash .github/scripts/test-basic-plugin-cache-isolation.sh \
  --output incremental-ci-evidence

# Opt-in slower diagnostic: materialize selected packages, capture real Nix
# build/substitution logs, and replay unchanged installables on one runner
bash .github/scripts/test-basic-plugin-cache-isolation.sh \
  --build --output incremental-ci-build-evidence
```

The harness uses detached, disposable Git worktrees. Source mutation cannot
overwrite tracked files in the original checkout. Cases A–J compare all
evaluated Nix package and check identities and inspect actual `inputDrvs`.
An explicit small set of package guards catches unintended coupling; the
canonical graph comes from Nix rather than a parallel handwritten graph.
Machine-readable JSON, dry-run plans and actual `nix build` logs are
retained by the existing cache-isolation workflow as Actions artifacts.

A source edit changing a derivation name alone is **not** a cache hit:
the evidence must separately report actual local compilation, substitute
transfer, or an already-present output. A second build in the same job (K)
does not establish cross-run substitution on a fresh runner (L). Until
controlled cross-run read-only cache restoration is demonstrated, L remains
unverified. No PR test receives Tailscale secrets or binary-cache write access.

### Outstanding acceptance evidence

See [incremental-build PR #996](https://github.com/matthis-k/phenix-ai/pull/996)
for the experiment commits and exact-head run IDs. Only completed, inspected
runs may be described as verified. The scenario matrix must be updated with
actual changed/rebuilt derivations and outputs, cache restore/substitution,
wall time and failure classification before marking the PR ready. Do not
infer full-suite performance from these narrow diagnostic checks.


## Current CI principles and applied assessment (15 October 2026)

This section supersedes the **historical candidate/cutover status** recorded below, not its benchmark measurements or uncompleted optimization tasks. It is an engineering guide, not a second validation authority. The [M1 roadmap](m1-roadmap.md) remains the milestone tracker. A generated CI workflow and `flake.nix` remain the source of executable check definitions.

**General standard:** CI should give quick, reliable evidence of *behavioral correctness*, including failure behavior and permissions, while keeping integration frequent and main consistently usable. Do not introduce a separately maintained architecture-rule model or AI reviewer as a CI gate. The Rust SDK, typed contract resolver, graph compiler and runtime authority implementation define admissibility; tests exercise their true positive and negative behaviors. Use code review and optional development-time Architecture skills for qualitative design choices.

Evidence-based guidance: [DORA continuous integration](https://dora.dev/capabilities/continuous-integration/) and [test automation](https://dora.dev/capabilities/test-automation/), [Martin Fowler's CI](https://martinfowler.com/articles/continuousIntegration.html), [Google on reliability vs fidelity](https://testing.googleblog.com/2024/02/increase-test-fidelity-by-avoiding-mocks.html), and [GitHub's workflow/cache trust guidance](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching).

### Observed production layout

| Surface | Current design | Purpose / caveat |
| --- | --- | --- |
| `flake.nix` `checks.x86_64-linux` | Canonical complete check inventory | Do not duplicate names in a handwritten YAML suite table |
| `ci/flake-check-config.nix` | Flake-derived, five-shard Rust strategy and measured prebuild hotset | Planner parameters are performance hints; Nix derivations define actual tests |
| `minimal-kernel-flake.yml` | Credential-free full PR matrix candidate calling pinned `phenix-flake-ci` | Its aggregate must require every discovered check and exact-head evidence; preserve until stronger measured alternative |
| `p1-rustfmt.yml` + `verify-generated-flake-ci.yml` | Quick, independent PR feedback | The first checks formatting; the second checks generated workflow drift and existing security/CI boundary assertions |
| `p1-acp-sdk-interop.yml` | Independent official ACP client against the packaged Phenix host | Important high-fidelity product test; build time must be measured before optimization |
| `phenix-flake-ci-generated.yml` | Full privileged remote-builder CI on trusted `main` push | Generated from pinned Nix policy; OIDC and remote credentials must never flow into untrusted PR jobs |
| Manual benchmark workflows | Exact-head cold/warm, remote-builder comparison | Performance evidence, not separate definitions of software correctness |

The complete PR matrix run [38683762185](https://github.com/matthis-k/phenix-ai/actions/runs/38683762185) completed successfully in about 35 minutes on 15 October. Separate formatting/CI-policy jobs on nearby heads completed within tens of seconds, while official ACP integration took several minutes. These are observed end-to-end durations, **not isolated build-time or warm-cache benchmarks**. Do not attribute the 35 minutes to a particular cause without a same-head step/derivation breakdown. Older 12 October timings remain historical baselines.

### Feedback stages, not duplicated suites

1. **Fast PR feedback:** formatting, generated-policy integrity and inexpensive deterministic functional checks that genuinely catch common mistakes quickly. Prefer existing Rust unit tests and targeted Nix check derivations; no new handwritten copy of test expectations. Aim for feedback in minutes, measure p50/p95 time-to-first-actionable-failure, and do not add a job merely to make a new badge.
2. **Complete PR verification:** compiled artifact and real kernel/plugin behavioral tests, cancellation, authorization denial, restart, selected provider substitution, and independent ACP interoperability. Check *all* discovered Nix targets while impact selection remains unproven. Invalid configurations must be rejected by the actual kernel, not by an alternate CI validator.
3. **Trusted main/release:** full exact-head verification of the integrated commit, publication or attestation of verified outputs, and the required independent compatibility evidence. Never infer that green stacked PR heads imply the final combined main SHA is green.
4. **Manual/periodic diagnostics:** cold/warm cache, different remote-builder counts, sustained performance, toolchain upgrades or expensive adversarial workloads. Their outputs inform optimization; they do not silently bypass required functional checks.

Google's test-size/fidelity guidance motivates mostly fast isolated deterministic tests, complemented by enough real provider/product tests to prove important interactions. Avoid asserting exact model prose, unconstrained live network behavior, real-time sleeps or timing-sensitive test outcomes when a deterministic fixture is sufficient. Real external protocols still require separate compatibility tests.

**Do not enumerate every plugin subset.** Test provider selection and rejection semantics with focused representative fixtures; run the packaged kernel-only, Basic, verification and (when implemented) Advanced presets. A test should fail when its actual feature is broken, not because the same feature is restated in a second architecture policy file.

### Prioritized improvements and measurement gates

| Order | Work | Evidence required before calling it an improvement |
| --- | --- | --- |
| 1 | **Diagnose latency first:** collect exact-head per-phase/shard Nix build and cache restore/save durations, local-versus-remote realized derivations, queue time, cold/warm hit/miss bytes, and the first meaningful failure | A reproducible bottleneck ranking; measure wall clock **and** runner-minutes, not only the number of parallel workers |
| 2 | **Reuse compatible compilation:** shrink Nix source closures and share Cargo outputs where ABI/toolchain assumptions match; retain independent native ABI/security-negative fixtures | Same exact-head check inventory and semantic negatives, fewer realized derivations/recompilations, verified warm and cold comparison |
| 3 | **Move valuable failures earlier:** identify slow product-test failures whose invariant can also be exercised by an existing cheap Rust/unit/contract test | Smaller p50/p95 time-to-first-actionable-failure while the full product behavior test still passes and remains required |
| 4 | **Tune the existing five-shard planner and 4×2 backend only from measurements** | Balanced shard critical path and lower wall time or cost without reducing evidence or weakening the trusted/untrusted boundary |
| 5 | **Consider conservative impacted-check selection** only when the shared planner can derive reverse dependencies from the actual Nix/Cargo build and explain every skipped check | Adversarial proof that changed shared contracts, tools, Nix config or unknown paths trigger full validation; main/release always full; skip is not a pass |
| 6 | **Track flaky failures explicitly** and repair nondeterminism instead of accepting silent retries | Every reported flaky case has a reproducer, named owner and visible status; no genuine regression becomes green due to retries |

Prefer existing GitHub workflow summaries and Nix logs for these measurements rather than a new telemetry service. Distinguish time spent waiting for runners, preparing the environment, restoring caches, compiling, running tests and collecting artifacts. A faster workflow that hides a failure, drops a check or exposes credentials is a regression.

### Acceptance policy

- Same source revision, same selected composition and complete observable outcomes. Any missing, skipped, failed or canceled required check makes the aggregate unsuccessful.
- No additional architecture CI gate, model inference, YAML contract mirror or independent test-orchestration engine.
- Cache substitution is performance only; pinned build inputs, verified artifacts and runtime admission remain authoritative.
- Keep PR execution credential-free and repository/token permissions minimal. Trusted cache publication and remote OIDC stay on explicitly trusted triggers.
- Evaluate any optimization on **comparable cold and warm runs at the same SHA**. Preserve the original full check inventory, negative tests and real product behavior.
- New functionality adds tests at its actual owning implementation boundary; remove superseded or redundant test scaffolding only after proving equivalent behavioral coverage.

## Historical rollout and baseline measurements

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
