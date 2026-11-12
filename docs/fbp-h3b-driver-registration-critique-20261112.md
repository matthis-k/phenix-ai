# H3b — Reentrant driver registration (Phase 2, acceptance)

## Adversarial critique

Alternative A (no change): recursive registration is caught as an outer
producer panic but misclassified, while an inner wake handle is mutated.
Reject because a public Result-returning driver should not need an unwind
to diagnose this predictable call pattern.

Alternative B (chosen): a typed `ReentrantDrain` variant at the existing
TLS guard. Detect before any notifier write. Minimal surface; no new lock.

Alternative C: acquire the producer drain mutex before reading the session.
Reject: it would invert the existing session-before-drain ordering during
registration and could introduce cross-thread deadlocks when session
callbacks enter `drain`. Entering the TLS guard alone does not acquire the
mutex; that distinction is essential.

## Phase 4 acceptance

- Exact-head formatting, kernel regression and workspace tests/Clippy.
- Verify no source or producer is discarded on the nested driver call.
- Do not claim arbitrary cross-thread wait-cycle freedom.
- No runtime scheduler, plugin authority, contract or ABI change.

The caller may still need an explicit shutdown for a successful run to end.
The test supplies shutdown before registration to exercise the registration
path without waiting for events.

## Follow-up adversarial check — eager guard construction

`bool::then_some` is not an interchangeable spelling for `bool::then`
when `T: Drop`. The prior code removed a live recursion sentinel on
failure. A direct repeated-entry regression catches the defect without
attempting a potentially hanging mutex lock.

The lazy constructor must run only after a successful insertion. Guard
identity remains keyed to the exact mutex address, and outer `Drop`
remains the sole owner of marker removal. This fixes same-thread sentinel
lifetime, not cross-thread deadlock prevention.
