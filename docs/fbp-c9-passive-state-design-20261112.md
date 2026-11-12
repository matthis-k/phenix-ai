# C9 — make native session state inspection passive

Status: Phase 0/1, 2026-11-12. Baseline `cbc98a438cf1938c098ba7707c856f4ce0b2b292`.

## Phase 0 — problem, invariant, existing contract

`NetworkSession::state(&mut self)` currently calls `refresh()`, which may:
invoke arbitrary `ReadinessPolicy::select` application code, reserve and
remove FIFO packets, resume waiting processes, close completed processes,
and report an input-policy failure. That makes a state query an *execution*
boundary, even when clients only want to render diagnostics, decide when to
poll, or check a request. User-owned policy code can run under the kernel
session mutex unexpectedly.

The actual need is a **passive snapshot of already committed scheduler
state**, not a second execution command. Public ingress, close, completion,
cancellation and `next_job` already call `refresh()` in their existing
paths; initial `NetworkSession::new` also performs initial refresh.

Non-negotiable: persistent queues, closure/EOS semantics, cancellation
accounting, fairness, and readiness policy evaluation *while deliberately
driving* must remain unchanged. No background worker, new scheduler, or
duplicated state index.

## Phase 1 — compare alternatives

| Choice | Strength | Weakness | Decision |
| --- | --- | --- | --- |
| Baseline `state(&mut self)` calls `refresh` | Immediate eager readiness | Read queries invoke plugin code and mutate transport | Reject |
| Keep active `state` and add `peek_state` | Backwards-compatible | Duplicates API; callers may still accidentally execute | Reject |
| Rename active method to `refresh_state` and expose passive `state` | Explicit commands | Adds a second public refresh verb not needed with existing drivers | Reject |
| **Make `state(&self)` a pure snapshot** | No implicit callback, clearer Rust borrow, minimal code | A new external atomic wake may not be reflected until next driver/refresh action | **Select** |

Only remove the `self.refresh()` call from `state` and relax receiver to
`&self`. `next_job` remains the driving boundary, and `run_until_idle`
keeps its current final-state reporting from `state`. Initial startup and
mutating ingress/closure operations already refresh. A wake notification
is a request to the executor, **not a promise that passive observation will
schedule the wake**. Do not materialize work in a read path.

## Phase 4 acceptance

Native regression: custom readiness policy increments a counter, and repeated
`state()` calls preserve that count without running callbacks; explicit
admission still performs refresh, and subsequent `run_until_idle` executes
correctly. Read-only `&NetworkSession` must be able to query `state()`.

All earlier EOS, request, and completion tests must pass without weakened
assertions. Verify strict formatting, clippy, full graph/kernel/storage tests,
feature variations, and final-head CI.
