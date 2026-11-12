# H1 Phase 2: adversarial review of bounded correlated join

1. **Violation of FIFO:** intentional and *only* for explicitly configured
   `CorrelatedAll` scoped requests. Strict default remains head-only.
2. **Reordering within one request:** forbidden. A queue scan picks its first
   matching `RequestId`, never a later same-request packet.
3. **Unscoped global traffic:** must remain head-only, or the policy becomes
   an unadvertised priority scheduler for nonrequest data.
4. **Multiple producers per port:** deterministic cursor search finds the
   first eligible queue in physical cursor order. This is not a global
   timestamp order across producers; no such order existed before.
5. **EOS and staged readiness:** `CorrelatedAll` has the same required input
   closure/error check as `All`. A separate `WaitInput` remains exact
   head-only; do not change it implicitly.
6. **Fanout and capture backpressure:** only the reservation's packet location
   changes; publication and capture use existing bounded transactions.
7. **Cancellation and request authority:** metadata is created internally
   and authenticated; selecting a request never relabels packet metadata.
   Cancellation retains order of other requests through `retain`.
8. **Fairness/performance:** this is O(queue occupancy), and a continuously
   supplied favored request may starve another. No fairness guarantee until
   adversarial stress testing. Preserve H4/H6 and Q5 as open.
9. **Lease safety:** removing a packet from its queue transfers the existing
   owned `Packet` and its code lease, never a clone or borrowed pointer.

Accepted only as an **opt-in** shared-session correlation solution with
distinct tests and no claim of global FIFO preservation in that mode.
Run Phase 4 checks before calling the issue verified.

## Phase 4 test correction — capture FIFO remains global

The initial crossed-head regression read request A's capture before B.
However B became eligible and was *admitted* as soon as the third packet
arrived, while A's last input was still missing. B can legitimately publish
first; the exported capture FIFO remains global and cannot skip B to hand
A's packet out-of-order.

CI 43147820467 caught this mistaken test assumption. The revised test
drains the capture FIFO in its real order, then verifies each packet's
authenticated `RequestId` and exact value, independent of request
completion order. This is a test correction, not a weakening of output
FIFO or a scheduler-order guarantee.
