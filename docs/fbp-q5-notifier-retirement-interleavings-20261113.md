# Q5 — notifier retirement, wake listener, and request-recovery interleavings

Date: 2026-11-13. Baseline: verified measurement-only [#1294](https://github.com/matthis-k/phenix-ai/pull/1294) at `89e48b896e9687ffbb8bb92abde621d7bbd74d97`; exact-head [Flake CI #43406458212](https://github.com/matthis-k/phenix-ai/actions/runs/43406458212), [H6 allocation #43406457667](https://github.com/matthis-k/phenix-ai/actions/runs/43406457667), and [H4 mixed-agent #43406457743](https://github.com/matthis-k/phenix-ai/actions/runs/43406457743) passed. [#1295](https://github.com/matthis-k/phenix-ai/pull/1295) is **rejected and unmerged** due to measured EOS/H5 regressions; its performance code is not included in this branch.

## Phase 0 — problem and baseline test coverage

`WakeSignal::notify_target` and `retire_target` share a mutex after #1292, so a stale process notifier should either:
- publish before retirement, where retirement removes its queued target and pending `WakeHandle` flag; or
- see retirement first and return `false` with no scheduler side effect.

`RestartAffectedProcess` reuses the same compiled process index with a **new** `WakeSlot`. Concurrent notifications from a retired handle must never become wake authority for the new request. `WakeListener` separately uses an epoch latch around condition-variable waiting to avoid missing a notification between registration and blocking.

Existing tests include sequential cancellation/recovery with a stale notifier and 64-process/8-thread targeted notification coalescing. They lack a test intentionally racing the *old* notifier and wake handle with the exact request-cancellation/replacement boundary. Q5 is still open; no exhaustive external lock-order or Loom proof exists.

## Phase 1 — alternatives

| Option | Benefit | Cost/limit | Decision |
| --- | --- | --- | --- |
| Rely on existing concurrent notification stress | No changes | Doesn't race retire and process-index reuse | Insufficient |
| Add a new runtime test hook to pause inside `notify_target` | Could force precise midpoint | Production seam for tests; changes synchronization surface | Reject |
| Build independent mocked protocol state machine | Explore schedules | Risks proving a model that differs from implementation | Defer until actual model checker is justified |
| **Exercise actual `WakeSignal` and compiled `NetworkSession` through barrier schedules** | Tests real lock, epoch, cancellation/reuse, no runtime API change | Finite schedules and thread execution; not exhaustive | Select |

## Phase 2 — adversarial invariants and limitations

1. A notification linearized before retirement may return `true`; that is **not** a promise that the old process ran. After retirement, the old notifier/wake must always return `false`, no stale pending target survives, and newly installed slot at the same index remains independent.
2. Both `ProcessNotifier::notify()` and `WakeHandle::wake()` must race recovery; only the latter can resume `WaitWake`, and neither may wake an unrelated/new request after cancellation.
3. `WakeListener` must observe process-only notification, process wake, global notification, and terminal invalidation when racing registration-to-wait. A bounded timeout is a failure guard, not the mechanism that schedules ordering.
4. The request test uses three schedules: old calls before cancel, after cancel, and barrier-released concurrently with cancel. Repeating 96 rounds in one persistent compiled session stresses process-index reuse and request release. The regression asserts one real owned process step into `WaitWake`, then exactly one **fresh** wake to resume the replacement. Count outcomes, not whichever thread wins the unconstrained race.
5. This is **bounded concurrency stress**, not model checking or comprehensive deadlock freedom. External plugin mutex inversions, handoff job effects outside the session, scheduler fairness and native plugin thread safety remain Q5/H3b scope gaps. Avoid describing this as a full Q5 resolution.

## Phase 3 — implementation and CI acceptance

- Add private `wake.rs` unit tests for 128 start-barrier races of `notify_target` vs `retire_target`, both flags and reuse of the same process index, and for 24×4 listener registration/wait races across notification mechanisms.
- Add public-API compiled-session regression test `fbp_notifier_retirement_interleavings.rs`, covering 96 request cancel/recovery rounds and the three controlled schedules; verify fresh-only continuation, typed `Pending/Cancelled/Settled`, no phantom wake or extra completion, and successful scope release.
- Dedicated GitHub Actions workflow repeats the integration and internal wake tests 8 times with exact source/commit/compiler provenance. Exact-head six-job Flake CI and all previously accepted H4/H6 benchmarks are the final gate.
- No changes to production runtime behavior, synchronization structs, API or scheduler.

## Phase 4 — pending results

Record the precise head SHA and green/failing CI after tests execute. Passing finite barriers verifies only tested interleavings; Q5 remains **Open with stronger scoped evidence** until model-checking/lock contract acceptance. H6 remains Partial, H7 remains Partial, and M7 is still incomplete.
