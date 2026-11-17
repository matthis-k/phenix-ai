# Remaining Finding And WIP Work Map

## Current Acceptance: Q7, 2026-11-17

Original Q7 is **scoped verified** at
`6f6e751b36f77273da8799900f665bc026447418`, following independent whole-slice
reviews, all local gates (Nix snapshot and actual committed-head flake check
included), and all fifteen exact-head hosted workflows confirmed SUCCESS via `gh`.
See [final Q7 acceptance and all run URLs](fbp-q7-final-review-20261117.md).
This accepts the full process/Value/guards/helpers/codecs/macros/adapters/handoff/
runtime/request/session/Kernel/StorageGraph source local Failure DATA/query
protocol, typed framework reasons and plugin lifetime errors, not a narrowed core.

Current counts are **19 scoped verified / 6 partial / 7 open / 1 approved
exclusion**, leaving **13 in-scope findings**. The [authoritative scorecard](fbp-issue-progress-estimates-20261113.md)
records Q7 **26.0% -> 100.0%, +74.0 pp**, and the subjective mean
**75.4% (2489 / 33)**, not product completion. C7 remains excluded with its old
12.0% unchanged and no sandbox credit. Prior Q1 acceptance, including the broader
Kernel concentration residual, is unchanged.

The six partial findings are H1, H3b, H4, H5, H6 and H7; the seven open findings
are A1, A2, A5, Q2, Q3, Q4 and Q5. Known leases remain per escaping handle and
nonretroactive, cached observation remains passive, and ordinary Drop cannot report
retroactively. Trusted native abort exclusions remain; green benchmark execution
does not establish performance acceptance or M7/M8 completion.

This current section supersedes earlier counts, Q7 development/pending labels and
the historical Q7 row below. All 22 starting rows and prior acceptance narrative
remain for traceability, not as a second current status authority. This evidence-only
docs revision requires its own checks; no later implementation is accepted here.

## Historical Work Map And Acceptance Sequence

Starting point: verified `30a62f706`, 22 remaining findings: seven partial and
fifteen open. The [33-finding scorecard](fbp-issue-progress-estimates-20261113.md)
remains authoritative. This map supplies concrete work and acceptance dependencies,
not new completion percentages or a promise that every finding is independently
implementable without a policy decision.

Resource/lifecycle acceptance: `026cfbe05`, with all fifteen exact-head hosted
workflows passing. At that stage A3 became scoped verified, leaving **21 findings: 7 partial and
14 open**, with twelve scoped verified overall. Native WIP #1258 is closed as
superseded, and the approved terminal/cleanup protocol is implemented and verified.
The table retains all 22 starting findings so no remaining work disappears from
the map; only A3 had received new whole-finding closure credit at that stage.

Subsequent accepted implementation `006b2e9cc` fixes Q3's demonstrated selection
defect with exact unique-model selection and finite host-owned work limits. All
fifteen workflows passed. This is material correctness and bounded-selector
evidence, not whole Q3 planning/recomposition scaling acceptance; the 21 remaining
findings and scores do not change. See [selection design/evidence](fbp-selection-correctness-20261115.md).

Later implementation `89171a843` passed all fifteen exact-head workflows. Whole-
scope reviews accept A4 canonical authoring equivalence, Q6 bounded passive
explanations and the user-approved C3 LeasedArchival, C5 Cooperative and C6 Restart
And Persist contracts. C7 TrustedNativeScope is separately **excluded**, not an
implemented or verified sandbox. Current classifications are **17 scoped verified /
6 partial / 9 open / 1 excluded**, with **15 remaining in-scope findings**.
The [scorecard](fbp-issue-progress-estimates-20261113.md) owns numerical accounting;
C7's old estimate remains unchanged and receives no implementation credit.

Q1's original three graph responsibility concentrations are subsequently scoped
verified at `7c6119122`, with all fifteen workflows passing. Counts are now
**18 scoped verified / 6 partial / 8 open / 1 excluded**, leaving **14 in-scope
findings**. The [ownership record](fbp-owned-boundaries-20261116.md) preserves the
broader Kernel root concentration as an explicit residual, not a completed claim.

## Current Implementation Tracks

- A3: remove graph-owned Store, locks and read/write policy in favor of explicitly
  granted opaque application capabilities. Whole-finding scoped verification after
  independent review and exact-head acceptance. See [boundary design/review](fbp-resource-boundary-20261115.md).
- Terminal cleanup: preserve inherited outcomes and original causes while explicit
  authenticated request termination disposes its remaining work. The user approved
  **Dispose, Keep Failed**. Preserve typed query cleanup failures and job inventory.
  See [terminal contract](fbp-terminal-cleanup-20261115.md).
- WIP #1258: adapt source/publication-driven host wakes to native FBP, retaining
  static-driver and derived guard contracts. No old Engine or new scheduler.
  See [native design and limits](fbp-published-host-wake-20261115.md).

Those resource/lifecycle tracks passed hosted acceptance at `026cfbe05`. The
scorecard, not this map, records that A3 increase, the five later acceptances
at `89171a843`, and Q1's acceptance at `7c6119122`; all other finding scores,
including excluded C7, are unchanged.
Q3's correctness unit is now accepted; remaining Q3 work is complete planning and
recomposition scale measurement/acceptance, not another provisional-defeat patch.
The later explicit C3/C5/C6/C7 approvals above supersede the pending-policy labels
from that earlier stage. Q1's three-root ownership decomposition is accepted at
`7c6119122`. Q7's complete typed-failure migration is now being developed in an
isolated worktree; its core alone receives no closure credit.

## All 22 Findings

| Finding | Starting State | Concrete Next Work / Closure Requirement | Dependency Or Decision |
| --- | --- | --- | --- |
| H1 FIFO correlation | Partial | Define arrival skew/multiplicity and demonstrate supported crossed joins, staged waits, EOS and cancellation under those bounds | Strict global FIFO cannot also promise arbitrary crossed-stream progress |
| H3b lifecycle interleavings | Partial | Extend actual-source ownership verification to producer drain, retirement, driver ownership and worker return, retaining real-thread adversaries | Q5; arbitrary plugin-owned locks remain outside kernel control |
| H4 scheduling scans | Partial | Measure supported addressed-event scaling and remove demonstrated unnecessary scans, retaining required global/polled Custom fallbacks | H6; no scheduler rewrite merely to remove every scan |
| H5 closure analysis | Partial | Meet agreed closure cost/scaling budgets while preserving absence, cycles, observation and deduplication | H6; exact paired current-head measurements before further optimization |
| H6 profiling acceptance | Partial | Representative latency/throughput, allocation/live-memory and concurrency budgets with exact comparable sources | Runtime acceptance is distinct from missing production model/tool integration |
| H7 isolation/fairness | Partial | Specify stalled-continuation/shared-state behavior, resource/byte bounds and progress assumptions; demonstrate supported independent progress | Queue lanes alone do not isolate mutable process state or external effects |
| C5 deferred watchdog | Partial, now scoped verified | User-approved cooperative supervision plus full ticket/deadline/policy acceptance at `89171a843` | No autonomous abandoned-host service, callback preemption or worker/effect reclamation |
| C3 retired-generation policy | Open, now scoped verified | User-approved LeasedArchival, with integrated retained request/clone/job authentication and exact leases | Retirement stops host-owned producers, not explicit archived execution; strict revocation not claimed |
| C6 migration/restart | Open, now scoped verified | User-approved Restart And Persist plus real independent SQLite reopen/CAS/schema/candidate-failure and archived-job proof | Fresh process-private successor state; durable continuity is explicit and no live migration or exclusive writer is claimed |
| C7 plugin isolation | Open, now excluded | User-approved TrustedNativeScope excludes untrusted native isolation | No OS sandbox was implemented or verified; exclusion earns no numerical implementation credit |
| A1 private extension | Open | Add a concrete author-approved nested Add/Connect/export seam, exact host grants, conflicts, provenance and failure atomicity | Existing nested replacement seams are not general extension authority |
| A2 compatibility | Open | Define the safe compatibility relation with positive/negative examples, then validate adapters/replacement and inherited obligations | Shape agreement alone cannot manufacture semantic contract membership |
| A3 graph/store separation | Open, now scoped verified | Concrete storage/permission/synchronization authority removed from Graph; capability identity, attenuation and cleanup/leases preserved | Exact-head acceptance at `026cfbe05`; no Graph-to-Storage dependency or relocated duplicate Store |
| A4 typed authoring | Open, now scoped verified | Canonical authority audit and paired real manual/key/derived executions accepted at `89171a843` | Necessary validated views/convenience APIs retained; no second authoring model or Engine shim introduced |
| A5 physical paths | Open | Cover required compilation/binding/execution operations with coherent bound handles and stale/foreign rejection | Typed InputKey is a name, not generation authority; retain necessary validated dynamic paths |
| Q1 responsibility concentration | Open, now scoped verified | Original network/composition/runtime roots decomposed into cohesive private ownership modules at `7c6119122` | Canonical state/public paths/drop order and behavior preserved; broader Kernel root concentration is explicitly residual |
| Q2 incremental compilation | Open | Define dependency-aware invalidation/cache identity; compare reused candidates against full fresh compilation and measure benefit | Q3 correctness and stable authoring/replacement semantics first |
| Q3 selection/scaling | Open | Exact global selection, explicit zero/multiple/exhaustion handling and bounded-selector scale evidence accepted at `006b2e9cc`; complete planning/recomposition scale acceptance remains | Allowance excludes ancestor/footprint planning and replay/compile; no whole-finding closure |
| Q4 static liveness | Open | Define an analyzable subset with sound reject/unknown/progress results and explicit environment assumptions | Arbitrary callbacks and every SCC cannot receive a universal progress claim |
| Q5 concurrency evidence | Open | Cover supported drain/retirement/driver and handoff/wake compositions with actual-source bounded models plus deterministic threads | H3b; more repetitions alone are not exhaustive verification |
| Q6 diagnostic completeness | Open, now scoped verified | Complete bounded passive process/request explanations accepted at `89171a843`, sharing actual selector and closure predicates | Custom/future behavior remains explicitly unknown; current-state explanation is not Q4 liveness |
| Q7 typed failures | Open | Migrate a coherent process-to-handoff-to-session/request-to-Kernel/StorageGraph failure slice with original causes and code leases | Current source forwarding and Cleanup envelopes are material but not a complete typed process protocol |

## GitHub WIP Disposition

The remaining finding count is not a count of open GitHub PRs. Many older PRs are
the already-integrated stacked lineage; closing or merging all of them would not
implement these findings and is not part of this task.

- #1257 independent storage capabilities: completed native adaptation, closed as
  superseded by #1302, not merged into main.
- #1259 composer-owned atomic registration: completed native adaptation, closed as
  superseded by #1302, not merged into main.
- #1258 published-generation wake hosting: native adaptation passed independent
  original-scope acceptance and exact-head CI at `026cfbe05`; closed as superseded,
  not merged into main.
- #1301 coordinated CI builders: separate CI proposal, not an FBP correctness
  finding or authority to replace this branch's verified gates.
- #1302 remains the assembled native campaign. No merge into main is authorized.

## Acceptance Discipline

Keep existing fail-fast session contention, ordered worker guards, whole-drain
retirement, exclusive driver ownership, authenticated request/capture authority,
first-terminal meaning and code-lease cleanup. A new API must not become a second
scheduler or authority. Public breaking changes remove obsolete routes rather
than adding speculative compatibility layers.

Each material track requires a falsifiable scope, constraints/alternatives,
independent critique, implementation, real failure-path tests, final independent
diff review and exact-head hosted evidence. Whole-finding credit follows that
finding's complete acceptance, not the number of changed files or green workflows.
Benchmark execution is not blanket performance acceptance; runtime acceptance is
not M7/M8 production-agent completion.
