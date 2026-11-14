# FBP 33-Issue Progress Estimates

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
foundations. After fetching, the campaign is now merging exact verified PR #1300
head `516e0883875d5ec483e3595d50b77b3d5e8979c4` to preserve the complete stack
and all scoped gates. Earlier claims that the later stack was absent describe the
initial candidate, not the reconciled merge target.

**No issue score increase or closure is accepted** here. The new merged head's
full acceptance is pending; neither verified baseline CI nor local checks on
`0d145f3` accept that new head. This is the authoritative 33-issue scorecard;
the [historical issue ledger](fbp-issue-ledger-20261111.md) and campaign record
provide detail, not a second current score authority.

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
suite eight times. Their recorded passes apply to the verified head, not the new
merge. Candidate cases exercise additional kernel session/producer boundaries.

The baseline does not claim arbitrary plugin-vs-plugin or host-lock deadlock
freedom. Q5 remains open for unbounded real interleavings, and H3b remains partial.

## All 33 Issues

| Issue | Scope | Previous | Current | Δ pp | Status | Evidence |
| --- | --- | ---: | ---: | ---: | --- | --- |
| H1 | Shared-session FIFO correlation | 60.0% | 60.0% | +0.0 | Partial | [Baseline][B]; candidate `fbp_request_lanes` |
| H2 | Callback panic containment | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; candidate `worker_guards` |
| H3a | Inventory lock inversion | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B] |
| H3b | Session/drain and worker interleavings | 78.0% | 78.0% | +0.0 | Partial | [Baseline][B]; candidate `native_sessions`, `worker_guards` |
| H4 | Scheduling scans | 76.0% | 76.0% | +0.0 | Partial | [Baseline][B]; scheduler stack preserved by merge, acceptance pending |
| H5 | Closure analysis | 71.0% | 71.0% | +0.0 | Partial | [Baseline][B]; closure stack preserved by merge, acceptance pending |
| H6 | Performance profiling | 70.0% | 70.0% | +0.0 | Partial | [Baseline][B]; no new paired measurements |
| H7 | Request isolation and fairness | 56.0% | 56.0% | +0.0 | Partial | [Baseline][B]; candidate `fbp_request_lanes`, bounded RR |
| C1 | Terminal transition precedence | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; candidate recovery regression gates |
| C2 | Obligation deduplication | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; candidate closure-only fairness test |
| C3 | Retired-generation revocation | 20.0% | 20.0% | +0.0 | Open | **Decision required:** [archival/revocation alternatives][C] |
| C4 | Preparation authority | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; candidate binding compile-fail tests |
| C5 | Deferred-job watchdog | 54.0% | 54.0% | +0.0 | Partial | [Baseline][B]; candidate `fbp_deferred_timeout`, driver wake test |
| C6 | State migration/restart policy | 12.0% | 12.0% | +0.0 | Open | [Restart-only contract][C]; candidate `generations` |
| C7 | Plugin isolation boundary | 12.0% | 12.0% | +0.0 | Open | [Trusted-native boundary][C]; OS adapter missing |
| C8 | Code-lease retention | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; candidate admission-error/timeout lease regressions |
| C9 | Passive session state | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; passive `state(&self)` retained |
| C10 | Redundant execution API | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; API removal retained by merge |
| A1 | Private composite extension | 18.0% | 18.0% | +0.0 | Open | [Baseline][B]; general extension API missing |
| A2 | Interface compatibility | 16.0% | 16.0% | +0.0 | Open | [Baseline][B]; existing contract/replacement tests |
| A3 | Graph/store separation | 20.0% | 20.0% | +0.0 | Open | [Baseline][B]; dependency direction unchanged |
| A4 | Typed authoring consolidation | 30.0% | 30.0% | +0.0 | Open | [Baseline][B]; not redesigned in this batch |
| A5 | Typed vs. string paths | 24.0% | 24.0% | +0.0 | Open | [Baseline][B]; not redesigned in this batch |
| A6 | Admission API ergonomics | 30.0% | 30.0% | +0.0 | Open | [Baseline][B]; candidate typed closure errors only |
| A7 | Identifier bounds | 100.0% | 100.0% | +0.0 | Scoped verified | [Baseline][B]; bounds retained by merge |
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
| H3b, Q5 | Unlocked producer/session ownership transfer, ordered worker guards, whole-drain gate, exclusive driver ownership, real-thread regressions, actual-source three-property session Loom plus retained PR #1300 wake verifier | Exact merged-head acceptance pending; arbitrary plugin-created thread/lock cycles remain outside the contract |
| C5 | Only `SessionOptions.deferred_timeout`, existing `overdue_jobs(now)` / `supervise_deferred(now)`, exact-ticket invalidation and driver deadline wake | `state()` stays passive; supervision is cooperative, external workers retain jobs/leases and can still perform effects |
| C6 | Generation replacement tested with fresh process-private state and independent old sessions | Restart-only policy documented; universal live-state migration not implemented |
| C3 | Explicit leased archival behavior documented | **Decision required:** strict old-generation revocation would change existing public behavior and independently cloned execution capabilities |
| C7 | In-process native callbacks explicitly classified as trusted | OS-isolated untrusted plugin adapter not implemented |
| H1, H7 | Opt-in bounded FIFO lanes across ingress, internal edges, captures; round-robin runnable lane selection; EOS, cancellation/reuse, retained-capture, mixed-traffic regressions | Queue isolation is not independent process state or progress through a nonconsuming reader; no per-payload byte budget or universal fairness claim |
| H4, H5, H6 | Verified optimization stack and scoped gates retained by merge; no new acceptance credit | New exact-head gates and representative paired timing/allocation/RSS acceptance remain outstanding |
| A1, A2, A4, A5, A6, Q2, Q3 | Existing typed authoring/composer regression suites retained | General private extension, authoring consolidation, admission ergonomics, incremental compilation and dependency scaling remain |
| A3, Q1, Q6, Q7 | Explicit ownership primitive; typed invalid closure diagnostics; deadline context | Broader graph/store authority separation, diagnostics and error provenance audit remain |
| Q4 | Intentional blocking and finite FIFO limitations documented/tested | Static liveness analysis and sound progress diagnostics remain |
| H2, H3a, C1, C2, C4, C8, C9, C10, A7, Q8 | Baseline scoped guarantees retained as merge acceptance obligations, not re-proven by initial candidate CI | Full merged-head acceptance pending |

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
the authoritative current table: all 33 current deltas remain +0.0 pp.
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
