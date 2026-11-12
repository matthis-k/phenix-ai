# Phase 2 — adversarial critique: reentrant host destruction

Date: 2026-11-12. Independent review of [the design](fbp-host-drop-reentrancy-design-20261112.md).

1. **Slot checked out:** `poll` and `set_waker` temporarily replace their producer slot with `None`. Host `Drop` must **not** detach inventory from inside those callbacks; the outer code writes the producer back by index after returning. Mark `retired` only and let the owner flush at its normal exit.
2. **Lock ordering and destructor execution:** The outer callback owns `producer_drain` and possibly another thread owns the session mutex. Use only the inventory mutex to set `retired`, since callbacks are already explicitly outside inventory locks. After callback completion, detach while holding the gate but run all destructors only after both locks and TLS guard are released. Destructor regressions should `try_lock` both.
3. **Waker error branches:** Earlier wake registration uses an early `return Err(...)`; this bypasses deferred cleanup. Preserve exact `UnsupportedWake` / `WakeRegistrationPanicked` results via stored error and a single cleanup exit. Do not lose producer ownership on panic.
4. **No post-retirement callbacks:** Additional wake registration iterations must inspect `retired` before taking the next producer. `drain` already stops polling on `retired`. Avoid a new driver state machine.
5. **Ownership proof:** The `Kernel` is borrowed by its outer drain/driver during reentrant host destruction. Tests must retain an `Arc<Generation>` so its memory stays alive while host ownership disappears, and prove that producers are detached even if that external generation handle stays alive.
6. **Scope of claim:** Cross-thread user callbacks that wait on unrelated locks, direct `Kernel::drain` competing with `run_until`, and long-running process steps are not made deadlock-free. An external driver may still wait for open ingress after producers retire: that is separate from safe destruction and needs a lifecycle policy before changing behavior.
7. **Regressions:** Use deterministic single-thread reentry and an open ingress for `set_waker`; no sleeps, no inserted background cleanup thread, and no tests that merely pass because the network begins Completed.

**Disposition:** The deferred-detach approach is preferable to blocking or skip-cleanup. Proceed only with both callback entry-path tests and full exact-head CI. No full H3b closure claim until remaining external/direct-driver cases are addressed.
