# H1 — final bounded correlation acceptance and adversarial review

Date: 2026-11-23. Finding: **H1, Shared-session FIFO correlation**.
Final tested implementation source: `2cda718a723bc51a793c125a0bbc26ba78ecfac5` on
draft [PR #1386](https://github.com/matthis-k/phenix-ai/pull/1386).
Original finding: preserve shared-session FIFO safety without conflating
requests, while documenting whether and when crossed input streams make
progress. Original score: 60.0%, Partial.

## Accepted scope and decision

**Scoped verified, 60.0% -> 100.0% (+40.0 percentage points)** for
bounded, authenticated, *opt-in* `Readiness::CorrelatedAll`.
This is an engineering finding-acceptance classification, not a promise of
unbounded scheduling fairness or independent mutable process execution.

1. **Strict-default policy:** `Readiness::All`, `Any`, `AtLeast` and
   `Custom` remain head-only on SharedFifo. Global/unscoped packets never
   skip their own physical FIFO head. A strict global FIFO cannot promise
   arbitrary progress for crossed request streams.
2. **Opt-in correlation:** The join may select the earliest authenticated
   packet for a request on each input queue even behind a foreign head.
   Commit/reservation uses the existing bounded transport. The per-request
   projection of every physical queue stays in order, while no cross-request
   global FIFO order is promised.
3. **Continuation consistency:** An exact `WaitInput(port)` already owned by
   an opt-in `CorrelatedAll` process uses that same request-local selection,
   not the strict head-only lookup. No new queue, scheduler, second authority,
   fallback API, memory buffer or legacy compatibility shim was introduced.
4. **Fairness is conditional:** Given distinct admitted requests with complete
   input pairs, cooperative finite process steps and continued driver
   execution, the existing bounded lane cursor rotates eligible requests.
   A process waiting for a missing required packet is not made runnable by
   another request's packet. Finite queue capacity and capture backpressure
   remain authoritative; `Transport(Full)` is the expected bounded outcome.
5. **Lifetime:** Sealing ingress does not synthesize required packets or
   global EOS. Explicit cancellation is scoped disposal; if a process was
   actively owned, `RestartAffectedProcess` must be enabled for peer
   continuation, while `FailSession` intentionally fails the session.
   External effects and worker completion are not preempted.

## Separate final adversarial diff review (no external reviewer implied)

This pass reviewed the actual proposed production diff, source selection
and reservation flow, the 11 public tests, the contract, and the final
exact-head CI results, separately from original test authoring.

| Review question | Evidence | Finding |
| --- | --- | --- |
| Does the new selector require authenticated request identity? | `exact_wait_selection` passes existing `p.request` into `packet_for`; correlated scanning only applies when that request is `Some` | No new authority or payload-based correlation |
| Is matching stable with foreign heads? | `packet_for` picks the first matching packet; initial group and exact continuation use the same helper; public crossed and delayed-wake tests | Request-local FIFO projection preserved |
| Could this accidentally change ordinary `All`? | `correlated` is true only when active readiness policy is `CorrelatedAll`; explicit ordinary-`All` staged negative test | Strict default preserved |
| Can unscoped packets be stolen? | `packet_for` only permits off-head selection for `request.is_some()`; unscoped-head preservation test | No |
| Are new buffers or bypasses introduced? | One production lookup argument changes; existing `ConnectionError::Full` and physical queue sizes untouched; backpressure/credit test | No extra capacity |
| Can seals/EOS overtake accepted packets? | Both writer closures exercised while crossed packets remain; pending staged wait remains blocked until its own late packet arrives | Bounded tested paths pass |
| Does cancellation unstick another owner without corrupting retained packets? | Explicit `RestartAffectedProcess` test under both queue policies, with terminal identity/peer captures asserted | Bounded tested paths pass |
| Is finite fairness observed under refill? | Rotating SharedFifo test maintains both eligible requests with 8-packet queue capacity, refilling A after its turn | Alternation and per-owner FIFO verified |
| Are interleavings more than one hand-picked fixture? | 24 crossed permutations; 768 permutations with bounded driving cut patterns under both policies; real two-producer edge test | Supported deterministic input/drive schedules verified |

The initial refill test wrongly preloaded 12 shared-queue packets into a
capacity-8 queue and correctly received `Transport(Full)`. This was a
**test-authoring error**, not a scheduler failure. It was corrected at
`2cda718a723bc51a793c125a0bbc26ba78ecfac5` to eight initial
packets and replenishment only after actual capacity release.

This is a **standalone adversarial review by the implementation assistant**,
not a second human's independent approval. No external GitHub review
was present at acceptance. Integration review/merge remains separately
subject to repository policy.

## Exact-source verification

All 11 PR-head workflows completed **SUCCESS**, against
`2cda718a723bc51a793c125a0bbc26ba78ecfac5`:

- [Flake CI run 45087742886](https://github.com/matthis-k/phenix-ai/actions/runs/45087742886):
  exact-head inventory, shared Nix build, Graph shard-00, workspace shard-01,
  format shard-02 and final evidence aggregate — all six jobs passed.
- Graph shard `137410457452`: all **11 new public H1 tests passed (0 failed)**,
  including the 768 bounded arrival/drive executions, 24 direct permutations,
  real producer-to-join paths, delayed continuation input, staged strict
  default, EOS, cancellation/recovery, explicit physical backpressure and
  finite continuously-refilled fairness.
- Ten other workflows passed: Q5 Loom actual wake interleavings, native
  runtime benchmark, H4 notified custom, scoped Optional polling,
  EOS endpoint, mixed-agent, targeted wake and live sparse pairings,
  H5 scoped closure paired benchmark and H6 agent allocation events.

The authoring/reporting changes which record this acceptance must pass their
**own exact-head checks**; passing source-head checks cannot be reused as
proof that a later documentation commit passed CI.

## Boundary: what H1 does not close

- **H3b:** arbitrary plugin-owned lock interleavings, lifetime races and
  deadlock-freedom are not proved by these deterministic schedules.
- **H7:** shared process state and single-flight occupancy remain shared;
  no independent-progress guarantee for a stalled owner, and neither
  undrained captures nor external effects are forcibly recovered.
- **H4/H5/H6/Q4:** no scheduler throughput, closure scaling, allocation
  budget or static liveness acceptance follows from H1.
- There is no universal claim about infinite arrivals, arbitrary packet
  skew beyond configured credit, fair driver service or adversarial
  nonreturning process code.

The broader FBP campaign remains unmerged in the stacked draft PRs.
No user approval for mainline merge or rebase is inferred from this finding
closure. The separate [contract](fbp-h1-correlation-contract-20261122.md)
is the normative bounded behavior specification.
