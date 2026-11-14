# H7 capture-backpressure diagnostics — Phase 2 adversarial review

Date: 2026-11-12. Review of `fbp-h7-capture-backpressure-design-20261112.md`.

1. **Only uncommitted targeted publications.** A full output FIFO is not automatically a request-local blocker. Count it only if a process belonging to the requested `RequestId` has a retained `pending` transaction with that exact capture queue in `pending_targets`. This must match the actual publication dependency used by `refresh()` and `blocked_queues()`.
2. **No cross-request data leaks.** The field reports a count, not another request's private identity or payload. Continue authenticating requests through `check_request`; foreign/stale IDs must error. Avoid new privileged inspection APIs.
3. **No double count.** Distinct fanout aliases should count physical queue indices, not names or packets. Use an existing bounded `BTreeSet<usize>` or a direct count if one process can never duplicate targets, but explicitly test the invariant if relying on that assumption.
4. **No implicit progress claim.** A cap-one full capture is a real backpressure dependency. Do not bypass fanout commit, discard A, or add hidden per-request buffering just to make B appear settled. The metric is not a guarantee that queue consumption resolves all remaining work.
5. **Cancellation correctness.** Cancelling A after its capture was committed must mark its terminal state, dispose only its queued capture, and increment `RequestDisposal.queued_packets`; it must not cancel B or reset state. Existing default `FailSession` policy applies only if A owns *active* processes. Prove test A is not active before cancel.
6. **Snapshot purity.** `request_work` is passive and must not invoke `selection`, `refresh`, producer callbacks, or mutate the transport. Repeated calls produce the same result until explicit state changes.
7. **Favorable tradeoff.** One field in an already public diagnostics struct plus tests has lower conceptual cost than a new scheduler, queue partition, or request-specific error type. Performance adds a bounded scan of pending process targets to an existing O(processes + queued packets) diagnostic request; no fast scheduler path changes.
8. **Observed limitations.** The blocked publication can be caused by a full queue owned by the *same* request; do not describe the count as necessarily foreign-owned or a fairness proof. Queue capacity budget and global FIFO remain unchanged.

Disposition: accept this scoped improvement. Phase 4 must verify cross-request capture, cancellation and ordinary host consumption, exact-head CI, and leave H7 partial.
