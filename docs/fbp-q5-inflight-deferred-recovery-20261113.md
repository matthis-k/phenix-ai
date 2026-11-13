# Q5 / H3b — in-flight delegated worker, cancellation, and replacement

**Date:** 2026-11-13. **Baseline:** Q5 notifier interleaving draft [#1296](https://github.com/matthis-k/phenix-ai/pull/1296) at `ccb1bf28c2af20688cc43469962eb82494da4a1e`, [six-job CI #43413378069](https://github.com/matthis-k/phenix-ai/actions/runs/43413378069), [eight-repeat Q5 CI #43413377692](https://github.com/matthis-k/phenix-ai/actions/runs/43413377692) green. Ancestor [#1294](https://github.com/matthis-k/phenix-ai/pull/1294) adds H6 measurement only. Rejected runtime experiment [#1295](https://github.com/matthis-k/phenix-ai/pull/1295) is **not** in this branch. The stack remains draft and unmerged.

## Phase 0 — problem and baseline

Existing `fbp_streams.rs` tests assert that `complete_job(job.run())` rejects a handed job after cancellation and that its code lease survives until the returned completion is released. They run the stale job *after* cancellation. The Q5 #1296 suite races old wake/notifier handles with the cancellation/replacement boundary but the process execution itself is not in flight. Neither case exercises a **worker actually inside `Process::step` while another thread cancels its scope and runs a fresh process at the same compiled index**.

Target invariant: cancellation invalidates an old ticket and prevents any late emission from publishing into a replacement, but cannot preempt the already-running worker, undo external effects, or force its code lease to disappear before the worker returns.

## Phase 1 — alternatives

| Alternative | Value | Limit | Decision |
| --- | --- | --- | --- |
| Existing late-job tests | Already exact-head CI verified | No simultaneous in-flight `step` vs cancellation | Insufficient |
| Mock scheduler/ticket generation | Enumerates abstract sequences | Diverges from Rust owner, handoff and resource behavior | Reject for this slice |
| Introduce test-only instrumentation into runtime | Can stop at precise internal points | Adds test branches into production synchronization structures | Reject |
| **Execute real `ActivationJob::run()` in a worker, synchronize inside `Process::step`, cancel and restart through public APIs** | Tests owned process, stale completion, external effect, and live reuse | Finite scheduled orders, not a universal model check | Select |

## Phase 2 — adversarial critique

1. A job merely held by the host does **not** prove in-flight cancellation. The process calls `Barrier::wait()` inside `step()`; cancellation begins only after the driver crosses the barrier confirming entry.
2. No user code or plugin-held lock is called while session/WakeSignal locks are held. Barrier waiting occurs on a separate worker thread and main never blocks on release while holding a runtime lock.
3. Test three observable orders: (A) replacement fully settles before old worker returns; (B) old worker returns before replacement starts; (C) fresh job already dispatched while old completion returns, then fresh completion is accepted. Repeat 48 rounds on the *same* persistent session to exercise process-index reuse.
4. Require: old cancellation yields exactly its outstanding ticket; old wake and process notifier remain invalid; old `complete_job()` returns `HandoffError::StaleCompletion`; fresh request publishes one correctly tagged output only and settles; old external effects occur exactly once *despite* cancellation; no producer lock collision, duplicate execution, or capacity-one leak.
5. `SessionNotifier` may be notified after old worker completes, and `WakeListener` must observe that even though the completion is stale. The notifier is not allowed to authorize an old ticket's admission.
6. This test deliberately does **not** promise effect rollback or external cancellation, and does **not** establish generic producer/plugin lock-order safety. Worker thread schedules remain bounded, deterministic cases; the exact linearization permutations of all kernel internals are not explored.

## Phase 3 — implementation

- Add public `fbp_inflight_worker_recovery.rs` regression, 48 in-flight cancelled worker jobs and 48 corresponding fresh requests in one session; three completion orders, bounded output capacity.
- Add `FBP Q5 In-Flight Worker Recovery` workflow that exercises this test 12 times, preserves exact commit, source hash, compiler and logs. 12 × 48 = 576 genuine in-flight cancel/recovery rounds.
- Keep existing #1296 wake/retirement regression workflow plus six-job Flake CI, H4 targeted/notified agent benchmarks and H6 allocation benchmark; **no production code changes**.

## Phase 4 — observed API identity distinction and pending gates

The initial run [#43417358615](https://github.com/matthis-k/phenix-ai/actions/runs/43417358615) correctly reached a live in-flight cancellation and rejected the old completion as \`HandoffError::ForeignCompletion\`, rather than the test's originally asserted \`StaleCompletion\`. This follows the actual \`ProcessHandoff::replacement()\` contract: replacement mints a **new owner identity**, while \`complete_job()\` searches for a live matching owner before invoking handoff completion. The old ticket is valid inventory of a retired owner, not of the replacement. \`StaleCompletion\` is instead returned when the *same owner* recognizes a stale step sequence. The test now asserts \`ForeignCompletion\` after replacement; no production behavior was changed. Fresh output and side-effect checks remain acceptance requirements.


Do not claim completion until the exact candidate-head 6-job Flake CI, dedicated repeated threaded workflow and existing regression workflows pass. On success, mark this scoped Q5/H3b *race* verified but keep **Q5 Open** (no systematic Loom model, external plugin lock-order proof, or non-revocable external effect policy); **H3b Partial**. Record one-decimal score changes in a separate traceable scorecard, not as objective measured percentage.
