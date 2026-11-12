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
