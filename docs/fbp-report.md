# Native FBP — current implementation and acceptance report

**Updated 2026-11-12.** This report replaces the earlier *uncommitted
worktree* snapshot, which incorrectly described current implementation as
lacking commits or remote CI. For per-issue status use
[fbp-issue-ledger-20261111.md](fbp-issue-ledger-20261111.md).

## Verified commit and branch status

- Original parent: `impl/graph-consolidation-20261110` at `89e00d06498979a2f231dfc8ca780eba44140d27`.
- [PR #1263](https://github.com/matthis-k/phenix-ai/pull/1263):
  H2 callback panic containment, same-thread H3b and terminal driver fixes,
  `72d56e413321b58204280b8b6b1d29d6aa5e3f1a`.
  [Exact-head six-job CI successful](https://github.com/matthis-k/phenix-ai/actions/runs/43185486539).
- [PR #1265](https://github.com/matthis-k/phenix-ai/pull/1265):
  cross-thread session/drain cycle and exclusive driver ownership,
  `fe09a68da755a8a435a2df4927b47db0e3cc73eb`, stacked on #1263.
  [Exact-head six-job CI successful](https://github.com/matthis-k/phenix-ai/actions/runs/43197663375).
- Both PRs were **open, reviewed/ready, mergeable and unmerged** when this
  report was produced. Neither base branch independently includes the newer
  stacked fixes. This documentation/benchmark branch requires its own CI.

## Delivered model and implementation

The native FBP engine uses one `ComponentDef` primitive/composite model,
typed input/output interfaces, compile-time flattened composites, and
`NetworkBuilder`/network-owned connections. Runtime
`NetworkSession` persists across admissions and implements bounded FIFO
transport, atomic fanout, EOS and capture backpressure, deferred owned jobs,
wake notification, cancellation and request-scoped work. Closure semantics
retain committed state and explicitly dispose cancelled packets.

`NetworkPatch` composition is host-authorized and declarative.
Kernel/plugin generation composition preserves leases and scope, and
StorageGraph integrates typed resources and observable storage changes.
The old graph V2/wave execution API and compatibility shims are not
preserved in production code. A deterministic iterative-agent composition
demonstrates ordinary plugin extension points; it is not equivalent to
a production LLM/tool/streaming agent product.

## Recent correctness hardening

| Area | Behavior | Evidence |
| --- | --- | --- |
| H2 callbacks | Typed panic containment in plugin and driver-owned callbacks; host event errors include producer identity | PR #1263, `callback_errors` and kernel driver regressions |
| H3b same-thread | RAII recursion guard is lazy; no outer guard loss or notifier mutation on rejected nested driver | PR #1263 |
| Terminal states | Completed returns success, Failed/Cancelled remain typed errors, first-terminal-wins preserved | PR #1263, `native_sessions` and live/empty fixtures |
| H3b two-thread | Producer callback session operations fail promptly on contended session lock rather than joining session↔drain cycle | PR #1265, ordered two-thread regression |
| Driver lease | Concurrent `run_until` registration returns typed `AlreadyRunning`; drop/retry recovery tested | PR #1265, kernel unit regressions |

Exact CI verifies tests, formatting, Clippy and configured build matrix.
It does not establish full thread-interleaving correctness, ABI safety or
timing guarantees.

## Size, performance and limitations

[fbp-measurements.md](fbp-measurements.md) compared one early native
snapshot to an older graph baseline and reported **15,688 → 7,645
non-test graph/derive physical LOC**. This is a historical structural
measurement, **not a current audited HEAD count and not a speedup**.
The first **native runtime latency baseline** is now documented in
[fbp-performance-benchmark.md](fbp-performance-benchmark.md), with
three raw CSV repetitions from [CI #43201132241](https://github.com/matthis-k/phenix-ai/actions/runs/43201132241).
One active ingress showed approximately 47× higher mean event latency
with 256 processes versus 1 on AMD EPYC 9V45; an independent
[second run](https://github.com/matthis-k/phenix-ai/actions/runs/43201797923)
on AMD EPYC 7763 reported roughly 56× within-run slowdown.
Cross-run absolute times must **not** be averaged because the CPU
model changed. This is a narrow, shared-runner hot-vs-idle microbenchmark,
**not** an allocation, concurrency throughput, pinned-environment
or production-agent baseline. H6 remains partial;
H4 is still not optimized.

Primary unresolved concerns: H3b host Drop and external cross-thread
synchronizers; H4 refresh inventory scans; H5 residual packet closure scans;
H6 measured performance; H7 shared-session request isolation/fairness;
C3 archival generation policy; C6 state migration; C7 in-process plugin
sandbox limitation; A1–A6 API/design; Q1–Q7 quality/performance evidence.

## Milestone acceptance

| Area | Assessment |
| --- | --- |
| M0–M3 | Substantially implemented, exercised by native examples and regression suites |
| M4 | Persistent runtime implemented; concurrency and scaling acceptance incomplete |
| M5–M6 | Kernel/StorageGraph integrations and old-engine removal implemented |
| M7 | **Not complete**: lifecycle, fairness, performance and independent acceptance gaps remain |
| Proposed M8 | Agent-domain iterative example/design exists; not product-ready model/tool integration |

**Disposition:** the stacked 1263/1265 corrections are CI-verified.
The full FBP issue ledger is **not** closed; no universal deadlock,
static liveness, performance, automatic migration or plugin sandbox claim.
