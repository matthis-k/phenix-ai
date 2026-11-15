# Remaining Finding And WIP Work Map

Starting point: verified `30a62f706`, 22 remaining findings: seven partial and
fifteen open. The [33-finding scorecard](fbp-issue-progress-estimates-20261113.md)
remains authoritative. This map supplies concrete work and acceptance dependencies,
not new completion percentages or a promise that every finding is independently
implementable without a policy decision.

Current accepted implementation: `026cfbe05`, with all fifteen exact-head hosted
workflows passing. A3 is now scoped verified, leaving **21 findings: 7 partial and
14 open**, with twelve scoped verified overall. Native WIP #1258 is closed as
superseded, and the approved terminal/cleanup protocol is implemented and verified.
The table retains all 22 starting findings so no remaining work disappears from
the map; only A3 has received new whole-finding closure credit.

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

These tracks passed hosted acceptance at the exact revision above. The scorecard,
not this map, records the A3 increase and unchanged scores of the other findings.
Q3 selection correctness is the next executable correctness unit; C3/C6/C7 decisions
remain outstanding and must not be silently converted into implementation claims.

## All 22 Findings

| Finding | Starting State | Concrete Next Work / Closure Requirement | Dependency Or Decision |
| --- | --- | --- | --- |
| H1 FIFO correlation | Partial | Define arrival skew/multiplicity and demonstrate supported crossed joins, staged waits, EOS and cancellation under those bounds | Strict global FIFO cannot also promise arbitrary crossed-stream progress |
| H3b lifecycle interleavings | Partial | Extend actual-source ownership verification to producer drain, retirement, driver ownership and worker return, retaining real-thread adversaries | Q5; arbitrary plugin-owned locks remain outside kernel control |
| H4 scheduling scans | Partial | Measure supported addressed-event scaling and remove demonstrated unnecessary scans, retaining required global/polled Custom fallbacks | H6; no scheduler rewrite merely to remove every scan |
| H5 closure analysis | Partial | Meet agreed closure cost/scaling budgets while preserving absence, cycles, observation and deduplication | H6; exact paired current-head measurements before further optimization |
| H6 profiling acceptance | Partial | Representative latency/throughput, allocation/live-memory and concurrency budgets with exact comparable sources | Runtime acceptance is distinct from missing production model/tool integration |
| H7 isolation/fairness | Partial | Specify stalled-continuation/shared-state behavior, resource/byte bounds and progress assumptions; demonstrate supported independent progress | Queue lanes alone do not isolate mutable process state or external effects |
| C5 deferred watchdog | Partial | Accept cooperative host-driven supervision or specify autonomous lifetime requirements, then close demonstrated detection/recovery gaps | Cannot preempt arbitrary worker code; keep state queries passive |
| C3 retired-generation revocation | Open | Select archival, drain-only or strict revocation semantics across retained generations, compiled sessions and jobs | Owner policy decision; no silent revocation of shipped archival behavior |
| C6 migration/restart | Open | Accept restart-only with versioned durable-state continuity, or design export/import/quiescence and atomic migration | Owner policy decision; fresh state and failure atomicity are already tested |
| C7 plugin isolation | Open | Implement an executable process-isolation capability boundary with crash, cancellation and resource limits, or approve exclusion | Untrusted native Rust cannot be sandboxed in-process by typed grants |
| A1 private extension | Open | Add a concrete author-approved nested Add/Connect/export seam, exact host grants, conflicts, provenance and failure atomicity | Existing nested replacement seams are not general extension authority |
| A2 compatibility | Open | Define the safe compatibility relation with positive/negative examples, then validate adapters/replacement and inherited obligations | Shape agreement alone cannot manufacture semantic contract membership |
| A3 graph/store separation | Open, now scoped verified | Concrete storage/permission/synchronization authority removed from Graph; capability identity, attenuation and cleanup/leases preserved | Exact-head acceptance at `026cfbe05`; no Graph-to-Storage dependency or relocated duplicate Store |
| A4 typed authoring | Open | Inventory actual surviving representations, eliminate redundant authoring routes and prove manual/derived primitive/composite equivalence | Do not reintroduce removed Engine authoring conveniences as compatibility shims |
| A5 physical paths | Open | Cover required compilation/binding/execution operations with coherent bound handles and stale/foreign rejection | Typed InputKey is a name, not generation authority; retain necessary validated dynamic paths |
| Q1 responsibility concentration | Open | Extract cohesive tested ownership boundaries without duplicated state or protocol authorities | Stabilize terminal/selection seams before competing refactors |
| Q2 incremental compilation | Open | Define dependency-aware invalidation/cache identity; compare reused candidates against full fresh compilation and measure benefit | Q3 correctness and stable authoring/replacement semantics first |
| Q3 selection/scaling | Open | Correct transient dependency defeat with termination, zero/multiple-solution handling and adversarial scale evidence | Reproduced expected `{P,R,p}` versus actual `{R,p}`; blind exclusion resets or unbounded subset search are not accepted |
| Q4 static liveness | Open | Define an analyzable subset with sound reject/unknown/progress results and explicit environment assumptions | Arbitrary callbacks and every SCC cannot receive a universal progress claim |
| Q5 concurrency evidence | Open | Cover supported drain/retirement/driver and handoff/wake compositions with actual-source bounded models plus deterministic threads | H3b; more repetitions alone are not exhaustive verification |
| Q6 diagnostic completeness | Open | Structured passive explanations for why a process/request cannot run/settle, including readiness, closure, jobs, wake, continuation and publication | Distinguish unknown conditions from proven blockage; preserve exact request/lane attribution |
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
