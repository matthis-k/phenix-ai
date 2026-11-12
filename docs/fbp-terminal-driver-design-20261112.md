# FBP driver terminal completion — Phase 0/1 (2026-11-12)

Baseline: `2fcac06005875e32aed279f7ef377b95a8d25775`, draft PR #1263.
Evidence: diagnostic run [43171079156](https://github.com/matthis-k/phenix-ai/actions/runs/43171079156) failed on 480-second test timeouts in both check shards because
`native_sessions::cancelled_sessions_stop_the_driver_instead_of_spinning_on_invalid_wakes`
never completed. The kernel library unit tests, including new H2/H3b cases,
passed in the same diagnostic run.

## Phase 0 — identify behavior, not proposed mechanism

The regression creates a graph with zero processes and zero public ingresses.
This is already `SessionState::Completed`. Its `NetworkSession::cancel()`
deliberately preserves completion (C1 invariant), so the test's expectation
of `Cancelled` is wrong. The driver only returns a typed stop error for
`Failed` and `Cancelled`, and sleeps forever when the session is
`Completed` with no external wake.

Doing nothing leaves a real blocking-driver hang and falsely failing
regression. Changing cancellation to overwrite completion would violate C1.
A new scheduler or notification service is unnecessary.

## Phase 1 — smallest safe change

- For a session `Completed` on entry, `run_until` returns `Ok(())`
  without registering producers or blocking. `Failed` and `Cancelled`
  on entry return the existing `DriverError::SessionStopped` with cause.
- For a session that transitions to `Completed` in a driver iteration,
  also return `Ok(())`. Failure and cancellation behavior is unchanged.
- Move the cancellation regression to a genuinely live open-ingress
  network using the existing fixture, asserting pre-cancel `Quiescent`
  and post-cancel `Cancelled`. No waker registration is required for
  an already terminal session.
- Add an explicit empty-completed-session regression, without timers
  or synthetic work. Retain all other test coverage.

## Preserved boundaries

No modifications to `NetworkSession::cancel`, completion semantics, plugin
provider authority, request cancellation or wake handling. `Completed`
is a successful terminal outcome for the blocking driver, while failure
and cancellation remain typed errors. This is a behavioral API correction,
not a compatibility alias.
