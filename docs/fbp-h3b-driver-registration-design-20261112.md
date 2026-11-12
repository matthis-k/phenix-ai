# H3b — Reentrant driver registration (Phase 0–1)

Baseline: `92e6770fe3b6729cc3ab94a576f4fc4d9d40ed96` on draft PR #1263, stacked on
`89e00d06498979a2f231dfc8ca780eba44140d27`.

## Phase 0

The kernel's same-thread `KernelLockScope` prevents recursive producer drain
callbacks. The public `run_until` driver currently calls
`expect("recursive producer drain")` on recursive registration, so a
nested `run_until` invoked from a producer's `set_waker` unwinds rather than
returning a typed driver error. It also mutates the proposed wake handle's
notifier before detecting recursion.

Leaving this unchanged relies on outer panic containment and reports the
failure as a producer wake panic, losing the actual cause. No additional
executor or mutex type is needed.

## Phase 1

- Reuse `KernelLockScope`, returning the new typed
  `DriverError::ReentrantDrain` when its same-thread entry fails.
- Enter the thread-local guard before reading the session or installing the
  new wake notifier; the actual producer drain mutex remains acquired only
  *after* session access, preserving the existing cross-thread lock order.
- Drop guard on any error via RAII. Preserve the existing register-once
  serialized producer callback loop and producer restoration.
- Regression: nested `run_until` during `set_waker` reports
  `ReentrantDrain`, outer registration completes, producer remains owned and
  subsequent drain is operational.

The existing `Kernel::drain` returns only a Vec and intentionally panics on
same-thread recursion, which is already contained at producer callback
boundaries. This change does not claim to solve cross-thread lock cycles.
