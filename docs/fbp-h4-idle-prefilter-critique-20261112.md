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

## Phase 4 counterexample — readiness without a candidate

Initial dedicated integration regression incorrectly assumed `Custom.select`
must execute again when **every queue is empty and open**, and asserted a
callback-count increase on `run_until_idle`. In this runtime, the native
readiness engine invokes a custom policy only for an actual candidate
packet or an outstanding request-closure obligation. An empty graph has
neither; the callback is not owed a pass.

CI [#43203931274](https://github.com/matthis-k/phenix-ai/actions/runs/43203931274)
caught that mistaken test oracle, not a production scheduler regression.
Revised fixture first injects one typed packet while `Custom.select`
intentionally returns no ports. It then enables the policy through a
shared atomic flag, without another admission, and checks that the
next driver pass selects and consumes the *same retained packet*.
The separate default-policy fixture still proves open-empty quiescence
and closed-input EOS. No readiness callback was made mandatory on an
empty graph, and no application-facing semantics were altered.

Exact-head CI and benchmark after this correction are the final
acceptance gates. Do not conceal genuine incomplete work by merely
removing the assertion; the test must positively prove a legal
candidate-dependent recheck.
