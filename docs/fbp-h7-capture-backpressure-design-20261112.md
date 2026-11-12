# H7: saturated capture queue observability and explicit recovery

Date: 2026-11-12. Base: verified producer-ownership PR #1273 at `08474b3d4cd8cdb257ed7a57097fde876afec754` (six-job exact-head CI #43277415425).

## Phase 0 — problem and invariant

Request-scoped `take_request_output` now correctly selects its first **already buffered** capture past unrelated FIFO heads. When `capture_capacity = 1`, however, request A can occupy the sole capture slot while request B has an uncommitted publication blocked on that queue. B cannot retrieve an unpublished packet; `take_request_output(B)` returns `None` without distinguishing no work from capture backpressure. The host can inspect `blocked_queues()` globally but `request_work(B)` does not identify which of B's own publications are blocked.

No bounded, lossless output channel can guarantee that B publishes while its single slot is held indefinitely by an unconsumed A without extra storage, consumer participation, or discarding A. This is a physical capacity constraint, not simply a scheduler bug.

Requirements: preserve atomic fanout, FIFO for ordinary output readers, ownership/provenance, existing queue capacity, no implicit capture disposal, no new runner/scheduler, and ordinary request cancellation semantics. User-owned cancellation may intentionally dispose A, but does not revoke B's job or mutate process state beyond existing recovery rules.

## Phase 1 — compare alternatives

| Option | Strength | Weakness | Decision |
| --- | --- | --- | --- |
| No change | No API growth | No request-local insight into capture-level blocking | Reject |
| Auto-discard oldest capture | B makes progress at capacity one | Violates losslessness, request authority, capture count, and FIFO | Reject |
| Per-request inbox / dynamic capture partition | Independent publication progress | Changes buffer budget by max requests; impacts output ordering, atomic fanout and leases | Not selected without separate primary-use-case acceptance |
| Larger capture capacity | Existing API, simple workaround | Does not guarantee progress under unbounded nonconsumption | Document |
| Extend existing `RequestWork` with passive blocked capture count | No new public concept, bounded inspection, real ownership-specific evidence | Diagnostics only; B still requires consumer action | **Select** |

Add one `RequestWork.capture_backpressure: usize` field, the count of unique full **capture** queues actually targeted by currently uncommitted publications held by the specified request. Inspect existing process `pending`, `pending_targets`, and `QueueEndpoint::Capture`; deduplicate by queue index. Include only full queues, not unrelated full inputs and not captures already committed. This is snapshot-only, with no readiness evaluation, refresh, queue mutation or packet transfer.

Write two regressions with a correlated shared-session join, capture capacity 1, ingress capacity 2:
1. A produces capture; B is admitted and becomes PublicationPending; `request_work(B).capture_backpressure == 1`, `take_request_output(B) == None`; `blocked_queues()` identifies the full capture, while `request_work(A).capture_backpressure == 0`.
2. Explicitly cancelling A disposes its committed queued capture, frees output capacity, allows B to publish on retry, preserves B's authenticated capture, effects and settlement, and leaves A's cancellation/disposal accounting intact. No implicit deletion is allowed.

A smaller ordinary-read test (consume A instead of cancelling) should also prove that B completes with capacity one, and that snapshots return to 0. Tests must not rely on wall-clock sleeps.

## Acceptance / limits

Expected `request_work` remains a public `Copy` snapshot. No second queue abstraction, no changes to `take_output` or `take_request_output`, and no memory growth proportional to requests. This does not by itself solve H7 fairness under infinite continuous contention, active-process cancellation/restart semantics, or guarantees across separately owned sessions. Future per-request queue isolation must be separately designed with explicit budget/ordering tradeoffs.
