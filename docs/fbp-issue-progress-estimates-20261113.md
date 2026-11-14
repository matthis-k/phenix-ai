# FBP 33-Issue Progress Estimates

Comparison baseline: verified PR #1300 at `516e0883875d5ec483e3595d50b77b3d5e8979c4`;
Flake CI [43537983510](https://github.com/matthis-k/phenix-ai/actions/runs/43537983510),
native Q5 [43537982855](https://github.com/matthis-k/phenix-ai/actions/runs/43537982855),
and Loom [43537982741](https://github.com/matthis-k/phenix-ai/actions/runs/43537982741)
passed on that head. Scores are subjective engineering issue-completion estimates,
not safety probabilities or an overall project-completion metric.

This campaign started on `impl/graph-consolidation-20261110`, commit `16e77f6c6`,
rather than PR #1300's branch, with existing uncommitted callback/correlation work.
PR #1300's stacked implementation is not silently transplanted or superseded by
this separate candidate. **No issue score increase or closure is accepted** here:
local candidate evidence and baseline verification must not be conflated. This is
the authoritative 33-issue scorecard; the campaign record contains designs and gaps,
not a second score authority.

The unweighted issue mean remains **55.3%**. Formal classifications remain **10
scoped verified / 7 partial / 16 open**. This mean is not a substitute for issue
closure or milestone completion.

## Verified Baseline Evidence

The final verified PR #1300 baseline records Q5 at 74.0% and H3b at 78.0%.
Its evidence includes a bounded Loom verifier over the actual `wake.rs`
synchronization methods, all 13,440 legal serialized wake-operation orders, real
notification/retirement races, in-flight delegated-worker cancellation and
replacement, and plugin-mutex inversion challenges for custom readiness,
dispatchers, and request-recovery factories. The Q5 workflow repeats its native
suite eight times. These tests are not repeated here; the new local cases exercise
additional kernel session/producer ownership boundaries.

The baseline does not claim arbitrary plugin-vs-plugin or host-lock deadlock
freedom. Q5 remains open for unbounded real interleavings, and H3b remains partial.

## All 33 Issues

| Issue | Scope | Previous | Current | Δ pp | Status | Evidence |
| --- | --- | ---: | ---: | ---: | --- | --- |
| H1 | Shared-session FIFO correlation | 60.0% | 60.0% | +0.0 | Partial | [Baseline][B]; candidate `fbp_request_lanes` |
| H2 | Callback panic containment | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; candidate `worker_guards` |
| H3a | Inventory lock inversion | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B] |
| H3b | Session/drain and worker interleavings | 78.0% | 78.0% | +0.0 | Partial | [Baseline][B]; candidate `native_sessions`, `worker_guards` |
| H4 | Scheduling scans | 76.0% | 76.0% | +0.0 | Partial | [Baseline][B]; later optimization stack not integrated |
| H5 | Closure analysis | 71.0% | 71.0% | +0.0 | Partial | [Baseline][B]; later optimization stack not integrated |
| H6 | Performance profiling | 70.0% | 70.0% | +0.0 | Partial | [Baseline][B]; no new paired measurements |
| H7 | Request isolation and fairness | 56.0% | 56.0% | +0.0 | Partial | [Baseline][B]; candidate `fbp_request_lanes`, bounded RR |
| C1 | Terminal transition precedence | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; candidate recovery regression gates |
| C2 | Obligation deduplication | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; candidate closure-only fairness test |
| C3 | Retired-generation revocation | 20.0% | 20.0% | +0.0 | Open, Decision required | [Archival/revocation alternatives][C] |
| C4 | Preparation authority | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; candidate binding compile-fail tests |
| C5 | Deferred-job watchdog | 54.0% | 54.0% | +0.0 | Partial | [Baseline][B]; candidate `fbp_deferred_timeout`, driver wake test |
| C6 | State migration/restart policy | 12.0% | 12.0% | +0.0 | Open | [Restart-only contract][C]; candidate `generations` |
| C7 | Plugin isolation boundary | 12.0% | 12.0% | +0.0 | Open | [Trusted-native boundary][C]; OS adapter missing |
| C8 | Code-lease retention | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; candidate admission-error/timeout lease regressions |
| C9 | Passive session state | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; must reconcile later-stack API semantics |
| C10 | Redundant execution API | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; later-stack removal not integrated |
| A1 | Private composite extension | 18.0% | 18.0% | +0.0 | Open | [Baseline][B]; general extension API missing |
| A2 | Interface compatibility | 16.0% | 16.0% | +0.0 | Open | [Baseline][B]; existing contract/replacement tests |
| A3 | Graph/store separation | 20.0% | 20.0% | +0.0 | Open | [Baseline][B]; dependency direction unchanged |
| A4 | Typed authoring consolidation | 30.0% | 30.0% | +0.0 | Open | [Baseline][B]; not redesigned in this batch |
| A5 | Typed vs. string paths | 24.0% | 24.0% | +0.0 | Open | [Baseline][B]; not redesigned in this batch |
| A6 | Admission API ergonomics | 30.0% | 30.0% | +0.0 | Open | [Baseline][B]; candidate typed closure errors only |
| A7 | Identifier bounds | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; later-stack bounds not integrated |
| Q1 | Responsibility concentration | 22.0% | 22.0% | +0.0 | Open | [Baseline][B]; candidate exclusive-ownership primitive |
| Q2 | Incremental compilation | 10.0% | 10.0% | +0.0 | Open | [Baseline][B]; not implemented |
| Q3 | Dependency scaling | 14.0% | 14.0% | +0.0 | Open | [Baseline][B]; not implemented |
| Q4 | Static liveness guarantees | 10.0% | 10.0% | +0.0 | Open | [Baseline][B]; explicit blocking limits only |
| Q5 | Concurrency verification | 74.0% | 74.0% | +0.0 | Open | [Baseline][B]; candidate actual-source three-property session Loom |
| Q6 | Diagnostic quality | 22.0% | 22.0% | +0.0 | Open | [Baseline][B]; candidate deadline path/request/ticket context |
| Q7 | Typed error consistency | 26.0% | 26.0% | +0.0 | Open | [Baseline][B]; candidate invalid closure-port details |
| Q8 | Report reconciliation | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; historical report labels corrected |

[B]: https://github.com/matthis-k/phenix-ai/actions/runs/43537983510
[C]: fbp-hardening-campaign-20261114.md

## Candidate Evidence, Not Yet Score-Accepted

- Kernel producer callbacks now run outside the producer-state mutex. A callback
  lease preserves single-drain behavior; callback panic is terminal for poll and
  a typed driver error for wake registration. Deterministic tests verify lock
  availability, bounded re-entry, panic cleanup, and callback retirement races.
- Session operations now move exclusive session ownership out of the mutex while
  executing callbacks. Same-thread access to that Kernel fails fast; other threads
  wait on a condition variable. RAII returns the session on normal and panic paths.
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
- Focused graph, kernel, and storage-graph suites passed locally, including the new
  adversarial cases. Workspace Clippy and format checks passed. Local
  `nix flake check --no-update-lock-file --print-build-logs` passed all eight
  declared derivation checks after the correlation source was included in the Git
  source inventory. This is local staged-tree evidence, not GitHub exact-head
  evidence for the PR #1300 branch.
- A same-harness optimized microbenchmark on the clean starting commit vs. candidate
  measured medians of 639/757 ns for no-op `with_session`, 34/100 ns for empty
  `run_steps`, and 23/39 ns for empty `drain` (baseline/candidate). This is a
  synthetic empty-kernel comparison, not end-to-end throughput or allocation
  evidence. The absolute added cost is at most 118 ns per measured call; H6 remains
  partial and unchanged.

Q5 and H3b remain at their verified baseline 74.0% and 78.0%. No arbitrary
plugin-vs-plugin or host-lock deadlock-freedom claim is made.

## Campaign Evidence And Remaining Gaps

All table deltas compare with the immediately preceding verified candidate above.
All unchanged entries inherit that baseline's evidence, not new completion credit.
Candidate changes are tracked in [the campaign design](fbp-hardening-campaign-20261114.md).

| Issues | Candidate implementation/evidence | Closure gap |
| --- | --- | --- |
| H3b, Q5 | Unlocked producer/session ownership transfer, worker execution guard, real-thread callback regressions, actual-source three-property session Loom verifier | Not a transplant of the PR #1300 wake verifier; arbitrary plugin-created thread/lock cycles remain outside the contract |
| C5 | Opt-in deferred handoff timeout; exact-ticket invalidation; driver deadline wake; `fbp_deferred_timeout` and stream/native-session tests | Cooperative, not preemptive; external workers retain jobs/leases and can still perform effects |
| C6 | Generation replacement tested with fresh process-private state and independent old sessions | Restart-only policy documented; universal live-state migration not implemented |
| C3 | Explicit leased archival behavior documented | **Decision required:** strict old-generation revocation would change existing public behavior and independently cloned execution capabilities |
| C7 | In-process native callbacks explicitly classified as trusted | OS-isolated untrusted plugin adapter not implemented |
| H1, H7 | Opt-in bounded FIFO lanes across ingress, internal edges, captures; round-robin runnable lane selection; EOS, cancellation/reuse, retained-capture, mixed-traffic regressions | Queue isolation is not independent process state or progress through a nonconsuming reader; no per-payload byte budget or universal fairness claim |
| H4, H5, H6 | No optimization/acceptance credit | Current branch does not contain the later PR #1300 scheduler optimization stack; representative paired timing/allocation/RSS acceptance remains outstanding |
| A1, A2, A4, A5, A6, Q2, Q3 | Existing typed authoring/composer regression suites retained | General private extension, authoring consolidation, admission ergonomics, incremental compilation and dependency scaling remain |
| A3, Q1, Q6, Q7 | Explicit ownership primitive; typed invalid closure diagnostics; deadline context | Broader graph/store authority separation, diagnostics and error provenance audit remain |
| Q4 | Intentional blocking and finite FIFO limitations documented/tested | Static liveness analysis and sound progress diagnostics remain |
| H2, H3a, C1, C2, C4, C8, C9, C10, A7, Q8 | Baseline scoped verification retained as acceptance obligations, not re-proven by this branch's CI | Later-stack guarantees must be reconciled before this candidate can replace the verified baseline |

M0-M6 retain substantial native implementation, but this candidate does not close
M7. M8 P1/P2 production-agent integration is still outstanding; no executable model,
credential/provider, durable conversation, local/SSH tool or frontend adapter is
claimed by the queue and lifecycle work.
