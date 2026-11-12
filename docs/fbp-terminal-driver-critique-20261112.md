# Driver terminal completion — independent Phase 2 challenge

**A — Only change the test fixture.** The original hang would remain for a
correctly completed empty graph and for normal completion after a driver
step. Reject: it fixes CI by concealing a production driver bug.

**B — Return `SessionStopped` for Completed.** Always terminates, but
represents successful graph termination as an error. Reject absent any
requirement that completion should be failure.

**C — Chosen.** Treat Completed as `Ok(())` on entry and after a bounded
driver iteration. Retain `SessionStopped` for Failed/Cancelled. Correct
the cancellation regression to exercise a live session.

**D — Allow cancellation to overwrite Completed.** Reject: breaks the
previously repaired first-terminal-wins invariant (C1).

Review evidence: diagnostic CI reports the stuck native-session regression
in kernel and integration/no-default suites, while kernel unit H2/H3b
tests pass. Verify new test enters Quiescent and preserves Cancelled.
Verify Completed exits before unsupported producer waker registration.
Use exact-head CI and normal unmodified `flake.nix` to accept.
No general cross-thread deadlock guarantee is made.
