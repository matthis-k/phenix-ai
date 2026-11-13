# FBP issue progress — one-decimal tracked estimates

**Scope and comparison:** exactly the previous **verified** estimates on [PR #1297](https://github.com/matthis-k/phenix-ai/pull/1297) at `2064a1a3a713d635dfec33dac72f669cfe026473` (six-job [Flake CI #43418004760](https://github.com/matthis-k/phenix-ai/actions/runs/43418004760) and 12-repeat worker [#43418004098](https://github.com/matthis-k/phenix-ai/actions/runs/43418004098) both green) to **the present test-only Q5 operation-order candidate**. These numbers are *subjective engineering completion estimates*, not measured safety probabilities or production/project-completion KPIs. The branch and all ancestors remain draft/unmerged.

**33-issue unweighted mean:** **54.9% → 55.0%** (visually **+0.1 pp**, unrounded **+0.045 pp**). Scores use rounded one-decimal issue values; the mean is *not* M7 or M8 completion. Issue statuses remain **10 scoped verified / 7 partial / 16 open**; Q5's limited new model evidence does not close it.

**Acceptance pending at authorship:** exact-head six-job CI and the already configured eight-repeat Q5 `wake.rs` workflow must pass. A candidate score increase is not final until those checks pass; accepted evidence should be recorded in the PR's Phase 4 comment.

## All 33 issue estimates — previous → candidate → change

| Issue | Focus | Previous | Candidate | Δ (pp) |
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
| Q5 | Concurrency verification | 65.5% | 67.0% | +1.5 |
| Q6 | Diagnostic quality | 22.0% | 22.0% | 0.0 |
| Q7 | Typed error consistency | 26.0% | 26.0% | 0.0 |
| Q8 | Report reconciliation | 100.0% | 100.0% | 0.0 |

## What changed in this iteration

- **Q5 65.5% → 67.0% (+1.5 pp, conditional on CI):** the previous #1296/#1297 checks exercised *real concurrent* notify/retire and workers executing during cancellation. The new `runtime/wake.rs` test checks **all 13,440 valid linearized orders of eight wake/retirement/terminal/drain events** against the *actual* `WakeSignal` after **every** operation and a separate expected-state model, excluding impossible fresh-owner-before-retirement histories. This is **not Loom**, instruction-level schedule exploration, external-plugin lock proof, or deadlock freedom.
- **H3b 74.0% → 74.0% (0.0 pp):** no new delegated worker/reentrant callback coverage in this iteration; avoid double counting the prior in-flight job proof.
- **Other 31 issues: 0.0 pp** additional verified progress. Earlier H4/H5 optimizations and H6 allocation profiling are already included in the immediately prior scores; rejected #1295 hot-path optimization must never be counted as accepted progress.

## Scoring and acceptance rules

1. Each subsequent comparison uses the last **verified posted current score**, not a historic ledger baseline. Every issue retains previous, current and signed one-decimal Δ, even when 0.0.
2. A passed bounded test adds only the assurance for the specifically tested boundary. Q5 remains Open until an accepted systematic thread/interleaving model and external/plugin locking contract; H3b remains Partial.
3. Persist candidate SHA, exact-head CI, branch/PR status, source-test evidence and any rejection. Do not describe a green draft as merged, and do not claim M7 complete from a 33-issue mean.
4. Keep decisions and rejected experiments in the record. A failed check results in **no accepted increase** until corrected and exact-head retested.
