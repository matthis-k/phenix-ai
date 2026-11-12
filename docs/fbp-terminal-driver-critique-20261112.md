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

## Phase 2 counterexample from existing callback tests

Early success for every already-Completed session skips wake-registration
callbacks even when producers are installed on an empty graph, breaking the
existing `WakeRegistrationPanicked` and `EventCallbackPanicked` regressions.
Do **not** weaken those tests solely to accommodate a terminal fast path.

Revised choice: Failed/Cancelled return on entry, but Completed is reported
as `Ok` after the existing registration/first bounded iteration. This
fixes the empty-session hang without silently removing callback checks.
The limitation is that a completed graph with an unsupported waker may still
report `UnsupportedWake` before completion; changing producer shutdown
precedence requires a separate policy/design decision.

## Final Phase 2 decision — production semantics over stale fixtures

Preserving wake callback execution on already-Completed graphs is an
unnecessary error path: a terminal session can report unsupported wake
registration despite having no possible work. This is a foreseeable
primary-use-case defect, not an acceptable compatibility limitation.

The wake registration and host callback tests must use an open-ingress
Quiescent graph. The plugin test's exact host composition grants admit its
test-only sink/input; no special production bypass is introduced. A
Completed graph with an unsupported producer must exit successfully
without invoking producer callbacks. This restores unconditional terminal
success while keeping all existing callback panic regressions meaningful.

## CI-discovered fixture counterexamples (2026-11-12)

CI [43184695814](https://github.com/matthis-k/phenix-ai/actions/runs/43184695814)
passed the core formatting and new driver callback tests but found two older
`kernel.rs` fixtures that incorrectly expected an empty, already-Completed
network to keep polling or block for shutdown:
`empty_driver_blocks_until_cross_thread_shutdown` and
`permanent_poll_failure_is_reported_once_and_driver_remains_shutdown_capable`.

An empty network is a valid terminal session. Retain the production terminal
fast path and move both tests to an explicitly open-ingress, Quiescent
network. The fixture registers only an ordinary graph sink and input projection,
with exact host composition grants. It adds no special kernel authority or
hidden waker. An assertion checks Quiescent before exercising shutdown.
The original direct producer polling error test remains unchanged.

Do not relabel a completed session as Waiting just to preserve a test.
Full exact-head Nix CI, all shards plus aggregation, remains the acceptance
gate. The fixture is test-only and should not change runtime semantics.
