# Terminal Request Cleanup

Status: implemented, independently reviewed and exact-head verified at
`026cfbe05e9b078f891233a2fc65bcada3fbfea0` in the assembled workspace.

Policy: **Dispose, Keep Failed**. Explicit request termination after a global
terminal transition disposes only that authenticated request's retained work.
It does not rewrite the effective terminal outcome or the session's cause.

## Runtime Contract

- Effective terminal status is the request-local terminal status, otherwise an
  inherited session `Failed` or `Cancelled`, otherwise no terminal status.
- `request_status` and request termination share only that session-to-request
  mapping. Termination does not invoke settlement or capture-count validation.
- Authentication precedes the existing local-terminal idempotence guard. A
  previously locally terminated request returns its existing job inventory.
- The first explicit cleanup of an inherited terminal request records the
  inherited status locally and uses the existing ownership disposal machinery.
  Late caller diagnostics are ignored; inherited failure diagnostics continue to
  use `request_failure`'s fallback to the original session failure.
- Retained captures belonging to other requests remain untouched. Disposal and
  invalidated-job counts are not doubled by repeated cancellation/failure.
- The existing bounded-lane release empty-queue check is unchanged. Explicit
  request disposal empties its queues so release can succeed after global failure.
- Global `session.cancel()` after failure remains nondestructive. It is not an
  alias for explicit request disposal, and still preserves retained captures for
  host draining. A live request cancellation can still trigger `FailSession`,
  with the request's `Cancelled` outcome recorded first.
- Recovery factory panic appends secondary recovery context to an existing
  request failure instead of overwriting it. Cancellation can record recovery
  context without changing its local `Cancelled` status. No factory is invoked
  by explicit request cleanup after the session is terminal.
- Cleanup destructor failures remain typed return errors. They do not replace
  the primary terminal status or overwrite an inherited session failure.

## Query Finalization

The query finalizer retains the original query result, attempts explicit request
termination on errors, snapshots cancelled tickets and disposal counts while the
scope is authenticated, then attempts release without an early `?` return.

For an original query error and either cleanup-stage failure, the public envelope
is `Error::Cleanup { cause, request, termination, release }`. Both stage failures
remain inspectable as typed `RequestError` values. `Error::source()` points to the
original error, not a cleanup error. `request` is retained because a failed
release can leave an authenticated scope requiring caller inspection or retry;
no new resource authority or disposal scheduler is introduced.

Nonempty cancelled-job inventory remains in the outer `RequestInterrupted`
envelope, including when its cause is `Cleanup`. Tickets are exact, and handed
jobs/returned completions keep their native code leases after scope release.
Successful queries do not acquire a fabricated error cause: a release error on
that path remains `Error::Request`. Successful queries and ordinary `Unsettled`
errors without cleanup errors keep their existing result shapes.

## Focused Evidence

`packages/phenix-graphs/tests/fbp_terminal_cleanup.rs` exercises both queue
policies and both failure policies where applicable:

- Separate nondisposal control for global cancellation after failure retaining a
  capture, and explicit request disposal retaining `Failed` plus the original
  cause, disposing only its own capture once, and permitting release.
- Global cancellation followed by late request failure preserving `Cancelled`
  with no late diagnostic; earlier local failure/cancellation outcomes survive
  subsequent global termination.
- Exact handed-job inventory across repeated termination and rejection of late
  completion; foreign and released request rejection.
- Recovery panic preserving initiating request failure, and live cancellation
  recording `Cancelled` before the fail-session policy runs.

`packages/phenix-storage-graph/tests/query_cleanup.rs` exercises both queue
policies with real query execution:

- Invalid dispatcher completion causes global failure after a source capture is
  retained. Query cleanup returns the original `Wave`, not release `NotSettled`,
  with exact cancelled-job inventory and successful scope release.
- An additional retained capture with a panicking destructor preserves that
  `Wave` under typed `PacketDestructorPanicked`, without changing the session
  diagnostic or losing peer tickets.
- Budget exhaustion with a deferred peer and a destructor/recovery factory panic
  preserves `Unsettled`, typed cleanup failure, exact peer inventory, late
  completion rejection, and completion-owned native lease lifetime.

`packages/phenix-storage-graph/tests/error_sources.rs` additionally checks the
original-first source chain and simultaneous inspectable termination/release
fields. Simultaneous cleanup-stage failure is an envelope construction test,
not a fault-injected failure of the runtime's release implementation.

Focused verification uses the Nix development environment:

```sh
nix develop --command cargo test --locked --offline -p phenix-graph --test fbp_terminal_cleanup --test fbp_request_lanes --test fbp_request_diagnostics --test fbp_request_session_isolation --test fbp_streams
nix develop --command cargo test --locked --offline -p phenix-storage-graph --test query_cleanup --test derived --test error_sources
```

Both commands passed. Targeted Clippy runs for the changed libraries and focused
tests also passed with `-D warnings`; `git diff --check` passed. Formatting was
limited to this track's Rust files, with module-child traversal disabled.

Existing capture-drain and native inflight control assertions are unchanged by
this track. Kernel/host, graph resource APIs, execution guards, and fail-fast
behavior are outside this change. Workspace-wide validation and issue/evidence
accounting remain with the integrating track. Full assembled workspace tests with
default/no-default features, formatting and strict all-target Clippy now pass.
Independent terminal-track and assembled integration reviews found no actionable
defects. This is scoped terminal/cleanup acceptance, not whole Q7 completion.

All fifteen hosted workflows passed on that implementation, including the full
six-job Flake matrix and both Loom/native concurrency gates. Their exact links are
recorded in [the shared acceptance evidence](fbp-resource-boundary-20261115.md#exact-head-acceptance).
The request-level inherited-outcome defect is resolved under the approved policy;
the old isolated reproduction's capture-preservation expectation after explicit
request cancellation is not that policy. Nondisposal controls still preserve
captures, while authenticated explicit disposal retains the original Failed status.
This evidence-only follow-up requires its own committed-head checks.
