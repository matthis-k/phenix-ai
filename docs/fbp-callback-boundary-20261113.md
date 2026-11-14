# FBP Callback Boundary Hardening

Status: design/history for the kernel callback-lock batch, now assembled on
`fix/fbp-lifecycle-request-lanes-20261114`. The starting checkout was
`impl/graph-consolidation-20261110`, not PR #1300's verified head; its existing
callback/correlation work is preserved. See the [campaign record](fbp-hardening-campaign-20261114.md)
for subsequent guard composition, timeout, lane lifecycle and acceptance details.

## Contract And Lock Order

Kernel synchronization is limited to ownership transfer and state publication:

| Boundary | Before | Enforced boundary |
|---|---|---|
| `Producer::poll` / `set_waker` | Producer-state mutex held across callback | Producer is exclusively taken from its slot; callback runs unlocked; state is reconciled afterward |
| `with_session` closure, dispatcher, guard, process, readiness, recovery | Session mutex held across runtime calls and callback | Session is exclusively leased out of `Mutex<Option<NetworkSession>>`; mutex is released for callback execution and restored by RAII |
| Producer event admission / readiness | Graph injection and its readiness callback ran under producer-state mutex | A producer admission permit is claimed against retirement, then session injection and readiness callbacks run without the producer-state mutex |
| Factory readiness/correlation declarations and request `closure_ports` | Factory methods were called during runtime construction/accounting | Factory declarations are panic-contained at construction; per-request closure ports are evaluated once, cached, and rejected before scope admission on panic |
| Re-entry through the same `Kernel` on the owning thread | Self-deadlock on the session mutex | Return a reentrant-session error before waiting |
| Process callback running on a dispatcher worker | Caller-thread-only re-entry tracking missed synchronous worker callbacks | Kernel session identity is installed by the existing `ProcessExecutionGuard` on the actual worker; caller-provided guards are composed |
| Resource `Store` read/write closure | Store's resource lock held | Remains a plugin/resource-owned critical section; callers must not re-enter the same resource from its closure |

Lock-order graph after the change:

```text
kernel producer mutex: claim/restore producer only; never surrounds plugin code
kernel session mutex: take/restore session only; never surrounds plugin code
exclusive ownership leases: span callbacks but are not locks
  -> same-kernel calls on the callback thread or a dispatched process worker fail
     fast instead of waiting for that callback's own session lease
plugin/resource locks: remain owned by their API and are not ordered by Kernel
```

Producer callbacks remain serialized per kernel. A recursive/concurrent `drain`
while another drain or wake registration owns the producer callback lease returns
no events rather than waiting. A producer callback panic is contained: poll panic
is a terminal `ProducerError::Poll`; wake-registration panic is a `DriverError`.
The producer mutex remains usable afterward.

If publication retires a generation during `poll`, the old callback is allowed to
finish, but its event is not admitted. Retirement does not forcibly stop external
work already started by a producer. The local producer and its code remain owned
until the callback returns, and are dropped outside the producer mutex.

An event that has already claimed its admission permit may finish the session
injection while publication proceeds; publication does not wait for arbitrary
readiness code. This permit is the admission linearization point. A poll result
which returns after retirement cannot claim a permit and is discarded. This
distinction avoids holding the producer mutex across readiness callbacks without
claiming that retirement can preempt an admission already in flight.

## Design Alternatives And Independent Critique

- **No change:** retain the producer mutex across injection. Rejected because
  admission can invoke a custom readiness callback that re-enters `drain`, creating
  a same-thread lock cycle.
- **Smallest extension:** move injection outside the producer mutex after checking
  `retired`. Rejected because publication can win between the check and injection,
  making the stale-admission boundary ambiguous.
- **Implemented:** claim an admission permit while checking the producer slot and
  retirement state, release the mutex, inject under the exclusive session lease,
  then reconcile. A successful permit is ordered before retirement; a late poll
  cannot obtain one. No new scheduler or lock manager is introduced.
- **Alternative redesign:** run callbacks on a dedicated executor or serialize all
  lifecycle work through an actor. Rejected because it adds a second execution
  authority and still cannot preempt arbitrary callbacks or external effects.
- **No change for worker re-entry:** keep only thread-local guards on the caller.
  Rejected because a synchronous dispatcher can join a process worker which calls
  back into the same Kernel. The existing process execution guard now propagates
  the stable session identity to that actual worker; arbitrary threads spawned by
  unrelated plugin callbacks remain outside this propagation contract.

An independent adversarial review identified and drove fixes for: readiness calls
under producer lock, worker-thread session re-entry, invalid custom closure-port
names, invalid built-in readiness wrapped as custom correlation, and panic payload
destructors that panic while being discarded. Regressions now cover the first four
and both poll/wake panic-payload destruction paths. The review also found that
`distinct_scope_skew` does not bound repeated packets from one request. The
correlation API now states its restricted one-packet-per-input progress envelope,
and a negative test records strict FIFO head blocking outside it; no arbitrary
arrival-skew or fairness guarantee is claimed.

A second independent diff review caught two producer-loop edge cases: removing a
terminal last-slot producer could leave the round-robin cursor out of range, and
`drain(0)` briefly claimed callback ownership despite doing no poll. Cursor removal
now selects the shifted next slot or wraps to zero, with a last-slot regression;
zero-budget drain returns before taking the callback lease.

## Alternatives And Adversarial Review

- Keeping callbacks under locks and documenting non-reentrancy was rejected: the
  public callback traits cannot enforce that rule, and same-thread re-entry
  deadlocks synchronously.
- A generic lock manager, second scheduler, or callback executor was rejected:
  exclusive ownership transfer is sufficient and retains the existing runtime.
- Taking the session out without a reentrancy check was rejected: same-thread
  `with_session`/`inject` re-entry would wait forever for its own lease. The
  reentrancy check is thread-local and scoped to the exact `Kernel`, so nested use
  of an independent kernel remains allowed.
- Releasing producer ownership without reserving the slot was rejected: concurrent
  drains could poll the same non-`Sync` producer or duplicate its pending event.
- Letting an old poll publish after retirement was rejected: publication is the
  producer-admission retirement point. Its already-running external side effects
  are not revocable, but the returned packet cannot enter the retired graph.
- Arbitrary plugin-vs-plugin lock cycles remain possible, including inside
  `ProcessContext::read/write` resource closures. Kernel lock freedom is not a
  claim of universal callback deadlock freedom.

## Acceptance Evidence

Regression coverage must prove poll/set-waker callbacks can re-enter without
holding the producer mutex, process callbacks can attempt same-kernel session
re-entry without hanging, callback panics do not poison kernel mutexes, external
session users serialize while a callback owns the session lease, and generation
publication during a blocked poll rejects its stale event. Readiness-factory,
correlation-bound, and request closure-policy panics must become deterministic
failures without partially admitting a request. Existing request, ticket,
notification, and lease identity behavior must remain intact.

The current worktree additionally tests admission callbacks outside the producer
lock, same-kernel re-entry from a dispatched worker, invalid closure declarations,
readiness-wrapper validation, and destructor-panicking panic payloads. These are
candidate-local regressions until a committed exact head passes CI.

The implementation must pass formatting, focused kernel/graph tests, relevant
bounded Loom tests, Clippy, and the repository's six-job Flake acceptance on the
exact committed head. No performance improvement is assumed; lease acquisition
and condition-variable wakeup overhead must be measured before changing any
performance score.

## Candidate Microbenchmark

Historical measurements from the inherited worktree, not rerun for the final
assembled candidate. They cannot establish its performance acceptance.

`phenix-kernel/benches/callback_lease.rs` runs the same optimized, setup-excluded
100,000-call / seven-sample harness on clean starting commit `16e77f6c6` and the
candidate. Median nanoseconds per call were:

| Operation | Starting commit | Candidate | Difference |
|---|---:|---:|---:|
| `with_session` no-op | 639 | 757 | +118 ns |
| `run_steps(0)` | 34 | 100 | +66 ns |
| `drain(0)` | 23 | 39 | +16 ns |

These tiny-operation results are sensitive to host scheduling and do not measure
real process throughput or allocation. The first candidate iteration paid an
unconditional empty `Condvar::notify_all`; the session lease now notifies one
waiter only when a waiter exists. The remaining absolute overhead is bounded to
two ownership-transfer mutex acquisitions and thread-local reentrancy tracking;
the no-op `run_steps` relative percentage is misleading, and H6 remains open for
representative end-to-end profiling.
