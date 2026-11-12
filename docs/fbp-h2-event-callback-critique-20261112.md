# H2 — Host event callback panic containment (Phase 2 / Phase 4 plan)

## Adversarial critique before implementation

**Alternative A:** document that a host callback can unwind and leave the
error handling to callers. Smaller change, but preserves an untyped escape
at the one high-level driver entry point that already contains plugin panics.

**Alternative B (chosen):** one `DriverError` variant and a narrowly scoped
`catch_unwind` around `on_event(event)`. No second executor or retry path.
Unlike a blanket catch around the entire loop, this identifies the producer
and leaves other internal failures visible.

**Alternative C:** retain and replay the event after a panic. Rejected:
the callback may already have performed side effects or consumed part of
the event. Automatic replay would produce duplicate external effects.

Review questions: Does the `ProducerEvent` and any leased callback cause
drop inside unwind containment? Are locks released before callback execution?
Does a caught failure return without a second poll? Does the error preserve
the exact producer key? No claim of complete process isolation is allowed.

## Phase 4 acceptance

Require full format, kernel tests and workspace CI on the implementation's
exact HEAD. Regression should check a typed failure and subsequent
session/producers accessibility. Do not count passing checks for the baseline
as implementation evidence. No performance claim is made.

The behavioral change is intentional: a panic in a host callback invoked by
`run_until` becomes a typed error rather than unwinding to the caller.
