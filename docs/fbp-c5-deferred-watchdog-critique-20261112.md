# C5 — adversarial review of deferred supervision

- Must register the job when ownership *leaves the session*, not only
  after `DispatchResult::Deferred`: manual `next_job()` is also an
  external worker handoff.
- Complete/reject must remove the timer only after the existing ticket
  validation succeeds; foreign completions cannot clear another job's age.
- Cancel/failure must clear the map; the ticket remains held by its runner
  and may only be rejected as stale on late completion.
- A status query must remain passive and not invoke supervision or callbacks.
  Explicit driver operations may check `Instant::now()`.
- Use monotonic `Instant` rather than system wall clock; the API injects
  `now` so tests need no sleeps.
- No recursion into the external dispatcher or process step on timeout.
- This is *not* a general process-killing watchdog; host supervision is
  required. Do not label abandoned external worker ownership reclaimed.
- Duration::ZERO is legal and denotes failure at the next supervision
  boundary, not retroactive execution cancellation.

Accepted with explicit limitations and exact-head regression/CI.
