# H4 hybrid dirty scheduler — Phase 2 adversarial critique

Reviewed [revised Phase 1](fbp-h4-hybrid-dirty-design-20261113.md) separately from code.

1. **Mixed zero-packet closures:** an all-optional downstream node must be evaluated after a request seals and after an upstream process stops owning that request—even with no physical packet. Preserve it in the always-poll set for every `refresh()`; never rely solely on queue events.
2. **Custom readiness:** a user policy may decide to select based on external state changes without a queue event. Always-poll custom nodes on every refresh, not only after wake notifications. Custom code/policy can panic; retain existing error containment and stop scan on recoverable errors.
3. **Staged local closures:** `WaitInput` and `WaitInputs` can depend on request-local upstream absence. Add the process to always-poll before the next refresh, without disabling all other nodes. Once promoted, never demote; this avoids validity-state maintenance.
4. **Sources / no-input:** compiled source nodes and zero-input nodes may start without ingress. Always-poll them and handle wake epoch; don't infer quiescence from an empty dirty set.
5. **Multiple request roots:** sealing must mark the union of registered request reachability (already maintained by `note_request_root`), not only the last injected ingress's consumers. All-optional `closure_obligation` must still block if any root has outstanding work.
6. **Concurrent requests:** another scope's queued work must never be treated as the sealed request's packet or closure proof. Existing `RequestId` metadata and conservative upstream DFS stay unchanged.
7. **Wake race:** keep the pre-scan epoch snapshot and full-scan fallback on observed change. A wake during the pass stays visible to the next refresh.
8. **EOS / backpressure:** preserve all queue-endpoint invalidations and full legacy repeat-on-finish. Reader pop/capture drain must dirty the pending producer. An unconsumed lossless capture cannot be promised fair independent progress.
9. **Passive `state()`:** no readiness callbacks or poll-index mutation from an observer. Always-poll applies when driving `refresh()`, never on `state(&self)`.
10. **All-special graph:** `always_poll.len()==processes.len()` must delegate directly to the exact legacy full scan and avoid dirty-set work. This is the predictable O(V) lower-bound for the conservative policy.
11. **No false completion:** a request seal may be observed by a globally idle optional consumer not in that request's reachable set; `closure_obligation` rejects it. Skipping that node after seal is safe; do not invent packet provenance.
12. **Cache/lock tradeoffs:** no persisted negative readiness memo and no global mutable closure cache. BTreeSet union cost may outweigh benefits for small graphs; measure 1/16/64/256 and reject unproven speedups.

**Conditional Phase 3 acceptance:** one shared readiness body, no public API, fallback audit, exact code tests and benchmarks. Re-check the accepted limitations in Phase 4. If the mixed graph is not faster or if all-special chains regress materially, reject rather than add bespoke caches or hidden state.
