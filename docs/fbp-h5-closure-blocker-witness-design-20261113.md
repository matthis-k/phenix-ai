# H5: bounded revalidated witnesses for blocked request-local closure

Date: 2026-11-13. Base: ready, exact-head-verified [PR #1279](https://github.com/matthis-k/phenix-ai/pull/1279), `3dbf8e540400cb678f6ca8fe7ee16b94139baeca`, [six-job CI #43335296565](https://github.com/matthis-k/phenix-ai/actions/runs/43335296565), [paired benchmark #43335296274](https://github.com/matthis-k/phenix-ai/actions/runs/43335296274).

## Phase 0 — observable problem and invariant

`request_input_closed(index, port, R)` is a pure proof for a **sealed** request: it returns false if a directly targeted request-owned packet remains buffered, or if *any* reachable upstream producer is actively assigned to R, owes an all-optional closure for R, or has an input queue holding R. The implementation performs a fresh upstream DFS with an allocated visited set on every evaluation.

Deep all-optional closure chains repeatedly query *blocked* downstream ports while the same ancestor remains active. PRs #1276/#1278/#1279 removed packet scans, unrelated branches, and duplicate checks inside one readiness selection, yet a 128-node fully reachable chain still costs ~5–8 ms per request on shared CI runners. The repeated negative proof is the next avoidable cost.

Requirements: exact request ownership, FIFO/correlated selection, no false early closure, cycles, multiple roots, deferred/retained publication, cancellation/release, custom readiness, no new public API, no hidden per-request payload buffer or scheduler privileges. Preserve precise error/failure precedence.

## Phase 1 — baseline, extension and proposed approach

| Option | Strength | Cost or risk | Decision |
| --- | --- | --- | --- |
| Keep full fresh DFS | No derived mutable state | Repeatedly searches the same live blockers | Reject for this scoped subproblem |
| Cache `true` closed proofs forever | Can skip successful historical closure | Requires proving monotonic closure under cycles, custom wakes, late work and restarts | Reject initially |
| Cache `false` result permanently | Fast | **Incorrect**: a queued packet can be consumed or an upstream optional obligation discharged | Reject |
| Cache entire transitive closed/open mask per refresh | Reuses proofs in one refresh pass | New global dependency cache with subtle invalidation on reserve, EOS, cancel and process restarts | Defer |
| **Retain one concrete blocker witness per request-input pair, revalidate on every use** | Sound short-circuit while the blocker remains, no revision/epoch and no change to actual closure proof | Bounded per-scope map and tiny witness lookups; requires interior mutability inside `selection(&self)` | **Select** |

### Exact mechanism

Add a **private** `BTreeMap<(process_index, input_port_index), ClosureBlocker>` inside each `RequestScope`, held in a `RefCell` because `selection` is an immutable observation of session state. `ClosureBlocker` holds only:
- `Process(index)`: observed upstream `p.request == Some(R)` or `closure_obligation(index,R)` when the DFS returned false; or
- `Queue(index)`: observed `ConnectionQueue.request_count(R) != 0` at a physical, request-relevant upstream or direct ingress queue.

For **sealed** R only, check a prior witness before starting DFS:
- if its actual predicate still holds, return `false`, exactly as a fresh DFS would;
- if not, run the complete existing relevance-pruned, cycle-safe DFS, and update the witness to whichever concrete blocker is found;
- if DFS proves closure `true`, clear any stale cached witness and return `true` **without** caching the result.

The data is not proof authority: the actual process or queue state is always re-read. The key space is bounded by **compiled process input ports per active request**; the entries never own payloads or leases, and are automatically dropped with `RequestScope` on release/recovery. Scopes are already bounded by `max_requests`. Do not invent a custom invalidation revision or retain witnesses across request identities/generations.

Use `std::cell::RefCell` only for the bookkeeping map. Release its borrow **before** calling `closure_obligation` or walking the graph to avoid reentrant borrows. Do not change `NetworkSession` public mutability or trust boundaries.

### Tests and measurement

- Repeatedly check an optional closure blocked by a wake-deferred same-request upstream process; wake, complete and prove eventual settlement without stale negative cache.
- Block on committed queued request packets while another activation is in flight; consume or explicitly cancel, prove witness invalidates; retain foreign-request work unaffected.
- Prove multi-root same-request inputs block closure until all work completes; cyclic/diamond and deferred cases are covered by full graph regressions.
- Build byte-identical `fbp_closure_benchmark.rs` against exact pre-witness #1279 and the candidate on one runner. Use existing paired fan-in, 16/64/128 all-reachable chain, and H4 sparse scheduling control. Record per-run means/medians and CPU identity. Report any shallow or fan-in regressions instead of only the best chain result.
- Full exact-head six-job Flake CI and independent Phase 4 diff audit required before ready for review.

## Accepted tradeoffs and limitations

The map is an optimization hint, not a second ownership ledger: its entries cannot revoke or forge a packet, work item or request scope. Negative results are **never** trusted after the witness predicate stops holding. We do not assert monotonic true closure and avoid tricky invalidation across dynamic dispatch/cancellation/restarts. The memory overhead is proportional to actually inspected process input ports in currently live scopes; per-query `BTreeMap` lookup and `RefCell` checks can regress small/simple workloads.

**H5 remains partial** even if measured chain latency improves: initial DFS still needed, and `refresh()` still performs O(process inventory) work; H4 and M7 are incomplete.
