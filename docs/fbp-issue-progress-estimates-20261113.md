# FBP 33-Issue Progress Estimates

## Current Acceptance: Q5, 2026-11-20

Latest accepted implementation: `4b765ddfbabf334895147c1f7bf8b5accfb96ee1`.
Independent final whole-scope and coverage reviews found no remaining findings.
Exact-source owner atomic/RAII behavior and all public identity/composition
fixtures were accepted. Full local default/no-default workspace tests, isolated
Graph no-default tests, explicit doctests, strict Clippy and formatting passed.
All ten checks on untracked-inclusive snapshot
`/nix/store/v6d1vcjn5fnh5l00r5cnqb84mp00kfwg-source` passed after approved
temporary-target cleanup of the initial disk-exhaustion failure. All fifteen
exact-head hosted workflows are SUCCESS, confirmed through `gh`; [the final Q5
record](fbp-q5-protocol-verification-20261117.md) contains the complete run URL
table, model/test evidence, reviews and limits.

Original Q5 is **scoped verified: 74.0% -> 100.0%, +26.0 pp**. Acceptance covers
bounded exact-source producer/session ownership, real handoff operation histories,
public identity/transport/worker composition, retirement and preserved wake
orders. Ten session/ownership properties complete 472 schedules with no duration
cap; five wake properties and 13,440 serialized wake histories are preserved.
The handoff matrix covers 21 anchors, 1,533 histories and 2,856 attempts (611
accepted / 2,245 refused), all eight outcomes, and public worker composition under
both queue policies, including five fresh checkpoints for a non-emitting old
`Complete` result. The intermediate two-Idle assertion was incomplete test
authoring, not a production defect.

Only Q5 changes relative to the accepted A5 table; all prior Q7/A5/Q1 acceptance
and every other issue score remain intact. Classifications are **21 scoped
verified / 6 partial / 5 open / 1 approved exclusion**, leaving **11 in-scope
findings**. The score sum is **2591**, and the subjective unweighted issue mean
is **78.5% (2591 / 33 = 78.515...%)**, up from 77.7% (2565 / 33); it is not
product completion, a safety probability or milestone acceptance.

H3b remains partial at 78.0%, with no automatic Q5 credit. The bounded
owner/handoff evidence is not a universal schedule proof for broader H1/H3b
lifetime behavior and supplies no fairness, static-liveness, deadlock-freedom or
performance conclusion. C7 remains approved TrustedNativeScope at its historical
12.0%, without sandbox credit. H7 remains partial at 56.0%. A1 stays open and
design-only; M7/M8 remain incomplete. This current status and Q5's table row
supersede earlier aggregate counts and Q5 open/pending assertions. This
evidence-only documentation revision requires its own checks and accepts no
later implementation or evidence-only commit.

## Previous Acceptance: A5, 2026-11-20

A5 accepted implementation: `45c2dfb6f9089fb4b2f25bd102a37d2181e44eaf`.
The independent whole-scope review found no remaining findings after all review
fixes. All local default/no-default Graph/workspace, explicit documentation,
compilation, strict-Clippy and formatting gates passed, including all ten checks on
the untracked-inclusive Nix snapshot. All fifteen exact-head hosted workflows are
SUCCESS, confirmed through `gh`; [the final A5 record](fbp-bound-handles-20261117.md)
contains the complete run URL table, tests, review facts and limits.

Original A5 is **scoped verified: 24.0% -> 100.0%, +76.0 pp**. Acceptance covers
real compiled-generation-owned handles for Graph basic/internal resolution and
execution, configuration, wakes/notifiers, sources and closure; checked composition
bridges with nontransferable preview authority; Kernel/Generation and final binding;
and the actual production StorageGraph query. Stale/foreign rejection and validated
dynamic paths are retained without physical tuples serving as authority. Known-owner
per-handle nonretroactive leases, hostile-drop cleanup and Q7 typed failure behavior
are preserved.

At A5 acceptance, the table changed only A5; all prior Q7 acceptance and other
issue scores remained intact. Classifications were **20 scoped verified /
6 partial / 6 open / 1 approved exclusion**, leaving **12 in-scope findings**.
The subjective unweighted issue mean is **77.7% (2565 / 33 = 77.727...%)**, up from
75.4% (2489 / 33); it is not product completion, a safety probability or milestone
acceptance. C7's historical **12.0%** is unchanged, with no sandbox credit.

H7 remains partial at 56.0%: older-request scoped-capture retention and shared-process
occupancy are still unresolved. A5 completes the original binding-authority finding,
not universal request progress or fairness. Trusted native abort/double-panic
exclusions remain. Green benchmarks are execution evidence, not performance
acceptance; M7/M8 remain incomplete. That A5 status and table row superseded
earlier status/count/remaining-work assertions below; historical evidence
and narrative are preserved. This evidence-only docs revision requires its own
checks; implementation-head passes do not accept a later revision.

## Historical Acceptance Sequence

Comparison baseline: verified PR #1300 at `516e0883875d5ec483e3595d50b77b3d5e8979c4`;
Flake CI [43537983510](https://github.com/matthis-k/phenix-ai/actions/runs/43537983510),
native Q5 [43537982855](https://github.com/matthis-k/phenix-ai/actions/runs/43537982855),
and Loom [43537982741](https://github.com/matthis-k/phenix-ai/actions/runs/43537982741)
passed on that head. Scores are subjective engineering issue-completion estimates,
not safety probabilities or an overall project-completion metric.

The campaign initially relied incorrectly on stale local remote-tracking
`origin/impl/graph-consolidation-20261110` at `16e77f6c6`. Its first commit,
`0d145f3`, is on `fix/fbp-lifecycle-request-lanes-20261114` (PR #1302, based on
the impl graph branch). The actual remote impl head is `89e00d0`, with later
foundations. The campaign subsequently merged exact verified PR #1300
head `516e0883875d5ec483e3595d50b77b3d5e8979c4` to preserve the complete stack
and all scoped gates. Earlier claims that the later stack was absent describe the
initial candidate, not the reconciled merge target.

The correctness-verified score comparison candidate, before A6 closure, is
`90591a1202e4a1e803c4258e50a1b7d80ec97d6c`, with
[six-job Flake CI 43674358770](https://github.com/matthis-k/phenix-ai/actions/runs/43674358770)
and both Loom/native concurrency gates passing. Its issue scores remained identical
to verified `516e088`; configuration-dependent performance counterevidence remains
open. The table compares with those unchanged scores. Implementation head
`08ab8fd23679522544c02ecedb431a3d0e1a992d` now passes all fifteen workflows,
including [six-job Flake CI 43696623155][A], and closes the bounded original A6
admission finding after independent review. Q6/Q7 subproblem fixes do not receive
whole-issue closure or score increases. This is the authoritative 33-issue scorecard;
the [historical issue ledger](fbp-issue-ledger-20261111.md) and campaign record
provide detail, not a second current score authority.

Subsequent implementation `51878d147e194a9f7aebf2020c2c550a10e50f9f` passed all
fifteen workflows, including [six-job Flake CI 43702741898][E]. Its additional
resolver-cause and invalid-emission diagnostic fixes leave all scores unchanged.
The [design, review and evidence](fbp-error-context-20261115.md) also records a
separately reproduced Q3 selection defect, unfixed at that revision and subsequently
corrected at `006b2e9cc` as recorded below.

Earlier implementation `979572eb9e08462adddac07d35625bd7f2cb940e` passed all
fifteen workflows, including [six-job Flake CI 43705906267][K]. It fixes public
tuple snapshot cardinality, query declaration-cause loss and continuation wait-lane
attribution; [design/review/evidence](fbp-quick-boundaries-20261115.md) records their
bounded acceptance and the separate, then-unresolved request-terminal precedence gap.
Scores and classifications below are unchanged by this batch; C1 verification is
the inherited session-level scope, not a claim about every request transition.

Earlier implementation `026cfbe05e9b078f891233a2fc65bcada3fbfea0` passed all fifteen
workflows, including [six-job Flake CI 43727271982][N]. Independent original-scope
and assembled integration reviews accept [A3 resource separation](fbp-resource-boundary-20261115.md),
[native WIP #1258](fbp-published-host-wake-20261115.md), and the user-approved
[terminal/cleanup correction](fbp-terminal-cleanup-20261115.md). A3 is scoped verified;
#1258 is closed as superseded, not merged. The [work map](fbp-remaining-work-20261115.md)
retains concrete acceptance/dependency requirements for all 22 starting findings,
of which 21 remained at that stage. The later approvals and acceptance below
supersede that stage's pending C3/C6/C7 policy labels.

Earlier implementation `006b2e9ccda6b06739571e5bef3b6d94a74fb220` passed all fifteen
workflows, including [six-job Flake CI 43770741835][S]. Its [selection correction](fbp-selection-correctness-20261115.md)
fixes the reproduced dependency-defeat bug, requires global uniqueness and exposes
finite host-controlled work limits. Independent truth-table/model tests, public
allowance/scaling tests and both Kernel composer paths are verified. Q3's broader
planning/recomposition scaling acceptance remains open; no new score or closure
credit follows from the bounded correctness slice.

Earlier verified implementation `89171a8437c6fc2aebcd9029eb119d8c7505d389` passed
all fifteen hosted workflows, including [six-job Flake CI 43804652273][L]. Independent
original-scope reviews accept [A4 canonical authoring equivalence](fbp-authoring-acceptance-20261116.md),
[Q6 bounded passive explanations](fbp-explanations-20261116.md), and the explicitly
approved [C3 archival/C5 cooperative](fbp-lifecycle-acceptance-20261116.md) and
[C6 restart-and-persist](fbp-restart-persistence-20261116.md) contracts. These five
findings are scoped verified. The user separately approved C7 TrustedNativeScope:
untrusted isolation is **excluded**, not implemented or sandbox-verified. Its old
12.0% estimate is unchanged and earns no numerical implementation credit.

Latest verified implementation `7c61191222a19cefccde9c38adedbc9ad290bf01` passed
all fifteen workflows, including [six-job Flake CI 43811659284][O]. Independent
structural review accepts [Q1's original three-root ownership decomposition](fbp-owned-boundaries-20261116.md):
network authoring/validation/compiler/resolution, composition payload/planning/replay,
and runtime admission/scheduling/execution/observation/recovery now have cohesive
private boundaries with unchanged canonical owners, public paths and behavior.
The broader Kernel root concentration remains explicitly outside this original scope.

The unweighted issue mean is **73.2%**, previously 70.8%. Formal classifications are
**18 scoped verified / 6 partial / 8 open / 1 excluded**. The historical 33-row mean
continues to include C7's unchanged 12.0% for comparability, not sandbox credit.
There are **14 remaining in-scope findings**. This mean is not a substitute for issue
closure or milestone completion.

## Verified Baseline Evidence

The final verified PR #1300 baseline records Q5 at 74.0% and H3b at 78.0%.
Its evidence includes a bounded Loom verifier over the actual `wake.rs`
synchronization methods, all 13,440 legal serialized wake-operation orders, real
notification/retirement races, in-flight delegated-worker cancellation and
replacement, and plugin-mutex inversion challenges for custom readiness,
dispatchers, and request-recovery factories. The Q5 workflow repeats its native
suite eight times. Their recorded passes apply to the verified head, not the new
merge. Candidate cases exercise additional kernel session/producer boundaries.

The baseline did not claim arbitrary plugin-vs-plugin or host-lock deadlock
freedom and remains historical evidence only. Current scoped Q5 acceptance still
supplies no unbounded real-interleaving proof; H3b remains partial.

## All 33 Issues

| Issue | Scope | Previous | Current | Δ pp | Status | Evidence |
| --- | --- | ---: | ---: | ---: | --- | --- |
| H1 | Shared-session FIFO correlation | 60.0% | 60.0% | +0.0 | Partial | [Baseline][B]; candidate `fbp_request_lanes` |
| H2 | Callback panic containment | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; candidate `worker_guards` |
| H3a | Inventory lock inversion | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B] |
| H3b | Session/drain and worker interleavings | 78.0% | 78.0% | +0.0 | Partial | [Baseline][B]; candidate `native_sessions`, `worker_guards` |
| H4 | Scheduling scans | 76.0% | 76.0% | +0.0 | Partial | [Baseline][B]; dirty scheduler retained; paired workload assessment |
| H5 | Closure analysis | 71.0% | 71.0% | +0.0 | Partial | [Profiled closure correction](fbp-closure-overhead-20261114.md); same-head timing required |
| H6 | Performance profiling | 70.0% | 70.0% | +0.0 | Partial | Exact paired baseline and regression evidence retained; no production acceptance |
| H7 | Request isolation and fairness | 56.0% | 56.0% | +0.0 | Partial | [Baseline][B]; candidate `fbp_request_lanes`, bounded RR |
| C1 | Terminal transition precedence | 100.0% | 100.0% | +0.0 | Scoped verified | Session-level precedence retained; [approved request disposal preserves inherited outcomes and original causes](fbp-terminal-cleanup-20261115.md), [exact-head][N] |
| C2 | Obligation deduplication | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; candidate closure-only fairness test |
| C3 | Retired-generation policy | 20.0% | 100.0% | +80.0 | Scoped verified | [User-approved LeasedArchival](fbp-lifecycle-acceptance-20261116.md), integrated old request/clone/job authority and exact leases; [exact-head][L]; not strict revocation |
| C4 | Preparation authority | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; candidate binding compile-fail tests |
| C5 | Deferred-job watchdog | 54.0% | 100.0% | +46.0 | Scoped verified | [User-approved cooperative supervision](fbp-lifecycle-acceptance-20261116.md), full ticket/policy/deadline matrix and native driver wake; [exact-head][L]; no abandoned-host service or preemption |
| C6 | State migration/restart policy | 12.0% | 100.0% | +88.0 | Scoped verified | [User-approved Restart And Persist](fbp-restart-persistence-20261116.md), real SQLite reopen/CAS/schema/candidate failure and archival jobs; [exact-head][L]; no live migration |
| C7 | Plugin isolation boundary | 12.0% | 12.0% | +0.0 | Excluded by approved scope | [User-approved TrustedNativeScope](fbp-lifecycle-acceptance-20261116.md#c7-trusted-native-boundary); untrusted isolation remains unimplemented, not verified-sandbox credit |
| C8 | Code-lease retention | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; candidate admission-error/timeout lease regressions |
| C9 | Passive session state | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; passive `state(&self)` retained |
| C10 | Redundant execution API | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; API removal retained by merge |
| A1 | Private composite extension | 18.0% | 18.0% | +0.0 | Open | [Baseline][B]; general extension API missing |
| A2 | Interface compatibility | 16.0% | 16.0% | +0.0 | Open | [Baseline][B]; existing contract/replacement tests |
| A3 | Graph/store separation | 20.0% | 100.0% | +80.0 | Scoped verified | [Opaque external capability boundary](fbp-resource-boundary-20261115.md); Graph Store, locks and read/write policy removed; independent review and [exact-head][N] |
| A4 | Typed authoring consolidation | 30.0% | 100.0% | +70.0 | Scoped verified | [Canonical authority audit and paired executable manual/key/derived equivalence](fbp-authoring-acceptance-20261116.md), real isolated no-default and doctest gates; [exact-head][L] |
| A5 | Typed vs. string paths | 24.0% | 100.0% | +76.0 | Scoped verified | [Complete compiled-generation binding authority across Graph, composition/final binding, Kernel/Generation and the real StorageGraph query; independent whole-scope review, local/Nix gates and all fifteen exact-head workflows](fbp-bound-handles-20261117.md) at `45c2dfb6f9089fb4b2f25bd102a37d2181e44eaf`; validated dynamic paths and the separate H7 residual remain |
| A6 | Admission API ergonomics | 30.0% | 100.0% | +70.0 | Scoped verified | [Typed key admission design/review](fbp-typed-admission-20261115.md); [exact-head acceptance][A]; keys are names, not generation authority |
| A7 | Identifier bounds | 100.0% | 100.0% | +0.0 | Scoped verified | Existing bounds retained; double-`id()` validation gap fixed with stateful-ID regression; [exact-head][A] |
| Q1 | Responsibility concentration | 22.0% | 100.0% | +78.0 | Scoped verified | [Original three graph roots decomposed into cohesive private ownership seams](fbp-owned-boundaries-20261116.md), canonical state/drop order/public paths preserved; independent review and [exact-head][O]; broader Kernel concentration remains |
| Q2 | Incremental compilation | 10.0% | 10.0% | +0.0 | Open | [Baseline][B]; not implemented |
| Q3 | Dependency scaling | 14.0% | 14.0% | +0.0 | Open | [Exact unique selection and global host work limits](fbp-selection-correctness-20261115.md), [exact-head][S]; defect fixed, broader ancestor/footprint planning and recomposition scaling remain |
| Q4 | Static liveness guarantees | 10.0% | 10.0% | +0.0 | Open | [Baseline][B]; explicit blocking limits only |
| Q5 | Concurrency verification | 74.0% | 100.0% | +26.0 | Scoped verified | [Bounded exact-source ownership and real handoff/public identity composition, independent whole-scope/coverage review, local/Nix snapshot gates and all fifteen exact-head workflows](fbp-q5-protocol-verification-20261117.md) at `4b765ddfbabf334895147c1f7bf8b5accfb96ee1`; H3b remains partial and no universal schedule, fairness, liveness or performance proof follows |
| Q6 | Diagnostic quality | 22.0% | 100.0% | +78.0 | Scoped verified | [Complete bounded current-state explanations](fbp-explanations-20261116.md), shared selector/closure authority, jobs/wakes/publication/request-live accounting and adversarial passivity; [exact-head][L]; not Q4 liveness |
| Q7 | Typed error consistency | 26.0% | 100.0% | +74.0 | Scoped verified | [Full native typed protocol, framework reasons and plugin lifetime errors; independent whole-slice reviews, local/Nix and all fifteen exact-head workflows](fbp-q7-final-review-20261117.md) at `6f6e751b36f77273da8799900f665bc026447418`; per-handle nonretroactive leases and trusted native limits remain |
| Q8 | Report reconciliation | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; historical report labels corrected |

[B]: https://github.com/matthis-k/phenix-ai/actions/runs/43537983510
[C]: fbp-hardening-campaign-20261114.md
[A]: https://github.com/matthis-k/phenix-ai/actions/runs/43696623155
[E]: https://github.com/matthis-k/phenix-ai/actions/runs/43702741898
[K]: https://github.com/matthis-k/phenix-ai/actions/runs/43705906267
[N]: https://github.com/matthis-k/phenix-ai/actions/runs/43727271982
[S]: https://github.com/matthis-k/phenix-ai/actions/runs/43770741835
[L]: https://github.com/matthis-k/phenix-ai/actions/runs/43804652273
[O]: https://github.com/matthis-k/phenix-ai/actions/runs/43811659284

## Concrete Work And WIP Disposition

Percentages above describe whole findings, not whether any implementation changed.
The following actual defects/WIP requirements are separately implemented. Latest
implementation acceptance is exact-head `4b765ddfbabf334895147c1f7bf8b5accfb96ee1`,
not an older commit's passing run. This evidence-only documentation revision must
pass its own head's checks.

| Scope | Original Failure | Implemented Resolution | Evidence / Remaining Gate |
| --- | --- | --- | --- |
| H5 measured regression | Optional hot loop outlined after Custom support; built-ins looked up unused lane metadata | Common borrowed-name observed-input loop; lane lookup only before effective Custom callback | `fbp_readiness_lanes`, closure/optional suites, independent review, instruction profiling; exact paired native timing |
| WIP #1257 plain/revision reads | Every scoped Read implicitly required CAS and returned tokens | Plain `Kv::get`, explicit `get_revisioned`, independent revision predicate | Populated plain-only/revision-only tests and native observer migration |
| WIP #1257 snapshots | Missing planned batch capability; cell binding checked weaker consistency | Explicit request capability set; conditional `CellSpec::request`; eager atomic binding | Populated batch-only/no-CAS/no-single-revision snapshots; zero-payload preflight tests |
| WIP #1257 authority | Distinct support promises conflated; declarations could not express actual needs | Validate lower-bound requirements independently per plugin; capabilities never grant operations | CAS-only Write, invalid combinations, missing advertisements, false actual support, same-identity/different-grants tests |
| Q7 source provenance | Scoped/plugin error envelopes hid existing typed causes | `Error::source` forwards nested storage/scoped/filesystem causes | `phenix-storage-plugin/tests/error_sources.rs` downcast and error-kind checks |
| H4 admission work | Every request scanned all processes just to find Custom declarations | Construction-time Custom ID index includes polled/notified policies; callbacks still evaluated per scope | `fbp_custom_closure_admission`; no claimed wall-time speedup |
| WIP #1259 ownership/atomicity | Composer/offer/import records independently authored and partially installed on caught failure | One public composer builder and consuming atomic register; metadata keys derived; private resolver payloads | Registrar atomicity, reserved-kind, multi-offer, self-import, lease and three-plugin nested extension proofs; hosted acceptance at `90591a12`, retained at [current head][A] |
| Newly found identity defect | `Registrar::add` validated one `id()` value but stored a second callback result | Capture and validate one stored ID; bundle preflight invokes no payload callbacks | Stateful-ID and invalid stored-name regressions; all original bounds remain |
| A6 typed payload admission | Typed authoring keys were disconnected from injection, allowing wrong Rust payloads until runtime | Mode-aware existing input keys feed canonical session/request/Kernel/Generation and producer admission | Move-only/wire payloads, compile-fail direction/type, actual host, request, retry, codec ordering and lease regressions; [exact-head][A] |
| Q6 request wait attribution | Session-wide blockers could not isolate a request's pending publication | Passive authenticated `request_blocked_queues`, attributed to pending producers | Both policies, internal/capture relief, cross-request buffered packets, invalid IDs and passivity; [exact-head][A] |
| Q7 graph source provenance | Graph envelopes hid nested typed causes; derived conversion stringified NetworkError | Forward real sources and retain typed NetworkError in derived errors | Real graph/StorageGraph failure-path downcasts; [exact-head][A]; broader typed runtime protocol remains open |
| Q7 resolver provenance | Explicit incompatible import selection stringified its InterfaceError | Typed IneligibleReason retains the actual cause; policy denial remains sourceless | Context, direct cause identity, denial/allowlist forms, diagnostic precedence and zero preparation; [exact-head][E] |
| Q6 invalid emissions | Unknown output and incompatible identity shared an opaque process-index message | Compiled path/output context and expected/actual identities before routing | No-consumer/discard, malformed mismatched-schema bytes and request-local recovery under both policies; [exact-head][E] |
| Snapshot tuple cardinality | Public tuple decoding panicked on short input and ignored extras | Exact macro-counted arity check before any decoder; typed SnapshotEntryMismatch | Arity one through four, zero custom callbacks and real-cell source identity; [exact-head][K] |
| Q7 query binding | Missing/ambiguous tap or root lookup was flattened to WrongGeneration | Preserve actual NetworkError in runtime and preparation binding; remove unused variant | Typed source identity, all four lookup failure paths, read-free valid binding and no activation on failure; [exact-head][K] |
| Q6 continuation input waits | Owned continuations reported canonical and unrelated request-lane queues | Filter bounded waits by actual owner, retaining idle/shared behavior | Scoped/unscoped WaitInput/WaitInputs, queue IDs/endpoints, unrelated traffic, lane reuse and passivity; [exact-head][K] |
| A3 external resource authority | Graph owned a concrete Store, locks and read/write permission interpretation | Opaque exact-type capabilities and validated sets; operations/authority stay external | Attenuation compile tests, identity/default deny, sources/continuations, duplicate rejection and independent capability leases/hostile cleanup; [exact-head][N] |
| Request terminal/cleanup | Late explicit cleanup rewrote inherited outcomes; cleanup errors could mask original query failure and inventory | Approved Dispose, Keep Failed, original-first typed Cleanup plus outer exact-job inventory | Both queue/failure policies, scoped capture retention versus explicit disposal, destructor/recovery failure and late jobs; [exact-head][N] |
| WIP #1258 native hosting | Published host lacked source/publication wake registration and sleep tokens | Existing wake handle gains versioned hints; private candidate registration and post-publication signaling; guarded DerivedHost forwarding | Actual sleeping/reload/source/shutdown reactor, partial sweeps/backpressure and real observer reload with offloaded guards; [exact-head][N] |
| Q3 selection correctness | Transient prerequisite defeat permanently suppressed an eligible dependent | Sound propagation and exact globally budgeted feedback search; unique selection required before replay/publication | Original/cyclic/zero/multiple/global-model cases, 19,683 signed systems, exact budgets/atomicity, bounded scale families and host previews; [exact-head][S]; whole planning scale remains open |
| A4 whole original acceptance | Overlapping authoring layers lacked a complete equivalence/authority acceptance | Audit proves one canonical interface/installation path; independent manual/key/derive primitive/composite executions | Explicit boundaries/capacities/FIFO/wire vectors, move-only nested error/unwind leases and authority/replacement negatives; [exact-head][L] |
| C3/C5/C6 approved contracts | Revocation, supervision lifetime and restart/continuity policies were unresolved | Explicit user approval plus integrated archival, cooperative deadline and SQLite restart-continuity proofs | Whole approved contract reviews, actual job/lease/schema/CAS/reopen controls; [exact-head][L]; no revocation, autonomous service or live migration claims |
| Q6 whole original acceptance | Isolated diagnostics did not explain all runtime-owned run/settlement conditions | Shared callback-free selection/closure evidence and passive process/request explanation records | All built-ins, opaque Custom, tickets/deadlines/wakes, target precedence, every request-live term and no-callback/drop passivity; [exact-head][L] |
| Q1 original responsibility scope | Network, composition and runtime roots mixed distinct tested responsibilities | Cohesive private compiler/authoring/resolution, planning/replay and runtime-control seams; same canonical owners and public type paths | Byte-preserved behavior blocks, independent structural review, full workspace/isolated-feature gates and child-only CI coverage; [exact-head][O] |
| A5 whole original acceptance | Physical string paths escaped typed handles as compilation/binding/execution correctness proof | Owned compiled-generation declarations/endpoints/ingress/captures/sources authenticate exact projections and owners before canonical Graph execution; checked preview/final binding, Kernel/Generation and production query migration | Real Graph basic/internal/configuration/wake/source/closure coverage, preview nontransfer, hostile lease/drop cleanup, actual query and independent whole-scope review; full local/Nix gates and [all fifteen exact-head runs](fbp-bound-handles-20261117.md#exact-head-hosted-acceptance) at `45c2dfb6f`; H7 shared-process/older-capture residual remains outside A5 |
| Q5 whole original acceptance | Concurrency evidence omitted systematic producer ownership and composed handoff/worker identity schedules | Exact-source owner atomic/RAII Loom, real owned handoff histories and public session/transport/worker identity matrices, retaining wake and retirement contracts | Ten properties/472 schedules with no duration cap, preserved five wake properties/13,440 histories, 21 anchors/1,533 histories/2,856 attempts, all eight outcomes and both-policy public five-checkpoint composition; independent review and [all fifteen exact-head runs](fbp-q5-protocol-verification-20261117.md) at `4b765ddfb`; bounded scope only, H3b remains partial |

The storage WIP is adapted onto the native FBP branch, not merged from its old
Engine-era tree. It preserves filesystem/mount reservations, schema/codec ordering,
snapshots, persistence, subscriptions and provider leases. Independent review
found no blocking defect; full workspace/default/no-default and strict local gates
pass. WIP #1257 passed hosted acceptance at `f608a3fc` and is closed as superseded
by its native adaptation in #1302, not merged to main. WIP #1259's bounded native
implementation/migration passed at `90591a12` and it is also closed as superseded,
not merged to main. Neither WIP alone closes all of A1/A3/A4/A5/A6/Q7 or M7/M8.
The independently accepted typed-admission batch closes A6; the later opaque
capability boundary closes A3. WIP #1258's bounded native adaptation passed at
`026cfbe05` and is also closed as superseded, not merged. Broad production-agent
integration remain independent follow-up work. After the five approved bounded
acceptances above, Q1 decomposition and the explicitly excluded C7 requirement, fourteen in-scope
findings remain. Exclusion is not a sandbox implementation or verified closure.

## Historical Candidate Evidence

The observations below describe the earlier candidate/merge sequence. Subsequent
exact-head correctness acceptance at `a2d7b4d3` is recorded above; neither that
acceptance nor the new local checks certify a later untested head.

- Kernel producer callbacks run outside the producer-state mutex. The verified
  whole-drain retirement gate and exclusive blocking-driver ownership are retained;
  callback panic is terminal for poll and a typed driver error for wake
  registration. The initial empty-return/no-wait policy for competing drains is
  discarded: manual drains serialize, recursive drains reject, and a live blocking
  driver excludes manual polling even for `drain(0)`. Deterministic tests verify lock
  availability, bounded re-entry, panic cleanup, and callback retirement races.
- Session operations now move exclusive session ownership out of the mutex while
  executing callbacks. All Kernel session calls fail fast on cross-thread
  contention, preserving the verified busy/`WouldBlock` contract; same-kernel
  callback/worker re-entry also fails fast. RAII returns ownership on normal and
  panic paths, and a caller can retry after release. `ExclusiveSlot` uses only
  `Mutex<Option<T>>`, `try_take` and idempotent release, with no Condvar or waiters.
  Initial waiting claims are historical and superseded, not an accepted Kernel
  lock cycle. The original lock-inversion regression requires no producer marker.
- The revised three-property session Loom verifier covers exclusive ownership
  under competing `try_take` calls, a busy caller unable to steal ownership plus
  successful retry after release, and idempotent early release publishing mutated
  state. Initial-candidate Loom passes do not accept these revised properties on
  the new merged head.
- Factory readiness/correlation declaration panics fail session initialization.
  Request `closure_ports` is cached per admitted lane; a panic rejects request
  admission before a partial scope is installed. Correlation wrapping preserves
  custom closure obligations.
- The fixed-lane correlation candidate has a bounded fairness regression: with
  one request missing a join input, an independent complete request settles under
  repeated one-step budgets, preserves request IDs/results, then the earlier
  request completes without duplicate processing. Its declared distinct-scope
  skew must fit both live lanes and ingress credit. This is local evidence only;
  H1/H7 scores remain unchanged pending exact-head acceptance and broader workload
  coverage.
- Local default workspace, no-default-feature, Clippy, Loom and Nix checks passed
  for the initial `0d145f3` candidate sources. PR #1302 remote workflows initially
  conflicted; manual [Flake #43588157944](https://github.com/matthis-k/phenix-ai/actions/runs/43588157944)
  targets `0d145f3`, not the final merged head. Full acceptance for the new merge
  remains pending. Earlier local checks are not relabeled as merge acceptance.
- A historical same-harness optimized microbenchmark on clean `16e77f6c6` vs. the
  inherited candidate
  measured medians of 639/757 ns for no-op `with_session`, 34/100 ns for empty
  `run_steps`, and 23/39 ns for empty `drain` (baseline/candidate). This is a
  synthetic empty-kernel comparison, not end-to-end throughput or allocation
  evidence or a bound on the new merge's overhead. The largest observed median
  increase was 118 ns/call; H6 remains partial and unchanged.

Q5 and H3b remain at their verified baseline 74.0% and 78.0%. No arbitrary
plugin-vs-plugin or host-lock deadlock-freedom claim is made.

## Campaign Evidence And Remaining Gaps

All table deltas compare with the immediately preceding verified candidate above.
All unchanged entries inherit that baseline's evidence, not new completion credit.
Candidate changes are tracked in [the campaign design](fbp-hardening-campaign-20261114.md).

| Issues | Candidate implementation/evidence | Closure gap |
| --- | --- | --- |
| H3b, Q5 | Original Q5 bounded owner/handoff/public identity scope accepted at `4b765ddfbabf334895147c1f7bf8b5accfb96ee1`, with exact-source models and retained native/wake evidence | Q5 scoped verified; H3b remains 78.0% partial because broader lifetime schedules, arbitrary plugin-created thread/lock cycles and universal deadlock freedom are outside this acceptance |
| C5 | Approved cooperative watchdog contract and complete ticket/deadline/policy evidence at `89171a843` | Scoped verified; no abandoned-host autonomous service, callback preemption or worker/effect reclamation |
| C6 | Approved Restart And Persist, independent SQLite reopen/schema/CAS and archived jobs at `89171a843` | Scoped verified; no live process-state migration or exclusive successor writer |
| C3 | Approved LeasedArchival with integrated retained generation/session/request/job authentication and leases | Scoped verified; producer retirement does not revoke explicit retained capabilities |
| C7 | User-approved TrustedNativeScope | Excluded, not sandbox-verified; an untrusted OS adapter remains unimplemented |
| H1, H7 | Opt-in bounded FIFO lanes across ingress, internal edges, captures; round-robin runnable lane selection; EOS, cancellation/reuse, retained-capture, mixed-traffic regressions | Queue isolation is not independent process state or progress through a nonconsuming reader; no per-payload byte budget or universal fairness claim |
| H4, H5, H6 | Verified optimization stack retained; exact paired timing/RSS exposed regressions, leading to a profiled correction | Same-head comparison must verify correction; production-representative acceptance remains outstanding |
| A6 | Existing typed input keys now feed actual session/request/Kernel/Generation and producer admission; independent review and `08ab8fd23` exact-head acceptance | Bounded finding closed; runtime validation still applies and generation authority belongs to A5 |
| A4 | Canonical authority/representation audit and paired manual/key/derived compiled executions, both feature configurations | Original bounded finding scoped verified at `89171a843`; no redundant authoring model found or invented replacement API |
| A5 | Complete compiled-generation-owned binding authority through Graph execution, preview/final composition, Kernel/Generation and actual StorageGraph query, with independent whole-scope review and exact-head acceptance at `45c2dfb6f` | Original bounded finding scoped verified; validated dynamic lookup/diagnostics remain, and H7 scoped-capture/shared-process limitations are not universal-progress closure |
| A1, A2, Q2, Q3 | Existing typed authoring/composer regression suites retained | General private extension/compatibility, incremental compilation and whole dependency scaling remain |
| A3 | Graph Store/locking/permission authority removed in favor of explicit opaque capabilities, with independent lease-safe cleanup | Original bounded finding closed at `026cfbe05`; external authority honesty/revocation and native sandboxing are separate scopes |
| Q6 | Shared callback-free selection/closure evidence and complete bounded passive process/request explanations | Original bounded finding scoped verified at `89171a843`; not static liveness, future Custom selection or globally atomic wake state |
| Q1 | Original network/composition/runtime responsibility concentrations decomposed at `7c6119122` with one canonical owner per state domain | Original bounded scope closed; broader Kernel root concentration is disclosed, not claimed eliminated |
| Q7 | Complete typed process/runtime/session/Kernel/StorageGraph migration accepted at `6f6e751b36f77273da8799900f665bc026447418` | Scoped verified; known-owner nonretroactive leases and trusted-native abort/double-panic limits remain |
| Q4 | Intentional blocking and finite FIFO limitations documented/tested | Static liveness analysis and sound progress diagnostics remain |
| H2, H3a, C1, C2, C4, C8, C9, C10, A7, Q8 | Baseline scoped guarantees retained and exercised at `a2d7b4d3`; newer changes require their own same-head gates | No permission to weaken existing guarantees or substitute earlier CI for current acceptance |

M0-M6 retain substantial native implementation, but this candidate does not close
M7. M8 P1/P2 production-agent integration is still outstanding; no executable model,
credential/provider, durable conversation, local/SSH tool or frontend adapter is
claimed by the queue and lifecycle work.

## Historical Verified Stack Detail

The incoming stack's previous comparison was PR #1300 at
`1a57f7a6ebdf512200244e1b14cb4a979ce99128`; Flake CI
[43437130509](https://github.com/matthis-k/phenix-ai/actions/runs/43437130509),
native Q5 [43437129633](https://github.com/matthis-k/phenix-ai/actions/runs/43437129633),
and Loom [43437129586](https://github.com/matthis-k/phenix-ai/actions/runs/43437129586)
passed. This is historical provenance, not the current table's comparison head.
The [incoming historical tracker](https://github.com/matthis-k/phenix-ai/blob/516e0883875d5ec483e3595d50b77b3d5e8979c4/docs/fbp-issue-progress-estimates-20261113.md)
records the earlier H3b 76.0% to 78.0% and Q5 73.0% to 74.0% increases, with
the mean moving from 55.2% to 55.3%. Those historical deltas are not repeated in
the authoritative current table: A6, A3, C3, C5, C6, A4, Q6 and Q1 have new accepted
scoped closure credit; the other 25 deltas, including excluded C7, remain +0.0 pp.
The accepted stack evidence includes the actual `wake.rs` Loom verifier, all
13,440 serialized wake-operation orders,
real notification/retirement races, delegated-worker cancellation/replacement,
and plugin-lock inversion challenges for custom readiness, dispatch, and request
recovery. Its native Q5 suite repeats eight times. The subsequent verified
callback-lock/cleanup batch established the H3b 78.0% / Q5 74.0% baseline used
above; those are not new campaign increases or closures.

Historical callback-lock/cleanup evidence:

- The session mutex guards ownership transfer, not runtime/plugin callbacks.
  Request/session teardown detaches process state before user destructors and
  retires old wakes before replacement. The verified batch used cross-thread
  fail-fast access; the reconciled candidate preserves that contract for all Kernel
  session callers rather than waiting behind a callback that may need their locks.
  This does not prove arbitrary plugin/resource lock-order freedom.
- A deterministic cross-thread process callback/plugin-mutex challenge and
  panic-safe teardown interleavings supplemented the five actual-`wake.rs` Loom
  properties. These are scoped historical results, not a proof of the new session
  lease protocol; candidate `ExclusiveSlot` has separate bounded Loom coverage.
- `Subscription::drop` takes the stored Waker while the Watch mutex is held, then
  sends its destruction through the existing deferred/panic-isolated Waker path.
  A custom destructor regression verifies success, transaction error, and unwind
  all defer destruction until the Watch lock and storage reentrancy guard are clear.
- Request queue cancellation now detaches packets and updates request counts/FIFO
  before payload destruction. `ProcessHandoff` invalidates tickets and detaches
  process/context/code lease state; request/session transitions retire all wakes
  and process ownership before disposing that state. Destructors are isolated one
  at a time, and public cancellation reports typed cleanup failures after cleanup.
- Historical kernel session access leased `NetworkSession` out of its mutex around
  dispatcher/readiness/process callbacks. Same-thread recursion failed fast; that
  batch also rejected cross-thread contention with busy/`WouldBlock`. That contract
  is preserved by the current candidate; its initial waiter policy is superseded.
- Adversarial tests cover panicking payloads in a queued packet and a reserved
  process context, stale wake rejection, fresh-request progress, and direct handoff
  cancellation reporting. The five-property existing Loom verifier passes; no
  Loom model was added in that batch because it used mutex-owned detachment and left
  production wake synchronization unchanged.
- Exact implementation commit `2a8091dd2067bb14cda93b91cc99e9347e5eda37` passed
  six-job [Flake CI #43530164488](https://github.com/matthis-k/phenix-ai/actions/runs/43530164488),
  [five-property Loom #43530657205](https://github.com/matthis-k/phenix-ai/actions/runs/43530657205),
  [8-repeat notifier-retirement Q5 #43530657598](https://github.com/matthis-k/phenix-ai/actions/runs/43530657598),
  and [12-repeat in-flight recovery #43530657552](https://github.com/matthis-k/phenix-ai/actions/runs/43530657552).
  H4/H5/H6 checks passed on the same head: H4 runs #43530163657, #43530163860,
  #43530163909, #43530163762; H5 runs #43530163697 and #43530163719; H6 runs
  #43530163988 and native runtime benchmark #43530163694. Strict workspace Clippy,
  format, and focused storage/graph/kernel tests passed locally.

Historically, the same optimized, setup-excluded kernel harness on PR #1300
baseline `1a57f7a6` vs. the session-lease candidate measured medians of 678/757 ns for no-op
`with_session` and 107/188 ns for empty `run_steps(0)` (baseline/candidate); three
sequential candidate samples varied by roughly 100 ns. Added absolute cost was
about 79-81 ns/call. This is a microbenchmark, not process throughput or allocation
evidence. H6 stays 70.0% because no profiling/allocation conclusion changes. The
historical session-lease evidence is scoped-verified but does not close H3b or
accept the new merge. Remaining risks include arbitrary plugin-vs-plugin and
cross-mount lock cycles, panics from multiple
nested destructors during one destructor's own unwinding, and unrevocable effects
already started by delegated work.
