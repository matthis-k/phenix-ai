# H4 — skip unrelated idle processes while requests are live

Date: 2026-11-13. Base: verified, ready PR #1279 at `3dbf8e540400cb678f6ca8fe7ee16b94139baeca`, six-job CI #43335296565.

## Phase 0 — problem and invariant

`NetworkSession::refresh()` scans all processes on every native wake/admission/complete. Existing `idle_inputs_cannot_progress(index)` avoids expensive `selection(index)` for idle, open, physically empty input queues **only when `self.requests.is_empty()`**. Admitting any scoped request disables this prefilter for *every unrelated idle process*, including processes that are not in any request's authoritative registered downstream `RequestScope.reachable` set. The resulting overhead is O(processes) readiness selection per event in a graph where only one scoped node is active.

Requirements: event and EOS semantics, custom policies, waiting staged input, request-local all-optional closure, multi-root provenance, source wakeups, retained pending publications, cross-request isolation, failure precedence, no new public API and no additional scheduling state. Preserve the original no-request fast path identically.

## Phase 1 — alternatives

| Option | Advantage | Risk/tradeoff | Decision |
| --- | --- | --- | --- |
| No change | No code change | Every live scoped request disables the existing verified no-packet prefilter globally | Reject |
| New global dirty/ready graph worklist | Can remove O(V) inventory scan | Requires new invalidation/provenance semantics for queues, wakes, closure/EOS and reload | Defer until H4 pinned workload profile |
| Build a per-request active-process bitmap for every refresh | O(1) lookup | Allocation and secondary scheduling state for a predicate already in scopes | Reject |
| **Extend the existing input-empty prefilter only for unrelated idle processes** | Reuses `RequestScope.reachable`, preserves all physical queue and custom policy gates | Extra O(active scopes × log V) membership checks; still scans process inventory | **Select** |

### Exact rule

Retain the original `self.requests.is_empty()` fast path unchanged, including its treatment of unscoped staged waiting input. In addition, when one or more request scopes exist, permit skipping `selection(index)` only if:
- the process is in **`HandoffState::Idle`** (not a staged `WaitingInput`, dispatched or other state),
- `p.request.is_none()`, `!p.source`, `!p.queued`, `p.pending.is_none()`,
- no active scope has `scope.reachable.contains(&index)` (including **unsealed** scopes and multiple roots),
- neither current nor waiting readiness is `Readiness::Custom`, and
- all process input queues are **physically empty and writer-open**, preserving EOS and correlated packet readiness.

A request-rooted all-optional downstream component remains inside R.reachable **even with no packet** and must remain evaluated for closure. A physically buffered queue is never skipped, regardless of scope identity; a closed writer is never skipped. Source wakeups and staged WaitInput are always evaluated in the new case.

No new mutable caches; no process is finished or marked runnable by this predicate. It is purely a safe negative readiness prefilter before the existing `selection`.

### Acceptance and measurement

- Add real persistent-session `fbp_scoped_sparse_benchmark.rs`: N independent open sinks, only one receives each sealed request. Assert exact one step, Settled(0), no capture, idle other branches, release, and Quiescent session across iterations.
- Compare exact pre-prefilter #1279 source and candidate with byte-identical benchmark on same GitHub runner, alternating three repetitions, sizes 1/16/64/256 (and optionally 512), record per-run means/p50/p95, raw CSV, CPU and Rust source/toolchain.
- Also run unchanged unscoped `fbp_benchmark.rs` and closure fan-in/fully-reachable chain as controls, to surface extra reachability-check overhead.
- Native tests for all-optional zero-packet closure, staged custom, cross-request pending, EOS and out-of-scope identity must pass alongside six-job exact-head CI.
- This is a **constant-factor** reduction of extra readiness selection. It does **not** remove O(V) scanning in `refresh()` or prove event-driven liveness; H4 remains Partial until bounded workload budgets.

## Accepted limitations

Extra per-node reachability membership checks can regress a fully reachable graph, and scanning multiple simultaneous requests has O(R log V) cost per empty process. Avoid arbitrary heuristic thresholds; use the simplest existing-set query and measure on sparse scoped versus fully reachable/multi-scope controls. If regressions outweigh benefits, reject rather than adding a global scheduler cache.
