# Q5 — Exhaustive serial wake-operation orders against the real lock

Date: 2026-11-13. Baseline: exact-head-verified [PR #1297](https://github.com/matthis-k/phenix-ai/pull/1297) at `2064a1a3a713d635dfec33dac72f669cfe026473` (six-job [Flake CI #43418004760](https://github.com/matthis-k/phenix-ai/actions/runs/43418004760) and 12-repeat [in-flight cancellation #43418004098](https://github.com/matthis-k/phenix-ai/actions/runs/43418004098) green). Draft/unmerged stack. No production runtime change is proposed.

## Phase 0 — Problem and assumption test

After #1292, `WakeSignal::notify_target()`, `retire_target()`, `notify()`, `invalidate()`, and `take_scheduler_wakes()` all serialize state through one `Mutex<WakeState>`. The Q5 #1296 and #1297 tests cover real threaded races and in-flight worker cancellation/recovery but cannot guarantee that the rare ordering of terminal invalidation, target retirement and scheduler drain was exercised. The smallest additional evidence is not another random stress loop; it is exhaustive testing of the **serial operation orders** these lock-mediated actions can have.

## Phase 1 — Candidate approaches

| Approach | Benefit | Limitation | Decision |
| --- | --- | --- | --- |
| More thread/barrier repetitions | Real scheduling races, no dependency | Still samples unknown subset of orders | Already covered; insufficient alone |
| Add Loom to production `wake.rs` sync types | Instruction-level state-space exploration | Needs Cargo.lock changes, cfg/sync abstraction, and independent modeling of worker/plugin locks | Separate future Phase 1, not justified for one locked core |
| Fake a standalone state machine only | Exhaustive over model | Cannot detect implementation discrepancies | Reject |
| **Enumerate all operation-level histories on actual `WakeSignal`, checking each intermediate state against a separate model** | Finite, reproducible, directly tests production implementation including terminal, global notify, reused target and drain | Only exhaustive at the **public/private operation order**, not Rust memory instruction scheduling | Select |

## Phase 2 — Adversarial limitations and acceptance

- Enumerate the 8 unique events `OldNotify`, `OldWake`, `Retire`, `FreshNotify`, `FreshWake`, `Take`, `GlobalNotify`, and `Invalidate`. Allow every order satisfying `Retire < FreshNotify` and `Retire < FreshWake`. Total exactly `8! / 3 = 13,440` histories, with no RNG, sleeps or assumed winner.
- Match each **actual** `WakeSignal` state after each event to independently updated reference flags: active, global and listener epoch, queued target, pending and active old slot, pending new slot. Validate results from target/global notification and scheduler drain, not only final state.
- `retire_target` always precedes fresh-slot use in the same compiled index by the recovery contract. If a future code path permits installing or targeting a fresh registration before old retirement, this test's admissibility rule must be revisited.
- `Invalidate` terminates the session; no notification afterward may be accepted. An already queued global epoch remains readable, and invalidation clears targets. A slot's raw `pending` atomic may remain true after terminal invalidation but it can no longer authorize a live notification. This is not a memory-reclamation claim.
- `WakeListener` must observe invalidation via its latch, even if epoch is unchanged.
- The test is a finite **sequential linearization-order checker**; it is **not Loom**, no exploration of mutex/condvar instruction interleavings, no arbitrary external plugin locks, no proof of fairness or deadlock freedom. The separate #1296 actual threaded races, and #1297 worker-ownership races, are still needed. A passing CI does not close Q5.
- Strict acceptance: compare implementation against expected state *on every transition*, require exactly 13,440 checked orders, green exact-head 6-job Flake CI, eight-repeat existing Q5 workflow and no regressions in H4/H6 workflows. Do not add a production synchronization layer solely for this test.

## Phase 3 — Implementation

A test-only event enum, reference-state structure and in-place exhaustive permutation generator live inside `runtime/wake.rs`'s existing unit tests. Use the real `WakeSignal`/Condvar/atomic slots, no duplicated runtime code and no production-facing methods. Existing `fbp-q5-notifier-retirement-concurrency.yml` already runs all `runtime::wake::tests::` eight times and triggers on this file. Document source SHA and exact-head CI once green.

## Phase 4 — Pending evidence

No score increase is accepted until CI passes on the exact commit containing the final doc and scorecard. If it passes, a cautious subjective Q5 estimate could rise **65.5 → 67.0 (+1.5 pp)**, with H3b **74.0 → 74.0 (0.0 pp)**; this provides *specific* wake operation-order assurance rather than new handoff/reentrancy capability. All 33 issue values remain traceable in the scorecard. Q5 remains Open, M7 incomplete.
