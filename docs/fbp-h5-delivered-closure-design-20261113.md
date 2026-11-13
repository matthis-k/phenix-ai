# H5: skip already-delivered request-local closure inputs

## Phase 0 — problem and baseline

Exact baseline: `64d3f9306633f57f098f4a8ce13485c3ab5bc5d8` (verified H4 PR #1281). On a sealed request, `request_input_closed` checks upstream queues, process ownership and all-optional obligations with a DFS. In a deep chain of all-optional closures, each consumer first receives an authenticated absent-input closure, executes, and completes; the next consumer then rewalks the already-closed upstream prefix. Measured 128-node chain completion remains ~5–8 ms/request, depending on runner. The experiment in #1282 shows that replacing the DFS visited set does not solve this.

Target: remove redundant *already-delivered request-local closure* ancestry work without changing the graph, transport, external APIs, or general cache invalidation rules.

## Phase 1 — comparison

| Option | Advantage | Problem |
| --- | --- | --- |
| No change | No new concepts or risk | Deep-chain repeated proofs remain |
| Cache arbitrary positive/negative closure results | Can prune all repeated proofs | Requires validity epochs, mutable caches, memory budget and multi-request lifecycle review; negative witness #1280 was rejected |
| Compute a full closure snapshot each refresh | One topological analysis per refresh in DAG cases | Cycles, request scope, queues, in-refresh mutations, and O(V) work even for sparse scopes |
| Reuse *delivered closed-input evidence* only | Trusts an event already validated by the scheduler; no speculative proof cache | Adds one per-request sparse set; helps only after an actual closure delivery; does not fix first proof or O(V) refresh |

Choose **delivered closed-input evidence** as the minimal behavioral extension.

## Proposed implementation

Add `RequestScope.delivered_closures: BTreeSet<(usize, usize)>`. Only `reserve()` records a port when it actually reserves `None` for a *specific sealed request* and the port is not globally at EOS. `selection` has already validated `input_closed_for` immediately before reservation, so this is authenticated **closure**, never a packet/data observation. Keep `observed_inputs` unchanged: it intentionally includes both packets and closure to suppress duplicate optional activations.

Inside `request_input_closed` DFS, after verifying a producer is not active for this request and has no outstanding all-optional closure obligation, skip only that producer's **previously delivered closed input ports** when traversing its predecessor queues. Continue scanning every other port, every target input's direct queues, other ancestors, and all reachable owners/queues. Retain the visited set and scope-based pruning. Never infer closure from a mere observed data packet, a different request, or a producer with an in-flight job.

## Invariants / monotonicity argument

- A request-local absence is supplied only once the request is sealed and the entire relevant transitive upstream has no packet bearing this request, no active request owner, and no outstanding request-local closure obligation.
- After seal, no new request ingress or request-owned source start is admitted. An upstream process cannot spontaneously produce a packet for that already-closed request without an earlier upstream request-owned activation (which the proof would have rejected).
- An independently admitted request may later execute any of these processes, but cannot create packets for the earlier request ID. Evidence is indexed by authenticated `RequestId`, and is discarded on request release.
- A graph cycle is traversed during the original proof. An outstanding process owner or closure obligation anywhere on that cycle prevents closure; the recorded input remains closed for that request once actually delivered.
- If a producer has some closed inputs but other active inputs, only the proven-closed ports are skipped. Processing its own active owner and remaining queues precedes/retains these checks.
- `state(&self)` and externally visible status inspection stay passive. No interior mutability, global cache, or publication buffering.

## Phase 3 intended tests and measurement

1. Existing multi-root, deferred, staged input, replay, shared request, cancellation, EOS, and cycle tests must pass unchanged in exact-head six-job CI.
2. Add explicit regression with repeated sealed request-local absence and a later separate request through the same upstream prefix, including bounded and long chain sizes.
3. Compare byte-identical existing H5 closure and H4 sparse benchmarks against **exact H4** `64d3f930...` on same runner, 3 interleaved repetitions, retaining CPU/environment and CSV evidence.
4. Reject on any changed request semantics or substantial synthetic fan-in/sparse regression.

## Acceptance / limits

A scoped speedup is not universal H5/M7 acceptance. Worst-case first-time closure still traverses upstream, `refresh()` still inventories all processes, and long cyclic/fan-in graphs can still require repeated work. Per-request delivered-closure set grows no larger than actually delivered absent-input closure events; no unbounded per-attempt speculative cache. Static liveness and global fairness remain separate issues.
