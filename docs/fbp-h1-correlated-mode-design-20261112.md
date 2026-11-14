# H1 Phase 0–1: bounded explicit correlated-join policy

Baseline: `296bedaa4ebabe139e90a965cd9026ca6d33cf32`. Selected alternative to the 2026-11-12 H1 diagnostic.

## Phase 0 — required behavior
Two physical FIFO inputs can be crossed:
`a = [A,B]`, `b = [B,A]`. A strict head-only `All` join cannot
complete either request. Without changing some ordering invariant, this is
mathematically impossible. The old, tested `All` guarantee and bounded
fanout must remain the default.

The explicit requirement is *a single shared process instance* that can
complete independently tagged requests in this situation; the existing
independent-session workaround does not provide shared in-process state.

## Phase 1 — comparisons and decision
- **No change:** preserves strict global FIFO; fails the shared-session case.
- **Implicit lanes or global head skipping:** hidden capacity/ordering changes
  for every graph; rejected.
- **Separate request-owned queues:** can work but adds a second buffering
  authority, capacity/fairness/closure semantics and object lifecycle; rejected.
- **Explicit `Readiness::CorrelatedAll`:** select an authenticated request's
  *earliest* queued packet at each required port, even when another request
  is ahead. The existing compiled bounded physical queue owns every packet;
  `VecDeque::remove` extracts by offset without extra storage. No packet is
  cloned or put into an unbounded waiting lane. Chosen for smallest new
  semantics and no extra resource-owner abstraction.

Rules: `Readiness::All` stays strict; `Readiness::CorrelatedAll` opts in
per process. Only packets with authenticated `Some(RequestId)` can be
selected behind an earlier packet. Unscoped packets remain head-only.
One request-local FIFO order per physical edge remains preserved: only the
first packet for the chosen request can be removed. Candidate requests are
visited in deterministic round-robin port/connection order; an arbitrary
upstream continually producing new requests may still cause starvation.
No general fairness or progress guarantee is claimed.

Atomic fanout/retained emissions, queue capacities, closed input semantics,
request cancellation/disposal, code leases and error ownership do not gain
a second path. `Readiness::Custom` remains strictly head-only; it cannot
smuggle out-of-order packets via ports. This mode costs O(total bounded queued
packets) to find candidates and first matches; do not enable it by default
or label it activation-scaled. H4/H6 remain open until measured.

## Phase 4 acceptance
1. Existing strict `All` crossed-head test stays blocked.
2. New `CorrelatedAll` crossed-head test drains both requests in one
   session with exact per-request captured values/settlement.
3. A canceled incomplete request drops only its own queued packets; another
   scoped request can still settle.
4. No queue capacity change and no separate buffering owner; static code audit.
5. Exact-head workspace CI, strict rustfmt/clippy and feature matrix pass.
