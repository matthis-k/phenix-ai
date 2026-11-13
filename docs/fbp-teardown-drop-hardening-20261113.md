# FBP Teardown Drop Hardening

Status: implementation candidate based on verified PR #1300 at
`1a57f7a6ebdf512200244e1b14cb4a979ce99128`. The prior Q5/H3b producer ownership
and wake changes remain the baseline; this batch addresses two teardown gaps found
by auditing callback/destructor boundaries at that exact head.

## Phase 0: Invariants

- A storage subscription's custom Waker destructor is plugin code. It must not run
  while the Watch mutex, mount transaction, or storage `ACTIVE` reentrancy guard is
  held. Existing `run_wakers` defers both wake calls and Waker destruction.
- `with_session`, `run_steps`, dispatcher, readiness, recovery-factory, and process
  callbacks must not execute while the kernel session mutex is held. Same-thread
  recursion is `ReentrantSession`; cross-thread contention is explicit busy/WouldBlock,
  not a blocking wait while plugin code may be holding a foreign mutex.
- Request termination publishes its terminal identity, detaches request packets,
  invalidates old process wakes, cancels owned activations, and installs replacement
  process state before arbitrary packet/process/context destructors run.
- Queue request counts and FIFO contents are updated atomically before a detached
  payload is destroyed. A destructor panic must not strand unvisited cleanup work.
- Native work already executing on a delegated runner remains unrevocable. Its
  ticket is invalidated and the runner still owns its process/context/lease.

## Phase 1: Design And Alternatives

| Boundary | Chosen change | Rejected alternative |
|---|---|---|
| Last `Subscription` drops | Take its Waker under the Watch mutex, release the guard, then route the Waker through `run_wakers(WakerAction::Drop)` | Add Waker callback logic to every teardown path or directly drop under the Watch mutex |
| Kernel session operations | Move the `NetworkSession` out of `Mutex<Option<_>>` into a private RAII lease; callback executes without the mutex and all public callers fail fast if a lease is active | Keep a blocking mutex across dispatcher/process callbacks or add another scheduler/drive protocol |
| Request queue disposal | Move request packets out while preserving remaining FIFO and updating counts; return packet ownership to the lifecycle caller | Catch panic inside `VecDeque::retain`, which can leave request counts inconsistent or skip later cleanup |
| Process cancellation | Invalidate handoff and detach context/process/code lease into one private `CancelledProcess`; drop fields individually after the outer request/session transition | Drop fields inline from `ProcessHandoff::cancel` before sibling wakes/queues are retired |
| Destructor panic | Catch per packet/value/resource/process/lease, continue cleanup, then return a typed cleanup error or record a terminal diagnostic | Let one plugin `Drop` unwind through scheduler cleanup, or silently retry/rollback committed effects |

No new scheduler, lock manager, compatibility API, or runtime mode is introduced.
The session lease is exclusive ownership, not a lock held across user code. All
public session callers fail fast and can retry after a `SessionBusy`/`WouldBlock`
result; the runtime never waits for another callback while a plugin may hold a
foreign mutex. The private detached process value represents exactly the state that
must survive until cancellation is visible.

Public API changes are limited to ownership/error accuracy: `ConnectionSet::detach_request`
and `ConnectionSet::cancel` return detached `Vec<Packet>` ownership instead of a
count; `ProcessHandoff::cancel` returns `Result<Option<StepTicket>,
HandoffCancelError>`; `RequestError` adds process/packet destructor-panic variants.
`Kernel::with_session`/`run_steps` return a busy error when another thread owns the
session lease, `run_until` returns `DriverError::SessionBusy`, and `inject` returns
`ConnectionError::WouldBlock` with its value. `NetworkSession::cancel` keeps its
signature and records cleanup-panic diagnostics in session failure state.

## Phase 2: Adversarial Critique

- Dropping a subscription inside a successful, failed, or panicking storage
  transaction must run its custom Waker destructor only after `ACTIVE` is empty,
  the Watch mutex is available, and same-mount re-entry succeeds.
- A request with a `WaitWake` process and queued request packets includes a
  panic-once payload destructor. Cancellation must still invalidate the old wake,
  clear queued/request ownership, finish process cancellation/recovery, and allow
  a new request to settle. The API reports which cleanup destructor panicked.
- The cancellation test must separately exercise a payload held in a queued packet
  and one held in a reserved `ProcessContext`; otherwise a queue-only fix could
  still leave `ProcessHandoff::cancel` as an unwind escape.
- If a replacement factory also fails, its `RecoveryFailed` result remains primary;
  request cleanup still runs and diagnostics retain the factory failure. No external
  effect is undone by this cleanup protocol.
- While a process callback owns the session lease, another thread attempts
  `with_session` while holding a plugin mutex. It must receive `SessionBusy` promptly
  so the callback can acquire/release that plugin mutex; it must not block behind a
  kernel session mutex.
- Panic hooks may print during `catch_unwind`; containment means state transition
  and remaining destructors complete, not that the process-wide panic hook is muted.

## Phase 3: Implementation And Tests

`Subscription::drop` takes its Waker outside the callback boundary and delegates
to the existing storage deferral/panic-isolation path. Queue cancellation returns
detached packet ownership. `ProcessHandoff::cancel_detach` invalidates its ticket
and moves state out; request/session cancellation disposes it only after wake and
queue state is terminal. Each owned payload is dropped separately under
`catch_unwind`, preserving the plugin code lease until process/context destruction
is complete. Kernel APIs take/restore the session with an RAII ownership lease,
holding the mutex only during those transfers.

Regressions cover storage transaction success/error/unwind and request cancellation
with both a queued packet and a reserved process input whose destructors panic once.
The kernel process callback also probes a concurrent session API attempt while
holding a test plugin mutex; it receives busy promptly rather than blocking on the
session mutex.

## Acceptance And Limits

Exact-head six-job Flake CI, the native Q5/concurrency workflows, the existing
bounded Loom verifier, focused storage/graph tests, and strict Clippy/format must
pass after implementation. This does not claim arbitrary plugin-vs-plugin or
cross-mount host lock-order freedom. A panic from an external Drop is reported only
after the kernel-owned state transition and all detached cleanup attempts complete;
the panic hook itself remains host-global.

## Candidate Session-Lease Microbenchmark

The same optimized, setup-excluded 100,000-call / seven-sample harness on clean
PR #1300 head `1a57f7a6` vs. this candidate measured medians of 678/757 ns for
no-op `with_session` and 107/188 ns for empty `run_steps(0)` (baseline/candidate).
The candidate medians are from three sequential runs; individual results varied
by roughly 100 ns. The empty-drive percentage is dominated by a near-empty
baseline; added absolute cost was about 79-81 ns/call. This harness does not
measure process throughput or allocations. Exact-head native runtime, H4/H5 paired
and H6 allocation workflows passed; H6 remains partial and unchanged.
