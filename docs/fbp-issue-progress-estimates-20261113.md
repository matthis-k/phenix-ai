# FBP 33-issue one-decimal progress — November 13, 2026

**Baseline:** the last exact-head-verified [PR #1298](https://github.com/matthis-k/phenix-ai/pull/1298), commit `54494be22d87d227111484b5bb7903face721682`, [Flake CI #43420534000](https://github.com/matthis-k/phenix-ai/actions/runs/43420534000) and [eight-repeat concurrency #43420533621](https://github.com/matthis-k/phenix-ai/actions/runs/43420533621) successful. Current candidate: [PR #1299](https://github.com/matthis-k/phenix-ai/pull/1299), isolated **Loom recompile of the original WakeSignal method bodies** and a standalone pinned dependency graph. The numbers are subjective completion estimates, **not** measured reliability probabilities, and the new Q5 increment remains conditional on this exact head passing CI.

**Unweighted 33-issue mean:** **55.0% → 55.1%** (displayed change **+0.1 pp**, unrounded +0.091 pp). This average **is not overall M7/project completion**, because issue costs, risk and overlap vary. Formal issue status stays **10 scoped verified / 7 partial / 16 open**: the bounded Loom slice does not close Q5.

## Every issue — immediately previous → new candidate → change

| Issue | Focus | Previous | Current candidate | Δ (pp) |
| --- | --- | ---: | ---: | ---: |
| H1 | Shared-session FIFO correlation | 60.0% | 60.0% | 0.0 |
| H2 | Callback panic containment | 100.0% | 100.0% | 0.0 |
| H3a | Inventory lock inversion | 100.0% | 100.0% | 0.0 |
| H3b | Session/drain & delegated-worker interleavings | 74.0% | 74.0% | 0.0 |
| H4 | Scheduling scans | 76.0% | 76.0% | 0.0 |
| H5 | Closure analysis | 71.0% | 71.0% | 0.0 |
| H6 | Performance profiling | 70.0% | 70.0% | 0.0 |
| H7 | Request isolation & fairness | 56.0% | 56.0% | 0.0 |
| C1 | Terminal transition precedence | 100.0% | 100.0% | 0.0 |
| C2 | Obligation deduplication | 100.0% | 100.0% | 0.0 |
| C3 | Retired-generation revocation | 20.0% | 20.0% | 0.0 |
| C4 | Preparation authority | 100.0% | 100.0% | 0.0 |
| C5 | Deferred-job watchdog | 54.0% | 54.0% | 0.0 |
| C6 | Live process-state migration | 12.0% | 12.0% | 0.0 |
| C7 | Plugin sandbox boundary | 12.0% | 12.0% | 0.0 |
| C8 | Code-lease retention | 100.0% | 100.0% | 0.0 |
| C9 | Passive session state | 100.0% | 100.0% | 0.0 |
| C10 | Redundant execution API | 100.0% | 100.0% | 0.0 |
| A1 | Private composite extension | 18.0% | 18.0% | 0.0 |
| A2 | Interface compatibility | 16.0% | 16.0% | 0.0 |
| A3 | Graph/store separation | 20.0% | 20.0% | 0.0 |
| A4 | Typed authoring consolidation | 30.0% | 30.0% | 0.0 |
| A5 | Typed vs string paths | 24.0% | 24.0% | 0.0 |
| A6 | Admission API ergonomics | 30.0% | 30.0% | 0.0 |
| A7 | Identifier bounds | 100.0% | 100.0% | 0.0 |
| Q1 | Responsibility concentration | 22.0% | 22.0% | 0.0 |
| Q2 | Incremental compilation | 10.0% | 10.0% | 0.0 |
| Q3 | Dependency scaling | 14.0% | 14.0% | 0.0 |
| Q4 | Static liveness guarantees | 10.0% | 10.0% | 0.0 |
| Q5 | Concurrency verification | 67.0% | 70.0% | +3.0 |
| Q6 | Diagnostic quality | 22.0% | 22.0% | 0.0 |
| Q7 | Typed error consistency | 26.0% | 26.0% | 0.0 |
| Q8 | Report reconciliation | 100.0% | 100.0% | 0.0 |

## Evidence for this iteration

- **Q5 67.0% → 70.0% (+3.0 pp, conditional on exact-head CI):** previous tests cover 13,440 exhaustive lock-linearized orders (repeated 8×), 2-thread notifier/retirement races and true in-flight delegated worker cancellation. The new verification-only crate `verification/fbp-wake-loom` **includes the original `runtime/wake.rs` implementation** and substitutes Loom Mutex/Arc/Condvar/AtomicBool, not a hand-maintained reimplementation. Three distinct two-thread properties verify terminal invalidation, old-slot retirement and same-index fresh-slot authority under **bounded instrumented instruction interleavings**. The initial source compiled and passed all three models: 42 + 71 + 32 explored schedules. The first workflow failed *after* those tests because rustdoc did not preserve the custom cfg; the workflow now runs library tests only. The verifier has its own `Cargo.lock`, pinned `loom = "=0.7.2"`, outside the production Cargo workspace.
- **H3b 74.0% → 74.0% (0.0 pp)**: no new general plugin lock-order, callback reentrancy or delegated-worker coverage in this slice. Existing Q5 worker tests remain in the stacked branch.
- **Other 31 issues: 0.0 pp**. No performance score increase: production wake operations have unchanged method bodies, and the rejected #1295 optimization remains excluded.

## Accepted limitations and next acceptance steps

1. The Loom models cover the **actual wake mutex/atomic methods**, but not `WakeListener::wait_timeout_while`: Loom 0.7.2's Condvar lacks that method, so the verifier excludes this method; existing native listener tests cover it. Model exploration is limited to max 3 threads, **two preemptions**, 20,000 permutations/property and only the three specified scenarios. More than one completed schedule must be observed for each.
2. No assertion of exhaustive Loom state space beyond these bounds, all plugin/external lock orders, general deadlock freedom, cancellation of external tool effects, or M7 completion. Q5 remains Open and H3b Partial.
3. Any CI failure blocks acceptance. The exact final-head Flake CI (six jobs), native Q5 threaded tests, H4/H5/H6 benchmarks and dedicated Loom workflow must pass before upgrading candidate scores to verified.
4. In the next iteration, compare each issue with the immediately prior **verified** value, preserve one decimal on all 33, and record exact PR/commit/CI evidence. PRs are draft and unmerged unless explicitly merged.
