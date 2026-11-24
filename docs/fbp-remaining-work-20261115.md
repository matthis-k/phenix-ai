# Remaining Finding And WIP Work Map

## Current Acceptance: H3b, 2026-11-24

Original H3b is **scoped verified: 78.0% -> 100.0%, +22.0 pp**, accepted
source `e166656379ac8837c7a51e1e0a3e84752ad42e85`. The
[whole-original record](fbp-h3b-whole-acceptance-20261124.md) maps the actual
kernel-owned reentrancy/lifecycle criteria to production ownership, five new
native composition tests, the reused OS-worker matrix, bounded exact-source
models and explicit wait-order reasoning. Independent whole-scope and exact-head
reviews found no remaining blocker. All 17 workflows pass, including six Flake
jobs and 12 session properties / 488 conclusive schedules. Later edits have
their own head's CI requirement, not inherited acceptance.

Current counts: **25 scoped verified / 4 partial / 3 open / 1 approved exclusion**,
**7 remaining findings**: H4/H5/H6/H7 and Q2/Q3/Q4. Sum **2819**, subjective
mean **85.4%**. H1/A1/A2 and C7's unchanged exclusion remain intact. Arbitrary
plugin-owned lock cycles were not the selected H3b contract and are not kernel
defects being waived.

## Previous Acceptance: A2, 2026-11-24

Original A2 is **scoped verified: 16.0% -> 100.0%, +84.0 pp**, accepted source
`1589ad0d2828fecea99f784ac8e64fe952e821bc`. Two independent original-record
audits found that explicit exact-boundary compositional adaptation satisfies
the original compatibility/projection criterion; the later alleged mandatory
direct-width requirement had no original source. The
[A2 record](fbp-a2-adapted-replacement-20261123.md) defines the safe relation and
maps real positive/negative replacement, schema conversion, claims/obligations,
backpressure, failed updates, old jobs and detached leases to hosted evidence.
All 17 exact-source workflows passed; this reconciliation head has separate
checks. No accepted invariant or original application promise is waived.

Current counts: **24 scoped verified / 5 partial / 3 open / 1 approved exclusion**,
**8 remaining in-scope findings**: H3b/H4/H5/H6/H7 and Q2/Q3/Q4. Scorecard sum
**2797**, subjective mean **84.8%**. A1/H1 and C7's unchanged exclusion remain.
Raw width binding, arbitrary semantics, static liveness, native sandboxing and
production-agent completion are not claimed.

## Previous Acceptance: A1, 2026-11-23

Original A1 is **scoped verified: 18.0% -> 100.0%, +82.0 pp**, source
`923ccd6376b4cd8e3980b61ae26ad78c08e9c6a9` on draft #1388. The
[acceptance record](fbp-a1-checked-nested-exports-20261123.md) maps the original
private-extension and executable-agent-loop criteria to real author-opted
Add/Connect/export replay, exact checked lowering, prepared Kernel integration,
both queue policies, failed updates, frozen claims and retained generations.
Independent whole-scope review found no remaining blocker. All six jobs of
[Flake CI 45249107088](https://github.com/matthis-k/phenix-ai/actions/runs/45249107088)
and both [Session Ownership 45249106208](https://github.com/matthis-k/phenix-ai/actions/runs/45249106208)
jobs passed on that exact source. These docs require their own head's CI.

Current counts are **23 scoped verified / 5 partial / 4 open / 1 approved
exclusion**, with **9 in-scope findings**: H3b/H4/H5/H6/H7 and A2/Q2/Q3/Q4.
The [assembled integration source](fbp-foundation-integration-20261123.md)
`8e96b31213e0ae9623f06814f45226db227d0b46` passes all 17 existing workflows,
including six-job Flake CI and the additional manually dispatched lifetime/H7
gates. This verifies the combined bounded source, not closure of those nine.
The scorecard owns the sum **2713** and subjective mean **82.2%**. H1 and C7's
approved exclusion are unchanged. Additive extension is not arbitrary destructive
rewiring, implicit interface compatibility, independent shared-request process
state, static liveness, sandboxing, or M7/M8 production completion.

## Previous Acceptance: H1, 2026-11-23

H1 **Shared-session FIFO correlation** is now **scoped verified
(60.0% -> 100.0%, +40.0 pp)** for the finite, opt-in, authenticated
`CorrelatedAll` contract. The [final standalone source/contract review and
exact-head verification record](fbp-h1-final-acceptance-20261123.md)
documents crossed joins, request-local staged waits, EOS, cancellation,
default strict FIFO, physical-queue backpressure, 11 public H1 tests,
24 input orders, 768 finite arrival/drive schedules and real producer edges.
All eleven hosted workflows passed at implementation source
`2cda718a723bc51a793c125a0bbc26ba78ecfac5`, including six-job Flake
[45087742886](https://github.com/matthis-k/phenix-ai/actions/runs/45087742886).
The later evidence-only edits require separate exact-head CI; no code
was merged or rebased.

Current counts are **22 scoped verified / 5 partial / 5 open /
1 approved exclusion**, leaving **10 in-scope findings**.
The [authoritative scorecard](fbp-issue-progress-estimates-20261113.md)
now totals **2631**, subjective mean **79.7% (2631 / 33)**.
The five remaining partial findings are **H3b, H4, H5, H6 and H7**,
and the five open findings remain **A1, A2, Q2, Q3 and Q4**.

H1 does not establish independent progress through shared process state,
preemption, arbitrary infinite-input scheduling, external plugin lock
correctness or static liveness. These remain H7/H3b/Q4 boundaries;
performance acceptance is separately H4/H5/H6. Existing C7
TrustedNativeScope exclusion and M7/M8 incomplete production status
are unchanged. The original historical work map below remains for
traceability and must not override this current classification.

## Previous Acceptance: Q5, 2026-11-20

Original Q5 is **scoped verified: 74.0% -> 100.0%, +26.0 pp** at
`4b765ddfbabf334895147c1f7bf8b5accfb96ee1`, following independent final
whole-scope and coverage reviews with no remaining findings, all local gates and
all fifteen exact-head hosted workflows confirmed SUCCESS via `gh`. The ten-check
untracked-inclusive Nix snapshot also passed after approved temporary-target
cleanup of an initial disk-exhaustion failure. See [final Q5 acceptance, model
counts and all run URLs](fbp-q5-protocol-verification-20261117.md).

The accepted bounded scope combines exact-source owner atomic/RAII verification,
real owned handoff histories, public identity/transport/worker composition and
preserved wake/retirement evidence. It includes ten session/ownership properties
with 472 schedules and no duration cap, five preserved wake properties and 13,440
serialized wake histories, plus 21 handoff anchors, 1,533 histories and 2,856
attempts (611 accepted / 2,245 refused). All eight outcomes are exercised. Seven
public worker tests include both queue policies and five fresh identity
checkpoints for a non-emitting old `Complete` result. The old two-Idle assertion
was an incomplete test-authoring intermediate corrected to three Idle/21 anchors;
there is no production defect/fix claim.

Current counts are **21 scoped verified / 6 partial / 5 open / 1 approved
exclusion**, leaving **11 in-scope findings**. The [authoritative scorecard](fbp-issue-progress-estimates-20261113.md)
records the score sum **2591** and subjective mean **78.5% (2591 / 33 = 78.515...%)**,
not product completion. C7 remains approved TrustedNativeScope with its old 12.0%
unchanged and no sandbox credit.

The six partial findings are H1, H3b, H4, H5, H6 and H7; the five open findings
are A1, A2, Q2, Q3 and Q4. H3b remains partial at 78.0% and receives no automatic
Q5 credit; the bounded owner/handoff lifetime evidence is not a universal H1/H3b
schedule proof. H7's older-request scoped-capture retention and shared-process
occupancy residual remains unresolved. No fairness, static-liveness or
performance acceptance follows. Trusted native abort exclusions and M7/M8 remain
incomplete.

Prior Q7, A5 and Q1 acceptances, including their disclosed residuals, are
unchanged; A1 remains open and design-only. This current section supersedes
earlier counts, Q7 development labels and Q5/A5 open/pending labels below. All 22
starting rows and prior acceptance narrative remain for traceability, not as a
second current status authority. This evidence-only docs revision requires its
own checks; no later implementation or evidence-only commit is accepted here.

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
`7c6119122`. Q7's complete typed-failure migration was developed separately and
is now scoped verified at `6f6e751b36f77273da8799900f665bc026447418`; its earlier
core-only stage received no closure credit.

## All 22 Findings

| Finding | Starting State | Concrete Next Work / Closure Requirement | Dependency Or Decision |
| --- | --- | --- | --- |
| H1 FIFO correlation | Partial, now scoped verified | Supported bounded skew/multiplicity and crossed joins, staged waits, EOS, cancellation, continuous refill and real producer edges accepted at `2cda718a723bc51a793c125a0bbc26ba78ecfac5` with all eleven exact-head workflows green; see [final review](fbp-h1-final-acceptance-20261123.md) | Default strict global FIFO still cannot promise arbitrary crossed-stream progress; unbounded fairness and H7 state isolation explicitly excluded |
| H3b lifecycle interleavings | Partial, now scoped verified | Whole original error/execution-boundary and lifecycle map, native composition regressions, reused-worker cleanup and exact-source ownership accepted at `e166656379ac8837c7a51e1e0a3e84752ad42e85` | [Acceptance](fbp-h3b-whole-acceptance-20261124.md); independent H3b evidence, not automatic Q5 credit; existing native/cooperative assumptions unchanged |
| H4 scheduling scans | Partial | Historical staged polling removed with mandatory-set density resynchronization; exact-head #1387 green, local paired evidence recorded; broader addressed-event scaling acceptance remains | [Staged lifecycle evidence](fbp-staged-poll-lifecycle-20261123.md); H6; retain required global/polled Custom fallbacks |
| H5 closure analysis | Partial | Own-lane scoped traversal plus exact witness/oracle and public owner/obligation regressions; corrected #1390 exact-head green; meet broader closure budgets | [Lane closure evidence](fbp-h5-lane-closure-20261123.md); repeated proofs/full global EOS scan remain |
| H6 profiling acceptance | Partial | Representative latency/throughput, allocation/live-memory and concurrency budgets with exact comparable sources | Runtime acceptance is distinct from missing production model/tool integration |
| H7 isolation/fairness | Partial | Specify stalled-continuation/shared-state behavior, resource/byte bounds and progress assumptions; demonstrate supported independent progress | Queue lanes alone do not isolate mutable process state or external effects |
| C5 deferred watchdog | Partial, now scoped verified | User-approved cooperative supervision plus full ticket/deadline/policy acceptance at `89171a843` | No autonomous abandoned-host service, callback preemption or worker/effect reclamation |
| C3 retired-generation policy | Open, now scoped verified | User-approved LeasedArchival, with integrated retained request/clone/job authentication and exact leases | Retirement stops host-owned producers, not explicit archived execution; strict revocation not claimed |
| C6 migration/restart | Open, now scoped verified | User-approved Restart And Persist plus real independent SQLite reopen/CAS/schema/candidate-failure and archived-job proof | Fresh process-private successor state; durable continuity is explicit and no live migration or exclusive writer is claimed |
| C7 plugin isolation | Open, now excluded | User-approved TrustedNativeScope excludes untrusted native isolation | No OS sandbox was implemented or verified; exclusion earns no numerical implementation credit |
| A1 private extension | Open, now scoped verified | Author-approved nested Add/Connect/export, checked bound lowering, prepared Kernel integration and genuine nested agent feedback/stop accepted at `923ccd6376b4cd8e3980b61ae26ad78c08e9c6a9` after independent whole-scope review and exact-head CI | [Acceptance](fbp-a1-checked-nested-exports-20261123.md); extension and replacement remain separate authorities; exact claimed interfaces cannot silently widen |
| A2 compatibility | Open, now scoped verified | Original safe compatibility relation, exact facade/V2 schema replacement and positive/negative real feedback evidence accepted at `1589ad0d2828fecea99f784ac8e64fe952e821bc` after independent whole-scope review and exact-head CI | [Acceptance](fbp-a2-adapted-replacement-20261123.md); exact claims and frozen obligations preserved; optional raw width binding is not an original prerequisite |
| A3 graph/store separation | Open, now scoped verified | Concrete storage/permission/synchronization authority removed from Graph; capability identity, attenuation and cleanup/leases preserved | Exact-head acceptance at `026cfbe05`; no Graph-to-Storage dependency or relocated duplicate Store |
| A4 typed authoring | Open, now scoped verified | Canonical authority audit and paired real manual/key/derived executions accepted at `89171a843` | Necessary validated views/convenience APIs retained; no second authoring model or Engine shim introduced |
| A5 physical paths | Open, now scoped verified | Coherent compiled-generation handles now cover required Graph basic/internal execution, configuration/wake/source/closure, preview/final compilation and binding, Kernel/Generation and the actual StorageGraph query at `45c2dfb6f` | Independent whole-scope review and all local/Nix/exact-head gates passed; necessary validated dynamic paths remain, and H7 older-capture/shared-process limitations stay outside A5 |
| Q1 responsibility concentration | Open, now scoped verified | Original network/composition/runtime roots decomposed into cohesive private ownership modules at `7c6119122` | Canonical state/public paths/drop order and behavior preserved; broader Kernel root concentration is explicitly residual |
| Q2 incremental compilation | Open | Root addition/removal/capacity/edge/batch reuse through #1371 is now assembled with nested-generation reindex oracles; finish general dependency-aware invalidation/scaling | [Integration](fbp-foundation-integration-20261123.md); no cross-Kernel reload cache or general nested lowering credit; preserve fresh authority |
| Q3 selection/scaling | Open | Exact selection and indexed planning plus #1389 hosted chain/diamond/skew/early-conflict workloads; complete total-planning/recomposition scale acceptance remains | Allowance excludes ancestor/footprint planning and replay/compile; broader triggers blocked by workflow permission; no whole-finding closure |
| Q4 static liveness | Open | Define an analyzable subset with sound reject/unknown/progress results and explicit environment assumptions | Arbitrary callbacks and every SCC cannot receive a universal progress claim |
| Q5 concurrency evidence | Open, now scoped verified | Supported drain/retirement/driver, handoff and wake/public identity compositions accepted at `4b765ddfbabf334895147c1f7bf8b5accfb96ee1` with actual-source bounded models and deterministic/public native complements | Original Q5 is scoped verified after independent review and all local/Nix/exact-head gates; H3b remains partial, and bounded schedules are not universal fairness/liveness/deadlock-freedom proof |
| Q6 diagnostic completeness | Open, now scoped verified | Complete bounded passive process/request explanations accepted at `89171a843`, sharing actual selector and closure predicates | Custom/future behavior remains explicitly unknown; current-state explanation is not Q4 liveness |
| Q7 typed failures | Open, now scoped verified | Complete process-to-handoff-to-session/request-to-Kernel/StorageGraph typed protocol accepted at `6f6e751b36f77273da8799900f665bc026447418` | Original causes, cleanup and known per-handle nonretroactive leases are preserved; trusted-native abort/double-panic limits remain |

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
