# FBP 33-issue progress estimates — Q5/H3b plugin lock-order batch

**Comparison baseline:** last exact-head-verified [#1299](https://github.com/matthis-k/phenix-ai/pull/1299) at `c76e82dfeb7139fe9d0d8dfcd1b23b96ac1ede1f`, [Flake CI #43427604234](https://github.com/matthis-k/phenix-ai/actions/runs/43427604234), [Loom #43427603995](https://github.com/matthis-k/phenix-ai/actions/runs/43427603995) and [8-repeat native Q5 #43427604436](https://github.com/matthis-k/phenix-ai/actions/runs/43427604436) green. New batch is *candidate only* pending all exact-head checks. This is a subjective engineering issue-completion assessment, not a measured safety probability, absolute production readiness or M7 completion score. All stacked PRs remain draft and unmerged.

**Unweighted issue mean:** **55.1% → 55.2%** (**+0.1 percentage points displayed; +0.152 pp unrounded**). Issue weights and dependencies vary drastically. Formal issue classification remains **10 scoped verified / 7 partial / 16 open**; Q5 and H3b are not fully resolved.

## All 33 issues — previous → candidate → delta

| Issue | Scope | Previous | Current candidate | Δ (pp) |
| --- | --- | ---: | ---: | ---: |
| H1 | Shared-session FIFO correlation | 60.0% | 60.0% | 0.0 |
| H2 | Callback panic containment | 100.0% | 100.0% | 0.0 |
| H3a | Inventory lock inversion | 100.0% | 100.0% | 0.0 |
| H3b | Session/drain & delegated-worker interleavings | 74.0% | 76.0% | +2.0 |
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
| Q5 | Concurrency verification | 70.0% | 73.0% | +3.0 |
| Q6 | Diagnostic quality | 22.0% | 22.0% | 0.0 |
| Q7 | Typed error consistency | 26.0% | 26.0% | 0.0 |
| Q8 | Report reconciliation | 100.0% | 100.0% | 0.0 |

## Evidence and attribution

- **Q5 70.0% → 73.0% (+3.0 pp, conditional):** extend the original-source Loom verifier with concurrent targeted-notification vs scheduler-drain and modeled plugin-lock/notification ordering. Add a real-session callback lock-order challenge for custom readiness, dispatchers and factory recovery. Each of the three kinds performs 12 rounds under real threads; the **existing eight-repeat** Q5 workflow executes them together with its current notifier retirement/recovery tests (up to 288 plugin callback cycles total). The exact `wake.rs` implementation still has unchanged semantics. [Adversarial design](docs/fbp-q5-plugin-lock-order-batch-20261113.md).
- **H3b 74.0% → 76.0% (+2.0 pp, conditional):** the three callback boundaries previously lacked explicit *plugin mutex held while notifying* integration regressions, a plausible cross-thread lock inversion. Factory recovery includes replacement-generation wake authority and request settlement. This doesn't establish plugin A ↔ B or arbitrary host-lock deadlock freedom.
- **All other 31 issues: +0.0 pp**; no new H4/H5/H6 optimization, H7 fairness proof or M8 production-agent integration. The earlier rejected #1295 runtime optimization remains excluded.

## Acceptance / scorekeeping protocol

1. Batch work before PR creation to make the **next CI run cover the entire increment**, instead of many formatting/test-only micro-PRs. Keep the dedicated native Q5 suite combined and Loom verifier isolated.
2. Do not claim the score increases until the **exact final-head** six-job Flake CI, eight-repeat Q5 suite, five-property Loom verifier and H4/H5/H6 acceptance workflows pass. Any failure holds previous verified scores (Q5 70.0, H3b 74.0) and triggers a focused correction/rejection.
3. Future increments compare **immediately previous verified** per-issue values, keeping one decimal and signed delta across all 33 issues, including zeros. Use commits, checks and tests to ground changes; avoid double-counting older evidence.
4. Q5 remains **Open** for unbounded real interleavings, all plugin/host mutex cycles and listener timeout path. H3b remains **Partial**. Green tests do not imply production branch merge, M7 completion or global deadlock freedom.
