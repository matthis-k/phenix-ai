# H7 — request-scoped capture access without unrelated FIFO head blocking

2026-11-12. Base: PR #1269 at `ae746e796550434713e53302b259c52fc3702fbd`.

## Phase 0: requirement / baseline

`NetworkSession::take_request_output(id, name)` currently returns `None` when a valid packet for `id` is buffered behind another request's packet, even when both are already published. An application using that explicitly request-scoped API must first consume an unrelated output through `take_output`, violating request ownership expectations and permitting completed requests to block one another at the capture reader. `ConnectionSet::pop_at` already implements bounded correlated extraction for process inputs.

Requirements: do not change global `take_output` FIFO or queue capacity. Preserve authenticated RequestId, capture origin, output name, source, exact-output checks, and packet ownership. Never remove a packet before validating it. Preserve order of all remaining packets (including multiple for the same request), capture accounting and `refresh` semantics. Avoid a second scheduler, new transport, implicit packet discards or extra public abstraction.

## Phase 1: options

| Option | Benefit | Problem |
| --- | --- | --- |
| No change, clients drain global FIFO | Simplest transport | Request-scoped reader can never make independent progress |
| New correlated capture policy / method | Explicit opt-in | Extra concept/API for behavior already implied by request-scoped read |
| Reorder capture queue at publication | Permits matching | Breaks order for all consumers and cannot guarantee arbitrary request ordering |
| **Use existing request-scoped method to select its first matching buffered packet** | Minimal, reuses existing `pop_at` and typed validation; leaves global FIFO unchanged | Allows scoped reads past unrelated heads; bounded linear search of capture capacity |

Only `take_request_output` changes: validate live RequestId and output key, find earliest packet with authenticated request metadata, validate it in place, pop its exact offset, run `observe_capture`, then `refresh`. Missing request packet returns `Ok(None)` with no mutation or callback. No implicit sorting or reprioritization of future publications.

This is a corrected request-scoped retrieval contract. `take_output` stays the strict FIFO API. A single-capacity queue with an older captured packet **still** blocks another request's future publication: this deliberately does not assert scheduling fairness or solve upstream active-job interference. Such fairness requires a separate bounded policy and tests.

## Acceptance

1. Two captured outputs buffered in FIFO A,B: `take_request_output(B)` returns B without first consuming A; `take_output` then returns A. Correct request metadata, values, and independent settled statuses.
2. Foreign RequestId cannot consume a packet, same-request capture order preserved, and exact-output validation must precede removal.
3. Empty matching scan is side-effect-free; no new queues or privileges.
4. CI passes exact candidate commit; remaining H7 starvation/backpressure/failure-recovery limitations documented.
