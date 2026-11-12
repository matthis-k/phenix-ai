# C9 — adversarial critique of passive session state

Status: separate Phase 2 analysis before implementation.

1. **Is a read-only signature enough?** No. `state(&self)` must neither call
`refresh` nor indirectly invoke readiness, factory, dispatcher or graph
policy callbacks. Its code may inspect committed transport and handoff state.
2. **Is a newly pending wake visible?** An external wake may flip an atomic
flag but not yet be scheduled into `ready`. Returning a snapshot of the last
committed scheduler state is correct; documenting this is required.
3. **What about captured or queued packets?** Do not simplify `Waiting`
calculation or `is_completed` checks while changing the API.
4. **Can callers rely on state queries to drive EOS?** All existing mutating
close/completion/ingress paths and scheduler steps already refresh; test
that their outcomes are unchanged. If a test required hidden refresh, expose
the actual operation instead of moving side effects back into inspection.
5. **What about `begin_request`?** It checks `state` to reject completed
sessions; this remains correct because terminal and completed status derive
from retained queues, closed ingress and done processes.
6. **Does it fix general reentrancy?** Only one callback entry path is removed.
Other explicit mutations may still call custom policies synchronously, and
cross-thread lock wait cycles remain possible.
7. **Should a future public `refresh` method be added?** Not without evidence:
it would expose scheduling semantics the existing `next_job` already owns.

Conditional acceptance: minimal signature/call removal plus a strong
callback-counter regression and exact-head CI; no runtime behavior changes
beyond read-only observation.

## Phase 2 revision — wake status must be observable

The previous critique missed that a pending atomic wake is itself observable
readiness, even before the scheduler consumes it. A passive `state()` that
only reads the `ready` queue regresses the established wake contract.

The revised approach is accepted only if:
1. It checks the existing atomic pending bit for Idle source processes and
   WaitingWake continuations, without `swap` or `refresh`.
2. It does not treat an arbitrary non-source Idle process as schedulable merely
   because its wake flag is true.
3. Existing wake, closure, cancellation, and request tests remain untouched.
4. The new callback-counter regression does not assume startup eagerly
   selects with empty queues.
5. Exact-head CI demonstrates both state purity and wake visibility.

The resulting behavior is a read-only *up-to-date wake-visible snapshot*;
it still cannot guarantee scheduling of a concurrent event that arrives
just after the atomic load. This is a bounded, documented tradeoff.
