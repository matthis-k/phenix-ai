# H3b — fail-fast reentry into kernel-owned locks

Status: Phase 0/1 design, 2026-11-12. Baseline `406c38185404bd62e86eca891ebc07b0d487e3b7`.

## Phase 0: actual deadlock and retained invariants

The kernel stores a single mutable `NetworkSession` under a `Mutex` and
serializes whole producer drains under another `Mutex`. Both synchronize
across threads correctly. Some user/plugin callbacks run synchronously while
the session lock or whole-drain gate is held:
- graph readiness or process dispatch may attempt to call
  `Kernel::with_session`, `inject` or `run_steps` on that same kernel;
- `Producer::poll` and `Producer::set_waker` may attempt a recursive
  `Kernel::drain` during a whole drain / wake-registration pass.

These *same-thread* recursive calls cannot complete; `std::sync::Mutex`
does not support recursive acquisition. Today they can hang indefinitely.
The producer inventory mutex has already been released during callbacks
(H3a), and whole-drain retirement serialization is established in tests.
Do not reopen that fix or change the retirement boundary.

No-change leaves preventable deadlocks. The desired behavior is a prompt,
observable error while preserving the existing single owner, transport,
bounded polls, plugin leases and cross-thread mutex serialization.

## Phase 1: compare no change, extension and redesign

| Alternative | Benefit | Cost | Decision |
| --- | --- | --- | --- |
| Baseline: blocking locks | No API changes | Same-thread callbacks can deadlock | Reject |
| Change all locks to `try_lock` | Avoids some hangs | Wrongly rejects legitimate cross-thread contention and changes fairness | Reject |
| Use reentrant mutex + interior mutability | Recursive acquisition possible | Overlapping `&mut NetworkSession` borrows remain unsound/impossible; may panic later, adds dependency | Reject |
| Extract all callbacks from lock-held paths | Strongest arbitrary reentrancy model | Requires two-phase execution and new authority/concurrency protocol across graph and kernel; high risk to EOS, atomicity, whole-drain retirement | Defer separately |
| **Thread-local same-lock ownership guard** | Rejects only same-thread recursive acquisition before blocking, preserves ordinary cross-thread waiting and existing runtime ownership | One small private RAII concept; requires explicit diagnostics for the different public return shapes | **Select** |

One private RAII guard tracks the **identity of the mutex** in a thread-local
set. Enter before `lock()`, remove on scope exit/unwind. Same thread +
same mutex is rejected; different kernels and different threads remain
independent. It is not a global scheduler, queue, or second lock.

Error surface:
- `Kernel::with_session` and `run_steps` already return `Result<_,String>`;
  return `Err("reentrant kernel session access")`.
- `Kernel::run_until` returns `DriverError`; return an explicit
  `DriverError::ReentrantSession` rather than unwrapping a failed internal
  `with_session` and panicking.
- `Kernel::inject` returns the original `Value` in its existing
  `InjectionError`; add the exact `ConnectionError::Reentrant` reason.
- `Kernel::drain` returns a vector with no error channel; **fail fast with
  a programmer-error panic before taking the gate**. In producer `poll`,
  this is contained by the established outer panic-to-`ProducerError::Poll`
  boundary. In wake registration it is contained by
  `DriverError::WakeRegistrationPanicked`. An unhandled host-side recursive
  invocation may unwind; it must never silently return an empty success.
- `run_until` wake-registration holds the same whole-drain gate and must
  acquire and release the same RAII scope around that interval, *not* during
  the later driver loop.

### Explicit exclusions

This is fail-fast **rejection**, not support for performing arbitrary work
reentrantly. A callback blocking on another thread that itself waits for the
held lock can still deadlock; no general multi-thread wait-for analyzer or
callback scheduler is introduced. Plugin-internal mutexes, external effects
and arbitrary callback panics remain their own authority's responsibility.
The restriction affects only unsupported recursive reentry.

## Phase 4 proof obligations

Tests must demonstrate: same-kernel recursive `with_session`, `inject`,
`run_steps` reject immediately, and `inject` returns the original
payload; independent kernels are not blocked by a different kernel's session;
a poll calling `drain` again turns into exactly one producer error and the
whole-drain gate is released; same-thread wake-registration recursion returns
the existing typed wake panic error with owned producer restored. Existing
whole-drain retirement, FIFO, producer fairness and ordinary multi-thread
contention remain unchanged. Verify all workspace tests, strict Clippy,
rustfmt, no-default tests and exact-head CI.
