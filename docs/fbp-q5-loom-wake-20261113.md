# Q5 — Loom model checks of the actual mutex/atomic wake implementation

Date: 2026-11-13. Base: exact-head-verified draft [PR #1298](https://github.com/matthis-k/phenix-ai/pull/1298) at `54494be22d87d227111484b5bb7903face721682`; [six-job Flake CI #43420534000](https://github.com/matthis-k/phenix-ai/actions/runs/43420534000) and [eight-repeat Q5 #43420533621](https://github.com/matthis-k/phenix-ai/actions/runs/43420533621) passed. All stacked PRs are draft/unmerged. Prior [#1295](https://github.com/matthis-k/phenix-ai/pull/1295) allocation optimization was rejected for H5/EOS regressions and is not included.

## Phase 0 — capability gap

The existing 13,440-order Q5 test is exhaustive **only over already-serialized operations** on a `Mutex<WakeState>`. The eight repeated thread stress runs exercise real `std::sync` threads but cannot show which instruction interleavings were covered. We want **instrumented thread and atomic scheduling of the actual `notify_target()`, `retire_target()`, `notify()`, `invalidate()`, and `take_scheduler_wakes()` method bodies**, including their mutex order and `WakeSlot.active/pending` atomic accesses.

The baseline does not contain Loom. A separately maintained mock lock protocol would be weaker than running the real implementation; wholesale conversion of the production graph/workspace to Loom would add unacceptable cost and API surface.

## Phase 1 — considered designs

| Design | Advantage | Risk / cost | Decision |
| --- | --- | --- | --- |
| Extend serial permutation generator | Fast, exhaustive serial orders | Cannot instrument atomic/thread instructions | Not sufficient |
| Introduce Loom as graph crate dev-dependency | Tests integrated under cargo workspace | Changes production workspace lock, flake dependency closure, binary size/tooling without benefit to graph API | Reject |
| Copy and reimplement WakeSignal in verifier | Isolates dependency | May silently diverge from actual method bodies | Reject |
| **Compile the same `wake.rs` in a standalone verification crate, selecting Loom sync types via verifier-only `cfg(loom)`** | Exact method bodies; external dependency isolated; model explores interleavings | Verifier-only import/availability directives needed; Loom scheduler budget | Select |

No new runtime state, scheduler API or synchronization semantics. The normal production compilation continues to use `std::sync` exactly as before.

## Phase 2 — adversarial critique

1. **Real implementation vs model fidelity:** The standalone verifier `include!`s the actual `packages/phenix-graphs/src/runtime/wake.rs`; it uses Loom's `Arc`, `Mutex`, `Condvar`, and `AtomicBool` with the **same actual lock, atomic, target set and method bodies**. There is no pasted/reconstructed `notify_target` or `retire_target` implementation.
2. **Honest omission:** Loom 0.7.2's `Condvar` lacks `wait_timeout_while`. The verifier excludes only `WakeListener` and its existing native `std::thread` unit tests, then covers the instrumentable notifier/retire/invalidate/drain core. It does **not** model-check the full listener deadline logic; the previous native listener race tests remain required.
3. **Bounded proof, not exhaustive schedules:** Loom Builder uses a preemption bound of 2, maximum 20,000 permutations per property and max three threads. Each completed schedule is counted and must be >1. Exhaustive language is limited to those tested bounds; no claim about unlimited thread counts, arbitrary preemptions or all plugin locks.
4. **Three properties:** (a) retirement discards stale queued targets and pending flags across a racing notify; (b) a new owner at the same compiled index never inherits old slot authority despite late old wake; (c) terminal invalidation rejects all late targeted/global notifications and clears targets. All are assessed on **the original methods**, with actual thread joins and atomics.
5. **Reproducibility:** Isolate `loom = "=0.7.2"` under `verification/fbp-wake-loom/Cargo.toml` with its own `[workspace]`; generate its own `Cargo.lock`, pin that lock after initial CI bootstrap; run `--locked` in the dedicated workflow, collect source + lock SHA, rustc version and schedule counts. The main workspace `Cargo.lock` and Nix flake inputs remain untouched.
6. **No product changes on verification branches:** A real counterexample discovered by Loom blocks acceptance and must be handled by a *separate Phase 0→4 runtime fix*. Testing-only modules must not silently ship a changed lock order. Compiler conditional directives are verifier-only; `cfg(loom)` never enters ordinary graph builds.
7. **Larger remaining Q5 scope:** Plugin-defined lock cycles, arbitrary callback recursion, process dispatcher/host coordination, external tool cancellation and full runtime liveness/fairness are outside these three tests. CI success does not close Q5 or M7.

## Phase 3 — implementation and acceptance gates

- Select `loom::sync` types only under `cfg(loom)` in `wake.rs`; preserve `std::sync` under `not(loom)`. Scope the unusual `cfg` lint to this module.
- Add an isolated verifier crate that `include!`s `wake.rs` and runs three two-thread Loom models (no copied runtime code).
- Dedicated GitHub workflow checks source hashes, compiler, pinned verifier lock, format, and model result logs. Verify exact-head six-job Flake CI, Q5 native-thread regression suite, and all existing H4/H6 workflows.
- Do not mark Q5 resolved. On success, minimally increase the tracked one-decimal Q5 estimate **67.0% → 70.0% (+3.0 pp)**; H3b **74.0% → 74.0% (0.0 pp)**. This reflects actual method-level interleaving assurance, still without universal plugin safety. Every one of the 33 issue deltas must be preserved in the scorecard.

## Phase 4 — evidence pending

Record the exact final commit and six-job Flake CI and all other relevant workflow links. If Loom test or any cross-workload check fails, record the counterexample, fix/reject the candidate, and do not increment the score. Standalone lockfile bootstrap is not the final reproducibility gate: require a checked-in lock and subsequent `cargo test --locked` on the final head.
