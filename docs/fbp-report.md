# Native FBP — implementation, acceptance and remaining gaps

**Reconciled 2026-11-12** against **stacked candidate PR #1274** at
`f81b49336dfce942f57dad1edc65add258be35fd`,
[exact-head six-job CI #43279922626](https://github.com/matthis-k/phenix-ai/actions/runs/43279922626),
**all checks successful**. See the [33-issue evidence ledger](fbp-issue-ledger-20261111.md)
for per-finding details. This report update itself is a distinct
documentation-only change requiring independent exact-head CI.

## Snapshot and branch provenance

- Native prototype baseline: `impl/graph-consolidation-20261110`
  at `89e00d06498979a2f231dfc8ca780eba44140d27`.
- Correctness and benchmark stack: PRs #1263, #1265, #1266 and #1267.
  These include the initial kernel panic/reentry guard, typed terminal
  results, cross-thread session/drain containment, native measurements,
  and a read-only idle-input scheduling prefilter.
- H3b/H7 reliability stack: [#1268](https://github.com/matthis-k/phenix-ai/pull/1268)
  host-drop reentrancy, [#1269](https://github.com/matthis-k/phenix-ai/pull/1269)
  correlated shared-session isolation regressions,
  [#1270](https://github.com/matthis-k/phenix-ai/pull/1270)
  request-scoped buffered capture extraction,
  [#1271](https://github.com/matthis-k/phenix-ai/pull/1271) combined CI,
  [#1272](https://github.com/matthis-k/phenix-ai/pull/1272)
  retired blocking-driver notification and post-retirement admission guard,
  [#1273](https://github.com/matthis-k/phenix-ai/pull/1273)
  exclusive blocking-driver vs direct-drain ownership, and
  [#1274](https://github.com/matthis-k/phenix-ai/pull/1274)
  capacity-one capture backpressure inspection/recovery tests.
- These are **unmerged candidate PRs** on a stacked lineage, not changes
  present on every older branch or the default branch. Independent
  exact-head checks verify specific candidate commits, not a release.

## Implemented native graph and composition model

The native FBP engine uses one `ComponentDef` primitive/composite model,
typed input/output interfaces, flattened compile-time composites and
`NetworkBuilder`-owned connections. `NetworkSession` persists between
admissions and supplies bounded typed queues, atomic fanout, EOS/capture
backpressure, owned deferred jobs, wakes, cancellation, request-scoped
work inspection and explicit capture extraction.

`NetworkPatch` composition is host-authorized and declarative. Kernel
contributions, code leases and plugin generations are integrated with
StorageGraph and typed/scoped storage. Legacy graph-wave execution APIs
and compatibility shims are not retained. An editable iterative-agent
composition demonstrates ordinary extension points; it does **not**
constitute a production model/tool/memory/streaming agent product.

## Correctness progress with exact evidence

| Area | Verified scoped change | Remaining |
| --- | --- | --- |
| Callback failure containment | #1263, [CI #43185486539](https://github.com/matthis-k/phenix-ai/actions/runs/43185486539), typed callback panic/terminal outcomes | `panic=abort` and external effects |
| Cross-thread session/drain cycle | #1265, [CI #43197663375](https://github.com/matthis-k/phenix-ai/actions/runs/43197663375), fail-fast session operations when callback would contend | External plugin mutex cycles |
| Host destruction during callbacks | #1268/#1271, [combined CI #43216093923](https://github.com/matthis-k/phenix-ai/actions/runs/43216093923), deferred cleanup outside mutexes | General external lock-order proofs |
| Driver retirement | #1272, [CI #43265588314](https://github.com/matthis-k/phenix-ai/actions/runs/43265588314), typed `Retired`, signal wake and no event admitted after self-retirement | Already dispatched external job effects |
| Producer ownership | #1273, [CI #43277415425](https://github.com/matthis-k/phenix-ai/actions/runs/43277415425), single atomic owner word for blocking drivers and direct drains, typed rejection in both directions | Comprehensive interleaving/model checking |
| Shared-session request isolation | #1269, [CI #43212196495](https://github.com/matthis-k/phenix-ai/actions/runs/43212196495), crossed inputs and queued cancellation; #1270 authenticated non-head capture reads | Active shared effects, fairness and policy |
| Saturated output diagnostics | #1274, [CI #43279922626](https://github.com/matthis-k/phenix-ai/actions/runs/43279922626), `RequestWork.capture_backpressure`, capacity-one recovery by explicit consumption/cancellation | No independent progress if the one-slot lossless buffer stays occupied |

A full queue with an unconsumed older capture cannot publish newer
captures without freeing capacity or **explicitly budgeting more storage**.
The implementation never automatically discards another request's packet.
H7 remains **Partial**, not proven fair in every shared-session schedule.

## Size and measured performance

[fbp-measurements.md](fbp-measurements.md) previously reported a historical
**15,688 → 7,645 non-test graph/derive physical LOC** between an older graph
snapshot and one native FBP snapshot. This is **not** the current HEAD LOC
audit and not a performance speedup.

[fbp-performance-benchmark.md](fbp-performance-benchmark.md) records the
initial native release-mode sparse-input benchmark with three repetitions
and raw CSV from [CI #43201132241](https://github.com/matthis-k/phenix-ai/actions/runs/43201132241).
At one active ingress among 1–256 processes, within-run time increases
substantially as idle nodes increase. Later idle-input prefilter measurements
on AMD EPYC 7763 reported around **59.7 → 9.8 µs/event** for 256 processes
across *separate* runs. This is not a pinned matched-environment causal
benchmark; it remains an observed constant-factor improvement with O(V)
`refresh()` scanning still in place. No heap/RSS, multiple active branches,
correlated joins or production-agent performance budgets have passed
formal acceptance.

H5 now has a separate [paired public-API request benchmark](fbp-h5-paired-measurements-20261112.md)
from [run #43321033238](https://github.com/matthis-k/phenix-ai/actions/runs/43321033238).
On one AMD EPYC 9V74 shared runner, the *median of three* interleaved
repetitions at queue depth 1,024 with one filler scope reduced
absent-request work inspection from **2,340.4 to 24.1 ns/query (~97×)**,
and absent-request status from **2,091.1 to 18.3 ns/query (~114×)**.
At depth 8, read improvements were only ~1.4–1.6×; cold-start
admission results showed no consistent speedup and sometimes regressed.
The same benchmark source was compiled against exact pre-index and
indexed runtime commits. This validates one bounded request-inspection
workload, **not** upstream closure traversal, scheduler scaling,
allocator/RSS or production-agent throughput. H5 remains Partial.

## Milestone and issue status

| Milestone | Assessment |
| --- | --- |
| M0–M3 | Substantially implemented; native model/compile/composition tests pass |
| M4 | Persistent runtime and cancellation implemented; global fairness/scale acceptance incomplete |
| M5–M6 | Kernel/StorageGraph integration, legacy-engine removal and agent-loop composition demonstrated |
| M7 | **INCOMPLETE** — performance, liveness, concurrency evidence and policy decisions outstanding |
| M8 | Agent-loop composition proof, **not a production agent product** |

**33-finding status: 10 verified, 7 partial, 16 open**.
These categories are not a weighted completion percentage. In
particular H3b and H7 are still partial despite substantial verified
subproblem closures.

## Next priority decisions

1. H7: Specify whether an optional per-request capture reserve is required.
   If so, define an explicit total memory budget, order among global and
   request-scoped reads, cancellation disposal semantics and backpressure.
   Never treat an indefinitely nonconsuming host as guaranteed fair with
   a one-slot lossless queue.
2. H4/H5/H6: Establish pinned paired benchmark results and profile
   `refresh()`, closure analysis, allocator/RSS and correlated joins.
   Then compare a minimal event-indexed scheduler to the verified
   prefilter baseline; do not optimize unmeasured workloads.
3. H3b/Q5: Add interleaving/model-checked concurrency and explicit
   external plugin-lock contract tests.
4. C3/C6/C7: Resolve generation archival/revocation, migration/restart,
   and trusted versus sandboxed plugin policy without violating current
   code lease and session preservation guarantees.
5. A1–A6, Q1–Q7: Apply design, adversarial critique and exact-diff review
   with concrete agent-loop use cases.

**Disposition:** scoped native graph/kernel reliability is materially
stronger and CI-verified on the complete stack. No universal deadlock
freedom, static liveness, all-request fairness, zero-cost scaling,
automatic live-state migration, in-process plugin sandbox or production
agent acceptance is asserted.
