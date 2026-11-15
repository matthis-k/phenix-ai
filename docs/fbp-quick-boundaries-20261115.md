# Small Snapshot And Diagnostic Boundary Corrections

Status: implemented, independently reviewed and exact-head verified at `979572eb9`.
Based on verified `94cae313f`.

## Constraints And Alternatives

Keep existing admission, request ownership, snapshot authority, codecs, cleanup and
scheduler semantics. Fix only three independently reachable validation/diagnostic
defects; no new public testing API, compatibility hierarchy or scheduler is needed.

1. Public tuple `StoredInputs::decode` indexed short slices and ignored trailing
   entries. Check the macro-counted arity before any cell decoder and return the
   existing `SnapshotEntryMismatch`. Computing sources to count inputs would allocate
   and invoke unrelated callbacks; validating after decoding would be too late.
   Normal derived queries already have SnapshotReader cardinality validation, so
   this repairs the public tuple helper rather than claiming a normal-query panic.
2. Query tap/root lookup discarded actual `UnknownDefinition` and
   `AmbiguousDefinition` errors as `WrongGeneration`. Use the existing typed
   `Error::Network` conversion at all four runtime/preparation lookup sites. Remove
   the otherwise-unused `WrongGeneration` variant rather than keeping a dead public
   compatibility case. This is an intentional error-classification API change.
3. `input_waits` reported unrelated live lanes for an owned input continuation.
   Retain process context through queue filtering: bounded WaitingInput handoffs
   use the owning lane, or canonical queues if unscoped. Idle processes still report
   all registered live lanes and canonical queues; SharedFifo is unchanged. Calling
   readiness selection from this passive query would violate its contract.

Independent preimplementation critique approved these bounded approaches and
required zero-decoder cardinality tests, tap/root failure cases at both binding
boundaries and ownership tests for both WaitInput and WaitInputs continuations.

## Implementation And Acceptance

Existing StorageGraph fixtures now cover exact/short/empty/extra tuple lengths for
arities one through four. Custom decoder counters remain zero on malformed lengths;
real ReadCells still reject exact-length wrong-source entries. Source callbacks
are not needed by the arity check.

Query tests retain typed declaration causes and verify direct downcasts/pointer
identity, source-leaf termination, tap/root unknown and ambiguous deployments,
pre-start runtime binding and host preparation binding. Valid binding remains
read-free and failed preparation never activates producers. Route/export validation
and genuine string leaves remain unchanged.

Existing stream fixtures verify actual diagnostic queue IDs/endpoints for scoped
and unscoped WaitInput/WaitInputs, unrelated traffic, idle ordering, released/reused
lanes and SharedFifo. Readiness counters establish passive observation. The fix
does not change which packets an activation can consume.

These are bounded fixes, not whole Q6/Q7 closure or milestone acceptance. No
subjective score or issue count changes are justified by this batch.

Final independent diff review found no actionable defects. Full locked workspace
tests with default and no-default features, formatting and strict all-target
workspace Clippy pass locally. Existing scoped concurrency and authority guarantees
remain required by the final same-head hosted gates.

## Exact-Head Acceptance

Implementation `979572eb9e08462adddac07d35625bd7f2cb940e` passed all fifteen
hosted workflows, including:

- [Full six-job Flake matrix 43705906267](https://github.com/matthis-k/phenix-ai/actions/runs/43705906267).
- [Actual-source wake Loom 43705905883](https://github.com/matthis-k/phenix-ai/actions/runs/43705905883).
- [Session ownership Loom 43705905876](https://github.com/matthis-k/phenix-ai/actions/runs/43705905876).
- [Eight-repeat native notifier/plugin-lock gate 43705910462](https://github.com/matthis-k/phenix-ai/actions/runs/43705910462).
- [Twelve-repeat in-flight worker recovery 43705910508](https://github.com/matthis-k/phenix-ai/actions/runs/43705910508).

These accept the three bounded corrections, not general query cleanup, static
liveness or whole Q6/Q7 findings. Benchmark workflow success is not blanket
performance acceptance. This evidence-only follow-up requires its own head's checks.

## Deferred Findings

The separately reproduced Q3 dependency-defeat bug still needs a termination and
ambiguity design; see [its reproduction](fbp-error-context-20261115.md#separate-q3-reproduction).

An isolated worktree at `/tmp/opencode/fbp-terminal-reproduction-979572eb`, based
on that implementation, reproduces two request-terminal precedence failures:

- `terminal_precedence_session_cancel_survives_late_request_failure`: session
  cancellation reports a request Cancelled, but late fail_request changes it to
  Failed while the session remains Cancelled.
- `terminal_precedence_global_failure_survives_cancel_and_retains_capture`: another
  request fails the session; late cancel_request changes an inherited Failed to
  Cancelled and actually disposes the retained capture, as confirmed by queued and
  disposal counts. The session remains Failed.

Both new tests fail against production; the existing retained-capture control
passes. Only the isolated tracked test file is changed, not the accepted batch.
This is a request-level gap beyond the scorecard's inherited session-level C1
regressions, not evidence that the broader terminal protocol is complete.

A blanket no-op guard is not
accepted here: query cleanup uses termination to dispose captures and then release
its scope. Bounded-lane release rejects retained packets, potentially masking the
original query error with NotSettled. Reconcile terminal precedence, explicit
disposal and inventory/release together before changing that path. This batch
does not change request termination or claim end-to-end query cleanup correctness.
