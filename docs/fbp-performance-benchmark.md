# H6 — reproducible FBP native runtime benchmark

Date: 2026-11-12. Status: **harness implemented; numerical baseline only
after an actual environment-labelled execution is captured.**
Do not compare the historical LOC savings in `fbp-measurements.md`
to throughput or claim an optimization has occurred.

## Phase 0 — observed uncertainty

Current `runtime/mod.rs::refresh()` iterates over the full process
inventory and repeats a pass after any process finishes. For a long-lived
agent graph, one hot ingress may be surrounded by many idle processes.
There has been **no measured** native scheduler/transaction throughput
baseline; an O(V) source audit alone does not quantify whether optimizations
are justified.

Alternative A: change refresh to incremental wake queues immediately:
reject, because changes in EOS, ready ordering, request obligations and
topology retirement are high risk without performance evidence.

Alternative B (chosen): add a repeatable, dependency-free workload:
one hot `u64` local-input sink and N-1 idle sinks with distinct **open**
public ingresses, each capacity one. On one persistent session, warm up
64 admissions, then admit one hot packet per sample, drive to idle and
measure. If an event is dropped, blocked or erroneously completes the
session, panic instead of treating it as an impressive throughput result.

Alternative C: benchmark compilation alone: reject for H4, but report
compile/start times as separate columns.

## Measured baseline (CI #43201132241)

This is **actual data**, not extrapolated or benchmark output invented
from the source audit. The benchmark workflow on 2026-11-12 recorded
three release-mode repetitions on a GitHub-hosted Ubuntu 24.04 runner
(x86_64, 4 vCPU, AMD EPYC 9V45, kernel 7.0.0-1017-azure).
Toolchain: Rust 1.96.1, Nix 2.35.6.
The pull-request workflow checked out synthetic merge commit
`332c0d87404e99d01b64b475d2d914f6b787d7de` containing
benchmark source head `522cf3f935743363a21104da73f840e6721bfb60`,
rather than checking out the head SHA directly.

Run: [Actions #43201132241](https://github.com/matthis-k/phenix-ai/actions/runs/43201132241).
[Raw CSV and environment artifact](https://github.com/matthis-k/phenix-ai/actions/runs/43201132241/artifacts/13934274002).
Each repetition: 5,000 events and 64 warmups, inventory 1/16/64/256.

| Processes | Mean ns/event rep 1 | rep 2 | rep 3 | Median of 3 means | Median p50 |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 679.2 | 678.9 | 680.7 | 679.2 | 621 ns |
| 16 | 2,541.1 | 2,463.9 | 2,448.5 | 2,463.9 | 2,413 ns |
| 64 | 8,434.7 | 8,318.4 | 8,312.6 | 8,318.4 | 8,202 ns |
| 256 | 32,094.2 | 31,285.6 | 32,342.5 | 32,094.2 | 31,057 ns |

Under one active ingress, increasing total process count from 1 to 256
raised mean event time by about **47×** in this shared-runner workload,
supporting the H4 concern that full-inventory scheduling scans scale
poorly for sparse activity. The 256-process median-of-three figure
represents about **31–32 µs/event**, not a universal latency guarantee.
Run-to-run variation remains visible, particularly at p95.

### Independent repeat on another runner (#43201797923)

A second successful three-repetition run on the same source workload but
a different GitHub-hosted CPU (**AMD EPYC 7763**, 4 vCPU, same Rust 1.96.1)
reported approximately **1.06 µs/event** for one process and
**59–60 µs/event** for 256 processes (~56× within-run slowdown).
[Run #43201797923](https://github.com/matthis-k/phenix-ai/actions/runs/43201797923)
contains the raw CSV; its synthetic test-merge checkout was
`e4eba8db8f87f42728e456ae7640a475a40b47c9`.
**Do not average absolute times across the two CPU models.**
The dedicated workflow is now configured to check out the exact
pull-request head for subsequent runs and prints both source and
workflow SHAs.

**Interpretation boundary:** this is a native hot-vs-idle microbenchmark,
not a representative agent latency measurement. The benchmark does not
yet record allocations/RSS or multiple active branches, joins, requested
correlation, deferred jobs, or concurrent host admission. Revalidate
after H4 implementation on the **same** topology, toolchain, and runner
class before claiming an optimization.

## Reproduction

```sh
nix develop --command cargo run --locked --release -p phenix-graph \
  --example fbp_benchmark -- 10000 1 16 64 256
```

CSV on stdout: physical processes, iterations, compile/start milliseconds,
end-to-end mean nanoseconds/event (with measurement collection overhead),
per-admission p50 and p95 nanoseconds. Capture **raw CSV**, runner architecture,
CPU model, OS, governor, Rust toolchain, Nix lock, git SHA, event counts,
whether the host was shared or isolated, and at least three repetitions.
Pinned process/CPU and memory/allocator instrumentation are a distinct
follow-up. If CI runs on a shared runner, the results are a noisy baseline
for *relative within-run scaling*, not a cross-machine speed claim.

The workload isolates hot-admission scaling with total process count; it
does not represent every graph (fanout, joins, requests, deferred jobs,
backpressure, cancellation or public captures). Expand scenarios after the
first baseline, never before without data.

## Evidence policy

New benchmark runs check out `github.event.pull_request.head.sha`
rather than the default synthetic PR merge commit. For previous runs
#43201132241 and #43201797923, the merge commits and actual benchmark
source head are recorded explicitly. This records source identity without
confusing an unrelated branch build with the measured candidate.

## Acceptance

1. `cargo test`/Clippy/format on exact final head, including benchmark smoke
   through example target where configured.
2. Actual bench run with environment-labelled raw numbers, preferably 3+
   repetitions and a N=1 vs N=256 scaling ratio; no invented samples.
3. If scaling is material, design a worklist-based refresh alternative and
   prove EOS, FIFO, wake, fairness and terminal correctness against the same
   deterministic regression corpus before seeking any H4 resolution.
4. Never equate one fast microbenchmark with full runtime or agent product
   performance; allocation, RSS and contention require separate instrumentation.
