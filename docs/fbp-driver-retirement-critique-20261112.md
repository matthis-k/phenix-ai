# Phase 2: adversarial critique — driver retirement wake

Date: 2026-11-12. Design reviewed: `fbp-driver-retirement-design-20261112.md`.

1. **Lost wake:** Retirement may race initial driver registration, wake registration, post-drain state check, or entry to `WakeListener::wait_timeout`. A wake before the session notifier is set can be lost. The driver must inspect the producer `retired` flag before entering registration and before sleeping, and the epoch-latched listener must handle notification between check and wait.
2. **RAII and poisoning:** The lifetime registration must release its Kernel-owned wake clone on normal exit, `UnsupportedWake`, callback panic, terminal return, and unwind. Never hold its mutex over a producer or process callback. A non-driver call must not erase an active driver's lease.
3. **No cancellation:** Do not call `WakeHandle::shutdown` or `NetworkSession::cancel`; return a typed retirement error. Retired `Generation::with_session` and existing capture draining stay available, as specified by the current contract.
4. **Typed precedence:** A completed/failed/cancelled native session should keep existing terminal behavior; registration callback failure/panic must remain specific instead of being masked by `Retired`. Same-thread recursive driver calls keep `ReentrantDrain`.
5. **Synchronized publication:** Host retirement must signal only after recording `ProducerState.retired`. The driver must not block the host waiting on the session mutex in its notification path; use its pre-registered `WakeHandle` instead.
6. **Ordering and callbacks:** Retirement may happen from `set_waker`, `poll` or a different thread; do not invoke further producer callbacks after retirement. Do not claim a worker step in progress was cancelled.
7. **Boundaries:** Explicit producer `drain` can still race a blocking driver. The fix does not solve all H3b/Q5 lock cycles. Event-driven retirement notification adds one mutex acquisition to host retirement and one to driver registration/teardown; avoid new per-poll scanning beyond already-required state snapshots.

**Disposition:** Accept the smallest internal wake-lifetime addition and typed retirement result. Require deterministic two-thread wake/sleep and callback-driven retirement tests, exact-head CI, and explicit nonclaims. Phase 4 rechecks these limitations.
