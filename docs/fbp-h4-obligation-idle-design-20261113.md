# H4: obligation-aware idle readiness prefilter

## Phase 0 — problem, baseline and assumptions

Base: verified H4 sparse-scoped prefilter `64d3f9306633f57f098f4a8ce13485c3ab5bc5d8` (PR #1281, six-job CI #43338907783, paired H4 #43338907011). It skips provably idle open sinks **only when outside every active request's reachable set**. An all-reachable scope globally disables the skip via `all_reachable`, even for processes whose readiness cannot produce a reservation. Every `refresh()` then allocates/selects candidates across many physically empty processes.

The rejected positive closure evidence experiment (#1284) left 128-node chain latency ~7.94 ms unchanged, indicating that scheduler selection, not duplicated DFS, dominates this workload. H4 still scans O(V) process inventory; this proposal optimizes that per-process selection cost, not the scan.

## Phase 1 — options

| Option | Effect | Limit |
| --- | --- | --- |
| Do nothing | Safest | Repeated needless readiness selection for reachable non-eligible processes |
| Event-driven readiness index | Avoid O(V) scan | New persistent scheduler state, invalidation and fairness proof across packet, EOS, wake, custom readiness, pending publication and scoped closure |
| Cache readiness failures | Less repeated work | Invalidations difficult on every input, scope and wake change; per-request state and negative-result staleness |
| Tighten existing proven idle guard by evaluating actual outstanding closure obligations | No new state/API; skip only demonstrably impossible reservations | O(V) inventory remains; candidate scanning persists for genuine all-optional obligations |

Choose the existing guard extension. No new graph concepts or API.

## Proposed invariant-preserving predicate

A process can skip `selection` only when:
- It has no source/wake-driven autonomous start, queued runnable item, or retained publication.
- It has at least one physical input; every physical input queue is **empty and writer-open**.
- Its configured readiness policy is non-custom (including no staged/custom override).
- If any scope is live, the handoff is exactly `Idle`, no request owns the process, and no staged input/policy is waiting.
- **No active scoped all-optional closure obligation exists at that process.** For policies without an all-optional zero-packet path, skip checking scope reachability entirely. For all-optional policies, use existing `closure_obligation(index, id)` across registered requests. This uses the same `scope.reachable` and `observed_inputs` semantics as `selection`.

Anything that can run without physical packet data, including an unobserved all-optional scoped closure, stays on the full selection path. If a request is newly rooted, sealed, or released, the next `refresh()` reevaluates the predicate against current state. The original no-request behavior is retained.

Remove the now-unneeded `all_reachable` special case. No new cache, mutable index, authoring API, compatibility shim or privilege change.

## Acceptance and evidence

- Existing sealed all-optional zero-packet chain, multi-root, shared request cancellation, staging/custom, global EOS, independent live scopes, source/wake and backpressure regression tests must pass.
- Add a targeted regression in the H5 closure benchmark example: multiple sequential sealed requests execute the full 64-process optional chain, exactly once per node per request, even though every node's input is physically empty/open. Existing synthetic benchmark performs the same assertions for each iteration.
- Run all-six exact-head Flake CI and a byte-identical paired H4/H5 benchmark on one runner with three interleaved repetitions, including 1/16/64/256 scoped sparse, 1/16/64/256 unscoped, fan-in and 16/64/128 reachable chain.
- Reject if the all-optional closure is skipped, if regressions fail, or if benchmark results are worse without meaningful benefit.

## Accepted limitations

Per-call `refresh()` still scans all compiled processes O(V); large entirely ready or all-optional graphs still require actual readiness checks. No static liveness, fairness, held-job cancellation or production-agent throughput budget is claimed. These are independent M7 acceptance items.
