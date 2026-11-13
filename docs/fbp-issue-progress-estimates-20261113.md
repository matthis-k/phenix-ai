# FBP 33-Issue Progress Estimates

**Immediately preceding verified baseline:** PR #1300 at
`1a57f7a6ebdf512200244e1b14cb4a979ce99128`; Flake CI
[43437130509](https://github.com/matthis-k/phenix-ai/actions/runs/43437130509),
native Q5 [43437129633](https://github.com/matthis-k/phenix-ai/actions/runs/43437129633),
and Loom [43437129586](https://github.com/matthis-k/phenix-ai/actions/runs/43437129586)
passed. This tracker uses subjective engineering issue-completion estimates, not
safety probabilities or overall project completion. All stacked PRs remain draft
and unmerged.

The baseline unweighted mean is **55.2%**. Issue classifications remain **10
scoped verified / 7 partial / 16 open**. Scores stay unchanged in this batch: the
two fixed teardown paths are meaningful evidence, but do not close their broader
issues. Q5 remains open; H3b remains partial.

## All 33 Issues

| Issue | Scope | Classification | Previous | Current | Δ (pp) |
| --- | --- | --- | ---: | ---: | ---: |
| H1 | Shared-session FIFO correlation | Partial | 60.0% | 60.0% | +0.0 |
| H2 | Callback panic containment | Scoped verified | 100.0% | 100.0% | +0.0 |
| H3a | Inventory lock inversion | Scoped verified | 100.0% | 100.0% | +0.0 |
| H3b | Session/drain and delegated-worker interleavings | Partial | 76.0% | 76.0% | +0.0 |
| H4 | Scheduling scans | Partial | 76.0% | 76.0% | +0.0 |
| H5 | Closure analysis | Partial | 71.0% | 71.0% | +0.0 |
| H6 | Performance profiling | Partial | 70.0% | 70.0% | +0.0 |
| H7 | Request isolation and fairness | Partial | 56.0% | 56.0% | +0.0 |
| C1 | Terminal transition precedence | Scoped verified | 100.0% | 100.0% | +0.0 |
| C2 | Obligation deduplication | Scoped verified | 100.0% | 100.0% | +0.0 |
| C3 | Retired-generation revocation | Open | 20.0% | 20.0% | +0.0 |
| C4 | Preparation authority | Scoped verified | 100.0% | 100.0% | +0.0 |
| C5 | Deferred-job watchdog | Partial | 54.0% | 54.0% | +0.0 |
| C6 | Live process-state migration | Open | 12.0% | 12.0% | +0.0 |
| C7 | Plugin sandbox boundary | Open | 12.0% | 12.0% | +0.0 |
| C8 | Code-lease retention | Scoped verified | 100.0% | 100.0% | +0.0 |
| C9 | Passive session state | Scoped verified | 100.0% | 100.0% | +0.0 |
| C10 | Redundant execution API | Scoped verified | 100.0% | 100.0% | +0.0 |
| A1 | Private composite extension | Open | 18.0% | 18.0% | +0.0 |
| A2 | Interface compatibility | Open | 16.0% | 16.0% | +0.0 |
| A3 | Graph/store separation | Open | 20.0% | 20.0% | +0.0 |
| A4 | Typed authoring consolidation | Open | 30.0% | 30.0% | +0.0 |
| A5 | Typed vs. string paths | Open | 24.0% | 24.0% | +0.0 |
| A6 | Admission API ergonomics | Open | 30.0% | 30.0% | +0.0 |
| A7 | Identifier bounds | Scoped verified | 100.0% | 100.0% | +0.0 |
| Q1 | Responsibility concentration | Open | 22.0% | 22.0% | +0.0 |
| Q2 | Incremental compilation | Open | 10.0% | 10.0% | +0.0 |
| Q3 | Dependency scaling | Open | 14.0% | 14.0% | +0.0 |
| Q4 | Static liveness guarantees | Open | 10.0% | 10.0% | +0.0 |
| Q5 | Concurrency verification | Open | 73.0% | 73.0% | +0.0 |
| Q6 | Diagnostic quality | Open | 22.0% | 22.0% | +0.0 |
| Q7 | Typed error consistency | Open | 26.0% | 26.0% | +0.0 |
| Q8 | Report reconciliation | Scoped verified | 100.0% | 100.0% | +0.0 |

## Previously Accepted Baseline Evidence

PR #1300 raised Q5 from 70.0% to 73.0% and H3b from 74.0% to 76.0%, using the
actual `wake.rs` Loom verifier, all 13,440 serialized wake-operation orders,
real notification/retirement races, delegated-worker cancellation/replacement,
and plugin-lock inversion challenges for custom readiness, dispatch, and request
recovery. Its native Q5 suite repeats eight times. This batch does not repeat that
coverage; it closes two separate destructor/cleanup gaps found on the exact head.

## Current Batch Evidence

- `Subscription::drop` takes the stored Waker while the Watch mutex is held, then
  sends its destruction through the existing deferred/panic-isolated Waker path.
  A custom destructor regression verifies success, transaction error, and unwind
  all defer destruction until the Watch lock and storage reentrancy guard are clear.
- Request queue cancellation now detaches packets and updates request counts/FIFO
  before payload destruction. `ProcessHandoff` invalidates tickets and detaches
  process/context/code lease state; request/session transitions retire all wakes
  and process ownership before disposing that state. Destructors are isolated one
  at a time, and public cancellation reports typed cleanup failures after cleanup.
- Adversarial tests cover panicking payloads in a queued packet and a reserved
  process context, stale wake rejection, fresh-request progress, and direct handoff
  cancellation reporting. The five-property existing Loom verifier passes; no
  Loom model was added because these changes use mutex-owned detachment and leave
  production wake synchronization unchanged.
- Candidate-local full Flake, strict Clippy/format, focused graph/kernel/storage
  suites pass. Exact-head GitHub runs are still required before treating this batch
  as accepted.

The measured performance issue H6 is unchanged; this cleanup runs only on teardown
and cancellation paths. No throughput improvement is claimed. Remaining risks
include arbitrary plugin-vs-plugin and cross-mount lock cycles, panics from multiple
nested destructors during one destructor's own unwinding, and unrevocable effects
already started by delegated work.
