# FBP issue-level progress estimate — November 13, 2026

**Comparison baseline:** the previous 33-issue, one-decimal estimates reported in chat on November 13 (Q5 **61.5**, H3b **72.0**). This is a *subjective engineering completion estimate*, **not** a percentage automatically measured by passing tests or a probability that code is correct.

**Current scope:** verified, still-unmerged stack through [Q5 in-flight recovery PR #1297](https://github.com/matthis-k/phenix-ai/pull/1297). Base [Q5 notifier retirement PR #1296](https://github.com/matthis-k/phenix-ai/pull/1296) passed [six-job exact-head CI #43413378069](https://github.com/matthis-k/phenix-ai/actions/runs/43413378069). [PR #1297 dedicated repeated worker tests](https://github.com/matthis-k/phenix-ai/actions/runs/43417638306) passed 12 × 48 real in-flight cancellation/recovery cases; exact final-head [Flake CI #43417638586](https://github.com/matthis-k/phenix-ai/actions/runs/43417638586) must also pass before full Phase 4 acceptance. No production-code change; these estimates reflect improved assurance, *not* a new capability.

**Unweighted issue mean:** **54.7% → 54.9% (displayed Δ 0.2 percentage points)**. Raw unrounded delta 0.182 pp; rounding affects the displayed before/after delta. This mean is **not** overall FBP milestone completion: issues overlap, differ drastically in impact, and include policy decisions and intentionally unsolved limits. The formal 33-issue classification remains **10 scoped-verified / 7 partial / 16 open**.

## Tracked estimates

| Issue | Focus | Previous | Current | Δ (pp) |
| --- | --- | ---: | ---: | ---: |
| H1 | Shared-session FIFO correlation | 60.0% | 60.0% | 0.0 |
| H2 | Callback panic containment | 100.0% | 100.0% | 0.0 |
| H3a | Inventory lock inversion | 100.0% | 100.0% | 0.0 |
| H3b | Session/drain & delegated-worker interleavings | 72.0% | 74.0% | +2.0 |
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
| Q5 | Concurrency verification | 61.5% | 65.5% | +4.0 |
| Q6 | Diagnostic quality | 22.0% | 22.0% | 0.0 |
| Q7 | Typed error consistency | 26.0% | 26.0% | 0.0 |
| Q8 | Report reconciliation | 100.0% | 100.0% | 0.0 |

## Why these two estimates changed

- **Q5 61.5% → 65.5% (+4.0 pp)**: previously only 128× notifier/retirement, 96× wake-listener and 96× process-index recovery schedules per repeat were covered. [#1297](https://github.com/matthis-k/phenix-ai/pull/1297) adds an actual worker executing **inside** `Process::step` when cancellation replaces the owner, with 3 separate publication/return orders and 48 rounds repeated 12 times. Old completion is rejected as `ForeignCompletion` after owner replacement; this distinction is grounded in `ProcessHandoff::replacement` mints a new owner and `complete_job` requires matching ownership. The test asserts correct surviving request output, one non-revocable old external effect, no old capture and no new-step contamination.
- **H3b 72.0% → 74.0% (+2.0 pp)**: additional worker-side reentrancy/lifetime boundary is tested through the actual process handoff and session cancellation while the worker still owns application code. External plugin/user-lock cycles, arbitrary driver callbacks and exhaustive interleavings remain unresolved.
- Other issues have **0.0 pp** additional verified change in this iteration. The accepted H6 profiler (#1294) and rejected H6 optimizer (#1295) are already represented in the prior baseline and must not be counted again.

## Scoring/acceptance contract for future iterations

1. Every report includes **issue code, previous %, current %, signed Δ in pp**, even when Δ = 0.0; top summary can show only changed rows but retains full report in a persistent scorecard.
2. Compare to the **immediately previous posted estimate**, not the November 12 original ledger. Change only issues whose acceptance evidence substantively improved; avoid counting a synthetic stress repeat as if it closed all concurrency concerns.
3. A score of **100.0%** means the *stated scoped issue* is accepted with exact-head tests/evidence, not absolute security, safety or production guarantees.
4. Include branch, commit and exact-head CI; distinguish draft from merged code. If a test fails or is rejected, do not credit it with an accepted score increase.
5. Full M7/M8 completion and cross-issue architectural quality are **not** the average issue score. The formal status distribution is updated separately when an entire issue's acceptance criteria close.
