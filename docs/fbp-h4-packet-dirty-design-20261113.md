# H4 — conservative packet-driven dirty scheduler

## Phase 0: problem and invariants
Verified baseline: #1285 at `819a64d852c6178720d95f65397b51e612bd45f9`; six-job CI #43344454402 and paired H4/H5 benchmarks passed. Every `refresh()` scans all compiled processes even when only one packet path changed, so sparse packet-driven graphs are O(V) per step in addition to unavoidable `state()`/status scans.

Required invariants: deterministic ready queue and packet ordering, atomic fanout, bounded queue ownership, writer EOS, cancellation, deferred handoff, authentic request correlation, wake progress and all-optional zero-packet closure obligations. No compatibility shim or public API. Existing full scan is the correctness oracle.

## Phase 1: alternatives

| Option | Benefits | Risks |
| --- | --- | --- |
| No change | Proven, no state | O(V) every refresh |
| Cache negative readiness | Small initial implementation | Stale across injection, EOS, wake and scope closure |
| Universal persistent event index | Potential O(active) under all graph workloads | Must prove all request/wake/closure invalidations and scheduler fairness; large conceptual change |
| **Selective dirty process set with exact full-scan fallback** | O(affected indices) for packet-driven graphs while preserving full-scan semantics elsewhere | Additional BTreeSet and mutation hooks; fallback still O(V) for complex graphs |

Choose the last as an experiment, NOT a general event-indexed scheduler replacement.

## Eligibility gate
Use incremental scheduling for a session only if every compiled process:
- is a non-source with at least one declared input;
- uses packet-driven built-in readiness (Any, All, AtLeast or CorrelatedAll), not Custom or Optional;
- has not switched to staged `WaitInputs` dynamically.

Any `WaitInputs` disables incremental mode permanently for that session, before its next refresh. `WaitInput` is handled through its concrete physical port and explicit local dirtying. Optional closure and custom policies retain the unchanged full-scan path. A wake notification changes the global WakeSignal epoch and forces a full scan (no invented per-process wake tracking).

## Dirty transition inventory

- Startup: initial complete legacy scan.
- Accepted host ingress or close_input: mark the affected physical queue endpoints.
- Consuming a packet during `reserve`: mark its producer, since releasing capacity may unblock a retained publication.
- Successful fanout publication: mark each committed queue's reader, and mark publisher itself for continuation.
- Host capture consumption: mark its producer, which may now commit a blocked fanout.
- `complete` / `reject`: mark the owning process; `Finished` changes writer/reader EOS and requests a full scan to preserve closure propagation.
- Request cancellation/failure/restart: force full scan after disposing queues and before resuming. Sealing a request with no optional policy requires no new activation beyond already marked packets, but can conservatively full-scan.
- External WakeHandle or SessionNotifier notify: changed epoch forces full scan.

The old `refresh()` process body becomes a single `refresh_process(index)` routine invoked either over all indices (same order/repeat-on-finish behavior) or over the sorted dirty set. A dirty mark created while scanning may persist for the next `refresh()`; this must not lose progress and must not duplicate a reservation. Ready queue remains authoritative; dirty set never owns packets. No change to status/read-only inspection.

## Risk controls and acceptance
- Exact-head six-job Flake CI and all preexisting tests, including request/session isolation, cycles, staged wake, global EOS, bounded fanout, cancellation, recovery, idle request settlement.
- Add an explicit regression for a chain with a backpressured capture, then drain and retry while unrelated sinks remain idle; no lost packet or duplicate step.
- Paired same-runner interleaved H4 sparse, H5 closure and native benchmarks against exact #1285. Byte-identical example sources. Promote only if H4 sparse materially improves and no correctness or meaningful fan-in/chain regressions.
- Document that `run_until_idle().state()` and `request_status` still have O(V) passive inventory; this is not an O(1) per-request guarantee, not M7 completion and not an event-index for optional or custom policies.

## Accepted tradeoffs (protocol default)
An extra sparse dirty set per eligible session and extra endpoint invalidation calls are accepted only if benchmark evidence supports them; full-scan fallback intentionally remains for uncommon semantics. No global liveness or fairness claim. The full runtime must remain correct even if the index is not used.
