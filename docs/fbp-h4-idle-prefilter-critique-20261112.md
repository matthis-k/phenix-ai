# H4 idle-input prefilter — separate Phase 2 adversarial critique

1. **Empty is not EOS.** An empty but closed queue must reach
   `selection`/`finish` so EOS can propagate and the graph can Complete.
   The guard checks `!writer_closed` for *every* queue, including fan-in.
2. **No imaginary data dependency.** Custom readiness can legally consult
   external state and be re-evaluated on an otherwise idle polling iteration.
   Never bypass Custom, including a dynamic waiting-policy override.
3. **Request-local closure.** A sealed request may prove absence without a
   closed physical writer. Therefore skip only if `self.requests.is_empty()`,
   not merely if `process.request.is_none()`.
4. **Zero input sources.** An inputless process/source may legitimately run;
   require a nonempty actual port/queue set and `!source`.
5. **Backpressure.** Pending publications may become runnable because a
   capture was drained, even with no incoming process packets. Require
   `pending.is_none()` and only consider Idle/WaitingInput states; existing
   publication readiness path stays unchanged.
6. **Queued readiness and fairness.** Do not drop a process already marked
   queued. The prefilter may be used only when `!queued`, and must leave
   ready deque order untouched.
7. **Cross-thread wake.** WaitingWake is excluded from the prefilter;
   sources are excluded even while Idle. Existing pending-bit swap remains
   in its original order.
8. **Measured vs guaranteed benefit.** A cheap O(V) scan remains.
   Even a significant 256-process benchmark speedup does **not** solve H4's
   asymptotic scheduling problem. Reject any claim of O(active) or global
   throughput gains based on this test alone.

Phase 2 acceptance is conditional: no new state and no new queue,
readiness semantics must be demonstrably unchanged by all existing tests,
and the benchmark must show material relative benefit. An isolated
readiness side-effect or EOS regression blocks acceptance, irrespective
of the benchmark numbers.
