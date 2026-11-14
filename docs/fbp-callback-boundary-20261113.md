# FBP Callback Boundary Hardening

Status: reconciled callback contract for the verified-stack merge on
`fix/fbp-lifecycle-request-lanes-20261114` (PR #1302). The initial `0d145f3`
candidate incorrectly relied on stale local remote-tracking impl head `16e77f6c6`;
actual remote impl `89e00d0` already had later foundations. After fetching, exact
verified PR #1300 head `516e0883875d5ec483e3595d50b77b3d5e8979c4` is now
being merged, preserving all scoped gates. See the
[campaign record](fbp-hardening-campaign-20261114.md) for provenance, guard
composition, timeout, lane lifecycle and pending new-head acceptance. The
[tracker](fbp-issue-progress-estimates-20261113.md) alone owns current scores.

## Contract And Lock Order

Producer/session inventory mutexes cover ownership transfer, not plugin code.
The separate verified whole-drain gate still spans polling and eager admission:

| Boundary | Before | Enforced boundary |
|---|---|---|
| `Producer::poll` / `set_waker` | Producer-state mutex held across callback | Producer is exclusively taken from its slot; callback runs without the inventory mutex, under the whole-drain gate; state is reconciled afterward |
| `with_session` closure, dispatcher, guard, process, readiness, recovery | Session mutex held across runtime calls and callback | Session is exclusively leased out of `Mutex<Option<NetworkSession>>`; mutex is released for callback execution and restored by RAII |
| Producer event admission / readiness | Graph injection and its readiness callback ran under producer-state mutex | Whole-drain retirement gate retained; injection and readiness run without the producer inventory mutex, using nonblocking session access |
| Factory readiness/correlation declarations and request `closure_ports` | Factory methods were called during runtime construction/accounting | Factory declarations are panic-contained at construction; per-request closure ports are evaluated once, cached, and rejected before scope admission on panic |
| Re-entry through the same `Kernel` on the owning thread | Self-deadlock on the session mutex | Return a reentrant-session error before waiting |
| Process callback running on a dispatcher worker | Caller-thread-only re-entry tracking missed synchronous worker callbacks | Kernel identity is first in ordered `execution_guards`; `with_guard` appends, reverse teardown keeps identity active through application-token and panic-payload cleanup |
| Resource `Store` read/write closure | Store's resource lock held | Remains a plugin/resource-owned critical section; callers must not re-enter the same resource from its closure |

Lock-order graph after the change:

```text
kernel producer mutex: claim/restore producer only; never surrounds plugin code
kernel session mutex: take/restore session only; never surrounds plugin code
exclusive ownership leases: span callbacks without holding inventory mutexes
  -> same-kernel calls on the callback thread or a dispatched process worker fail
      fast instead of waiting for that callback's own session lease
whole-drain gate: spans producer callbacks/admission; external publication waits
  -> same-drain publication fails; producer callbacks never wait for session access
all Kernel session callers: fail fast on cross-thread contention; retry after release
plugin/resource locks: remain owned by their API and are not ordered by Kernel
```

Producer callbacks remain serialized per kernel by the whole-drain gate. Recursive
`drain` returns `DriverError::ReentrantDrain`; concurrent manual drains serialize.
A blocking driver owns polling for its whole lifetime: a competing manual drain
returns `AlreadyRunning`, while driver startup during a manual drain returns
`DrainInProgress`. Retired drains return no events. A producer callback panic is
contained: poll panic is a terminal `ProducerError::Poll`; wake-registration panic
is a `DriverError`.
The producer mutex remains usable afterward.
This discards the initial no-wait/empty-return policy for competing drains, not
the baseline's polling-ownership guarantees. The driver/direct-drain exclusion
tests in [`lib.rs`](../packages/phenix-kernel/src/lib.rs) cover both directions,
including zero-budget manual drains while a driver is active.

External `KernelHost::publish` waits for an admitted **whole drain**, not just its
current poll, before retiring the generation. Events from that drain can enter the
old session before publication. Publication from the same drain is rejected with
`LifecycleError::ReentrantDrain`, rather than waiting on its own gate. Publication
does not wait for explicit session work, but a nonreturning producer callback can
delay it indefinitely. The initial no-wait-publication proposal is superseded.

Host self-destruction within a producer callback is different: it marks retirement
immediately and defers producer destruction until the enclosing drain/registration
restores ownership and releases the gate. A poll result returned after that
retirement is discarded. Admission already permitted before self-retirement may
finish injection. External effects already started cannot be stopped or undone;
producer/code ownership survives the callback and destruction runs outside locks.

All Kernel session calls, including ordinary concurrent `with_session` /
`run_steps` / `inject` calls, fail fast on cross-thread contention with
busy/`WouldBlock`. A caller can retry after release; producer admission retains one
event on contention. Same-kernel session callback/worker re-entry also fails fast.
This preserves the verified lock-inversion contract, including the original
regression without a producer callback marker. Initial session-waiter claims are
historical and superseded: a caller holding a plugin lock must not wait behind a
callback that needs it. Arbitrary plugin-vs-plugin lock freedom is not claimed.

`ExclusiveSlot` uses only `Mutex<Option<T>>`, `try_take` and idempotent release/RAII
restoration, with no Condvar or waiters. Three revised bounded Loom properties
cover competing exclusive owners, busy/no-theft plus successful retry after
release, and idempotent early release publishing state; exact-head acceptance is
still required.

Timeout changes reuse only `SessionOptions.deferred_timeout` and existing
`overdue_jobs(now)` / `supervise_deferred(now)`. C9 `state(&self)` remains passive:
it does not run readiness callbacks, apply expiry or mutate scheduler state.

## Design Alternatives And Independent Critique

- **No change:** retain the producer mutex across injection. Rejected because
  admission can invoke a custom readiness callback that re-enters `drain`, creating
  a same-thread lock cycle.
- **Smallest extension:** move injection outside the producer mutex after checking
  `retired`. Rejected because publication can win between the check and injection,
  making the stale-admission boundary ambiguous.
- **Reconciled implementation:** keep the verified whole-drain gate and exclusive
  driver ownership; release the producer inventory mutex around callbacks and
  injection, then reconcile ownership. External publication waits for that gate;
  same-drain publication is rejected and self-destruction rejects a late event.
  The initial independent no-wait admission-permit proposal is not adopted. No new
  scheduler or lock manager is introduced.
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

Historically, a second independent diff review caught two producer-loop edge cases:
removing a terminal last-slot producer could leave the round-robin cursor out of
range, and `drain(0)` briefly claimed callback ownership despite doing no poll. Cursor removal
now selects the shifted next slot or wraps to zero, with a last-slot regression.
The initial zero-budget early-return change is not a claim about the merged API:
`drain(0)` polls nothing but still participates in exclusive polling ownership and
the whole-drain gate, including typed driver/re-entry rejection.

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
- Letting an old poll publish after self-retirement was rejected: its external
  effects are not revocable, but the returned packet cannot enter the retired graph.
  External publication instead waits for the admitted whole drain to finish.
- Arbitrary plugin-vs-plugin lock cycles remain possible, including inside
  `ProcessContext::read/write` resource closures. Kernel lock freedom is not a
  claim of universal callback deadlock freedom.

## Acceptance Evidence

Regression coverage must prove poll/set-waker callbacks can re-enter without
holding the producer mutex, process callbacks can attempt same-kernel session
re-entry without hanging, callback panics do not poison kernel mutexes, external
session users fail fast while a callback owns the session lease and can retry
after release, and external publication waits for a blocked whole drain.
Same-drain publication must reject;
host destruction inside a callback must reject its late poll event. Readiness-factory,
correlation-bound, and request closure-policy panics must become deterministic
failures without partially admitting a request. Existing request, ticket,
notification, and lease identity behavior must remain intact.

The current worktree additionally tests admission callbacks outside the producer
lock, same-kernel re-entry from a dispatched worker, invalid closure declarations,
readiness-wrapper validation, and destructor-panicking panic payloads. These are
candidate-local regressions until a committed merged head passes CI. Initial
`0d145f3` local workspace, no-default-feature, Clippy, Loom and Nix passes are not
new-head acceptance. PR #1302 remote workflows initially conflicted; manual
[Flake #43588157944](https://github.com/matthis-k/phenix-ai/actions/runs/43588157944)
targets `0d145f3`, not the final merge. Full acceptance remains pending, including
all retained verified-stack scoped gates.

The implementation must pass formatting, focused kernel/graph tests, relevant
bounded Loom tests, Clippy, and the repository's six-job Flake acceptance on the
exact committed head. No performance improvement is assumed; nonblocking lease
acquisition and release overhead must be measured before changing any
performance score.

## Candidate Microbenchmark

Historical measurements from the inherited worktree, not rerun for the new merge
head. Its performance is unmeasured; these cannot establish its acceptance.

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
unconditional empty `Condvar::notify_all`; a later historical iteration notified
one waiter only when a waiter existed. Both waiter designs are superseded by the
current try-only slot without Condvar or waiter bookkeeping. Neither these
historical medians nor their observed overhead bound the merged head's latency,
contention or throughput.
The no-op `run_steps` relative percentage is misleading; H6 remains Partial,
unchanged pending representative end-to-end profiling.
