# FBP whole-drain producer lifecycle — 2026-11-11

Status: Phase 1 design note, baseline `0003c4fb0d3b5392c1fee37631f4453d2977d183`.
Scope: producer polling/admission and generation retirement; no graph execution
or queue semantics change. Follow the [FBP design](fbp-design.md), [migration
ledger](fbp-migration.md), and [previous failed prototypes](fbp-followup-design-20261111.md).

## 0. Constraints and problem

`Kernel::drain(N)` currently keeps `ProducerState` locked across every poll,
`NetworkSession::inject` and synchronous `ReadinessPolicy::select`. A plugin
readiness callback or producer that reenters producer-inventory inspection
deadlocks. Yet `KernelHost::publish` must not interrupt an active drain *between
polls*, and each successful admission must eagerly reserve queue credit.
Nonblocking producer event retention and source-code leases are established.

Doing nothing leaves callbacks under an inventory mutex. Removing eager
readiness breaks `fbp_streams`; unlocking per poll breaks
`generation_retirement`. Neither is acceptable.

## 1. Design alternatives

| Alternative | Pros | Blocking limitation | Verdict |
|---|---|---|---|
| Existing global producer mutex | Serializes drain + retirement with one lock | User callbacks run while inventory locked | Reject |
| Lazy enqueue | Simple | Changes eager capacity + cancellation | Reject |
| Per-poll lock release | Small diff | Retirement cuts a drain mid-flight | Reject |
| Single whole-drain gate, short inventory locks | Preserves the whole operation and eager scheduling; callbacks outside inventory lock | Requires an explicit in-flight producer slot | Selected |

The proposed `Kernel` has one `Mutex<()>` **drain gate** serializing complete
`drain` calls with generation retirement. `ProducerState` remains the sole
source of producer inventory, round-robin cursor, pending packet and retirement
status. Its producer slot is an `Option<Box<dyn Producer>>`; while one poll is
active the slot stays in the inventory as `None`, not secretly duplicated.
The drain gate ensures no other drain or retirement can replace that slot
mid-operation. Each iteration:

1. Hold drain gate for the whole `drain(N)`.
2. Under short inventory lock choose the next slot, advance cursor and **take**
   its producer plus at most one pending InputEvent. Unlock inventory.
3. Perform `poll` (unless using pending event) and eager
   `NetworkSession::inject` without inventory mutex, using existing
   `try_lock` on session for nonblocking admission.
4. Re-lock inventory, restore producer or remove failed poll source, preserve
   exactly one pending event on Full/contended session, update cursor. Drop
   detached sources only outside producer inventory lock.
5. Release drain gate after the whole drain including last poll, preserving
   retirement atomicity. Host publish and drop take the drain gate *before*
   taking producer inventory; release both before destructors run.

This is *one* lifecycle serialization boundary and *one* inventory. It does not
add an alternate scheduler, buffered channel, producer pump, or request authority.

## Error/panic and reentrancy boundaries

A producer panic is terminal to that producer and is converted to
`ProducerError::Poll` with a code-leased error. No producer is left absent
from its inventory slot when `drain` returns. A process/readiness panic is
handled by the existing graph session where supported. Reentrancy of **producer
inventory inspection** from `poll` or readiness must be safe.

A callback cannot synchronously nest a *second drain* or *publish* in the same
active drain: that would violate exclusive `&mut Producer` ownership or the
whole-drain retirement rule. The new implementation must not claim recursive
drain/publish support. If an available API would permit that usage, add an
explicit fail-fast Busy outcome in a later operation-specific design; never
silently fabricate progress. Application code must not reenter the same session
mutex while a session callback is executing.

## Acceptance evidence

- Existing `native_sessions::full_retains_exactly_one_event...` unchanged.
- Existing `generation_retirement::blocked_poll_finishes...` unchanged.
- Existing fair polling and terminal-error/drop/lease regressions unchanged.
- A producer's `poll` can `try_lock` its inventory (no deadlock).
- A readiness callback entered by producer admission can `try_lock` inventory.
- Deferred session lock leaves one pending event and does not repoll.
- Startup preflight remains intact; no duplicate transports or public API shims.
- Phase 4 diff review and exact-head CI.

## Out of scope

Request correlation/skew, private nested editing, arbitrary recursive callbacks,
distributed transport and noncooperative execution cancellation are separate
unresolved design tasks. Their status is unchanged by this patch.
