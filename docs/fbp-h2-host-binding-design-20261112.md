# H2 — host binding callback panic containment

Status: Phase 0/1, 2026-11-12. Baseline `3de96f53454e61794b4ccd9faf4e5200a49a1724`.

## Phase 0 — concrete gap

`KernelHost::prepare` invokes the host's `bind(Binding<'_>)` closure after
constructing a complete kernel but before starting producers or publishing
the candidate. Returned `CallbackError` already becomes the typed
`LifecycleError::Binding`, with a lease retaining code during destruction.
A panic currently unwinds through the caller instead of returning an error.
Preparation must not alter the active published generation on either kind
of callback failure. Side effects performed by the callback itself are not
rolled back.

No additional plugin capability, scheduler, lock or graph concept is needed.

## Phase 1 — baseline, extension, redesign

| Option | Benefit | Cost | Decision |
| --- | --- | --- | --- |
| Keep unwind | No implementation change | Plugin-owned code can escape the preparation error boundary | Reject |
| Add a new `LifecycleError::Panic` | Distinct error variant | Duplicates existing `Binding` phase, unnecessary public API | Reject |
| **Catch at binding invocation, reuse typed `Binding` error** | Preserves host phase and lease, matches preparation/activation precedent | Panic payload not forwarded; only a stable diagnostic | Select |
| General plugin supervisor | Common callback orchestration | New ownership and policy abstraction for one callsite | Reject |

Use `catch_unwind(AssertUnwindSafe(...))`, translate a panic into
`"generation binding panicked"` under the existing `LifecycleError::Binding`.
Do not swallow returned typed callback errors; their exact source chain remains.
No producer activation or pointer swap on either error. Candidate objects and
leases release via the existing rollback/drop flow.

## Phase 4 evidence

A regression first publishes a generation, then deliberately panics in a
replacement binding. The call must return `LifecycleError::Binding` without
unwinding, preserve the active pointer and epoch, and leave producer activation
and snapshot registry unchanged. Existing typed-error source-chain regression
must stay green; exact-head workspace test, rustfmt, Clippy and CI required.
