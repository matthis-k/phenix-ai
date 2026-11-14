# Phase 2 adversarial review: cross-thread kernel session APIs and driver lease
Date: 2026-11-12. Proposed implementation in
[fbp-driver-lifecycle-design-20261112.md](fbp-driver-lifecycle-design-20261112.md).

1. **Check the actual cycle.** A producer callback under producer_drain must
not block on session while another thread holds session and requests drain.
Try-lock only in this exact callback scope, not for normal client calls.
Cover with an ordered two-thread test that hangs without the fix, plus a
bounded CI watchdog (rather than substituting sleeps for ordering).
2. **Do not lose original admission payload.** A failed Kernel::inject
must return its unmodified Value and a *distinct* `WouldBlock`, not
`Full` or `Reentrant`. No queue state or poll cursor may advance.
3. **Preserve uncontended callbacks.** Do not forbid a producer callback from
inspecting the session when no contention exists. Same-thread session
reentry continues to fail by existing TLS identity, not by try_lock.
4. **Driver setup reordering.** Current run_until initially enters TLS gate
before with_session. If try-lock were used blindly, normal cross-thread driver
start would now fail spuriously. Probe held gate first, read session second,
then enter gate before notifier changes. No actual drain mutex acquired
while the initial session lock is held.
5. **Driver lease lifetime.** An atomic boolean must be RAII released on
every normal, error and unwind return, and needs Acquire/Release ordering.
Verify nested run_until from set_waker still returns ReentrantDrain rather
than AlreadyRunning; check initial NotStarted precedence.
6. **Competing wake registrations.** A second external driver must return
AlreadyRunning *before* setting notifier or calling set_waker. The first
driver must still be able to exit by sticky shutdown and the next call
must work with a fresh handle.
7. **Scope/authority.** No new scheduler, callback execution thread,
cross-plugin shared mutable state or producer retirement shortcut. Avoid
claiming arbitrary cross-thread deadlock freedom or fixing KernelHost::drop.

**Phase 2 disposition:** Accept Phase 3 only if these are implemented with
deterministic regressions; any remaining suspected deadlock becomes an explicit
open finding rather than an accepted universal guarantee.
