# Small Snapshot And Diagnostic Boundary Corrections

Status: implemented, independently reviewed and locally verified; exact-head hosted
acceptance pending. Based on verified `94cae313f`.

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

## Deferred Findings

The separately reproduced Q3 dependency-defeat bug still needs a termination and
ambiguity design; see [its reproduction](fbp-error-context-20261115.md#separate-q3-reproduction).

Source inspection also found that late request termination can replace a status
inherited from terminal session failure/cancellation. A blanket no-op guard is not
accepted here: query cleanup uses termination to dispose captures and then release
its scope. Bounded-lane release rejects retained packets, potentially masking the
original query error with NotSettled. Reconcile terminal precedence, explicit
disposal and inventory/release together before changing that path. This batch
does not change request termination or claim end-to-end query cleanup correctness.
