# Phase 2 — adversarial review of H7 scoped capture extraction

2026-11-12. Independent review of `fbp-h7-capture-matching-design-20261112.md`.

1. **Authentication, not payload IDs.** Match only `CaptureMetadata::request()` and verify using `validate_capture(id, name, packet)` **before removal**. Foreign/stale/terminal scopes are errors without queue mutation; malformed matching captures must not be silently skipped.
2. **Output counter consistency.** Existing global `take_output` calls `observe_capture` when consuming. Scoped extraction must also call it exactly once, and `refresh` exactly once, regardless of offset. Validate before removing, not afterward.
3. **Transport and order.** `pop_at(position)` has a bounded linear search, preserves ordering of all *other* packets and leaves capacity/writer state unchanged. Selecting the first matching packet preserves FIFO for repeated captures of one RequestId. Global `take_output` must remain strictly FIFO.
4. **Completion semantics.** `request_status` depends on queued packet inventory and capture accounting. Explicit tests should check statuses only after consuming both captures, and detect duplicates or accidentally discarded packet.
5. **Same-queue ownership.** Multiple request captures share one queue; there is no requirement that one client can consume other request packets. On missing match, return `None`, not someone else's output, and do not call `refresh`.
6. **Limits.** This fixes *buffered capture retrieval* head blocking, not active generation fairness, capacity-one capture backpressure, source cancellation semantics, shared mutable state or complete H7 request isolation. Do not claim closure of H7 or M7.
7. **Design protocol acceptance.** One existing method + two existing internal primitives is smaller than a new capture-policy enum, indexed scheduler or public cursor. Accept bounded O(capacity) search; compile and exact-head CI must independently validate.

**Disposition:** Implement without new types, public methods or compatibility aliases; retain H7 partial until dynamic fairness/capacity and isolation criteria are addressed.
