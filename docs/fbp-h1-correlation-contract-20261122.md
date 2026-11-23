# H1 — bounded request correlation and FIFO progress contract

This is the supported contract for the **opt-in** `Readiness::CorrelatedAll`
runtime policy. It does not add a new scheduler, hidden buffer, request authority,
automatic input reordering, or an unbounded-progress guarantee.

## 0. Constraints and non-goals

- Every request must be admitted through the authenticated
  `NetworkSession::begin_request` identity; application payload fields cannot
  establish correlation. The host sets finite `max_requests`, per-queue input
  capacities and output capture capacity.
- All active process instances remain **single-flight and stateful**. Bounded
  per-request FIFO lanes isolate transport queues, **not** mutable process
  instances, external effects or execution durations.
- `Readiness::All`, `Any`, `AtLeast` and `Custom` remain head-only. The
  default `SharedFifo` plus `All` does not silently become correlated.
- Global/unscoped packets are selected only at their physical FIFO heads;
  they cannot be manufactured into a request-local pair.
- There is no guarantee of admission when a configured bounded queue is full,
  even when another request could complete after more input. Backpressure is an
  observable `Full` error, not an invitation to exceed capacity.

## 1. Positive contract

For two inputs `left` and `right`, each requiring one packet, consider:

| Arrival on left | Arrival on right | CorrelatedAll |
|---|---|---|
| `A1, B1` | `B2, A2` | Pairs `A1+A2`, `B1+B2` with no cross-request mixing |
| `A1, A3, B1, B3` | `B2, B4, A2, A4` | Pairs `A1+A2`, `A3+A4`; `B1+B2`, `B3+B4` |
| `A1` | `B2, A2, A4` | A's initial pair and a same-owner staged `WaitInput(right)` may skip B's foreign head |
| `A1` | `unscoped, A2` | A may use `A2`; the unscoped packet remains head-only for an unscoped join |
| `A1`, then seal A | missing A right | No synthetic right input, premature join, or implicit completion |

The authenticated per-request projection of every input FIFO is preserved,
even when request packets are taken from behind foreign heads. A committed
activation consumes one matching input from each required port atomically.
An **exact staged** `WaitInput(port)` owned by `CorrelatedAll` uses the same
request-aware first-packet rule, rather than reverting to foreign-head-only
selection. This is a deliberately policy-local change: other waiting
continuations remain head-only on shared queues. BoundedLanes already routes
a staged request to its own lane.

If multiple scopes are fully eligible, round-robin candidate selection rotates
the bounded active correlation lanes; it cannot preempt a currently executing
process, revoke a capture, manufacture missing packets, or guarantee real-time
scheduling. Per-request packet order is mandatory; across different requests
there is **no global FIFO order** for this opt-in policy. Foreign heads are
never dropped just to create progress.

## 2. Lifetime, closure, and cancellation

- Closing a request seals ingress; it is not global EOS and does not supply
  missing required inputs. Staged waits still require their next own packet
  or actual closure.
- Closing a global input writer cannot erase already accepted packets. A
  process may observe genuine EOS only according to the existing port closure
  protocol, **after** its accepted work can be handled. These tests do not
  prove all possible EOS timing schedules.
- A partial or stalled owner remains single-flight. Explicit cancellation
  disposes that request's queued packets under the existing authenticated
  terminal policy. With `RestartAffectedProcess`, fresh process state then
  permits an eligible peer to continue. Default `FailSession` is not peer
  progress and must not be presented as such.
- There is no fair-progress promise for an unbounded stream of incoming work,
  a nonreturning plugin callback, an undrained capture, or an external effect
  blocking the process. These remain H3b/H6/H7 concerns.

## 3. Alternatives and selected approach

1. **Keep head-only staged waits:** preserves global FIFO but allows a foreign
   head to strand an already-owned `CorrelatedAll` continuation even when its
   authenticated next packet is physically buffered. This contradicts the
   opt-in request-correlated selection model and is rejected for this scope.
2. **Silently correlate every staged wait:** risks changing the default/other
   readiness policies and hiding head-of-line blocking. Rejected.
3. **Use the existing per-request packet selector only for CorrelatedAll's
   exact waiting continuation (selected):** keeps the bounded physical queue,
   original owner/session authority, existing per-request packet order and
   single-flight process state. No additional cursor or storage is introduced.

## 4. Falsifiable acceptance and remaining boundary

The public-API regression `fbp_h1_correlation_contract` runs both
`SharedFifo` and `BoundedLanes` for crossed multiplicity, foreign-head
staged waits (both preloaded and late-arriving own packets), unscoped-head
preservation, bounded backpressure, and explicit cancellation followed by
peer execution and EOS. It also challenges ordinary `All` under shared
FIFO to ensure that an existing exact staged wait is **not** silently made
correlated. It checks captured request identities and payloads, finite driver
budgets, settlement/release and no session failure.

In addition to representative tests, **all 24** arrival permutations of two
authenticated requests on two input ports are checked under both transport
policies, including **16** patterns that optionally drive one process step
after each injection: `24 * 16 * 2 = 768` bounded execution schedules.
These are deterministic finite integration executions, not an exhaustive
Loom concurrency proof or an assertion about unbounded input arrivals.

A separate integration case routes both incoming streams through **two
ordinary stateful FBP producer processes and their internal queues**, proving
that correlation metadata and per-request FIFO survive multiple edges rather
than only direct injection into the join.

A further SharedFifo regression keeps one correlated scope continuously
refilled while the other has a finite standing backlog. It requires alternating
owner selection during the complete overlap window, then ordered consumption
of the refilled remainder. This verifies **cooperative bounded work
conservation**, not real-time scheduling of infinite arrivals.

The already existing `fbp_request_lanes` regressions separately guard strict
FIFO `All`, correlated round-robin rotation, bounded unscoped opportunities
and closure-only request progress.

Accept H1's **bounded supported-scenario** contract only after exact-head
formatting, default/no-default Graph tests, Flake CI and relevant concurrent
runtime verifiers pass, plus independent code/contract review. This contract
does not establish universal arbitrary arrival-skew fairness or H7 independent
process-state isolation. Older issue percentages remain unchanged until the
authoritative 33-issue scorecard is explicitly reconciled.
