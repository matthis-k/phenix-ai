# C5 — explicit host-supervised deferred job watchdog

Phase 0/1, baseline `f317c79e3dda196d9f3b06011124ddfdd9f0d427`.

Problem: `DispatchResult::Deferred` transfers an uncloneable `ActivationJob`
to a worker, which may never return it. A lost job can leave a persistent
`NetworkSession` in Waiting indefinitely. Force-dropping runner-owned state
is impossible without a host cancellation contract and may invalidate
native code leases too early. The Kernel must not start hidden timer threads.

Compare:
- No change: unbounded waiting without attribution.
- Background timer inside graph: adds scheduler/thread/process preemption
  capabilities the kernel explicitly does not own. Reject.
- Force-reclaim worker job: violates exclusive process ownership. Reject.
- **Opt-in deterministic age supervision:** `SessionOptions::deferred_timeout`
  defaults to None. Record `Instant` when a physical `ActivationJob`
  is handed to the runner. Typed `overdue_jobs(now)` only observes; explicit
  `supervise_deferred(now)` invalidates tickets through existing session
  failure/cancellation, retaining external job ownership. `next_job` and
  `run_until_idle` check supervision during normal driver operation.

Semantics: an expired worker might still perform external side effects; a
returned late completion must fail as StaleCompletion. No global retry,
resurrection, blocking wait, automatic deadline thread or wall-clock time
in tests. A host that never drives or supervises can still stall; this is
an explicit supervision API, not an always-on daemon.

Phase 4: deterministic deadline test asserts typed ticket identity,
session failure, late completion invalidation, default disabled mode,
and no process ownership reclamation. Verify exact-head CI and prior
dispatch tests.
