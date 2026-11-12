# H5 paired request-index benchmark — protocol and evidence

Date: 2026-11-12. **Measured paired comparison**: [successful exact-source benchmark #43321033238](https://github.com/matthis-k/phenix-ai/actions/runs/43321033238), candidate source `5b00589296482e7c36709386dd87b92b35b6b378`, pre-index baseline `9911b42e3c0a3d84c7c60658090a51d426190dd1`.

## Phase 0 — problem

H5's queue count index shipped in [PR #1276](https://github.com/matthis-k/phenix-ai/pull/1276)
at `0b3416e80915ea0af7294f5fe6898dc3ad13f135` with
[six-job CI #43315137691](https://github.com/matthis-k/phenix-ai/actions/runs/43315137691).
It removes repeated per-packet scanning in `request_live`, `request_work`
and upstream `request_input_closed`, but adds map maintenance to packet
publication and extraction. Static complexity and correctness tests do
not establish an actual net performance benefit.

## Phase 1 — alternatives and choice

| Option | Strength | Weakness | Decision |
| --- | --- | --- | --- |
| Compare unrelated previous CI timing | No new code | Different CPU/topologies; not causally attributable | Reject |
| Compare old/new on separate CI runners | Easy to orchestrate | Hardware drift can dominate | Reject |
| Microbenchmark internal `BTreeMap` versus `VecDeque` | Isolates data structure | Not public runtime closure behavior | Not primary |
| **Run the identical public-API scenario against both exact source trees on the same runner** | Paired attribution with common API, workload, toolchain and CPU | No CPU pinning; benchmark code copied to baseline; measured workload narrow | Choose |

The identical `fbp_request_benchmark.rs` file is compiled against:
- Pre-index baseline: `9911b42e3c0a3d84c7c60658090a51d426190dd1` (the exact #1275 head, before index changes).
- Indexed candidate: this PR's exact head (includes #1276 index and this benchmark).

Both checkouts are separate repositories on one GitHub-hosted
`ubuntu-24.04` runner. Copy **only** the new portable benchmark source
from head into the baseline example path; its runtime code stays exactly
at the pre-index SHA. Record both source SHAs and matching benchmark
file SHA256, toolchains, `uname`, CPU model and timestamps.

### Scenario

Build one persistent network with one open sink input of physical FIFO
capacity `N`. Begin an authenticated empty sealed request `needle`
and `R` other live request scopes. Inject `N+1` packets distributed
over those `R` requests without executing the sink. The first input
is admitted to a Ready process and **exactly N** subsequent packets
remain buffered in the physical queue.

Measure separately, with a warmup and without per-iteration clock calls:
- `fill_ns_per_packet`: admission/queueing, including BTreeMap updates in the indexed version;
- `status_absent_ns`: `request_status(needle)`, which must be Settled(0) and must not inspect foreign payloads;
- `work_absent_ns`: `request_work(needle)`, exactly zero queued packets;
- `work_present_ns`: `request_work(filler[0])`, one of the real buffer owners.

Run queue depths 8, 64, 256, 1024 with 1 and 16 filler scopes, 20,000
queries per case, 64 warmups and **three interleaved repetitions**
baseline/indexed, indexed/baseline, baseline/indexed. Validate exact
queue cardinality before and after all repeated read-only inspection.

Output raw CSV files and machine provenance as a workflow artifact,
plus all rows in job logs for transparent inspection.

## Phase 2 — adversarial critique

1. **Data source contamination:** runtime sources differ *only* by the intentional commit ancestry; the benchmark harness must have identical SHA256 bytes in both checkouts. Do not casually copy `Cargo.lock`, production source or Nix flake from head into baseline.
2. **Workload validity:** the first packet must reserve (not execute) the process and exactly `N` additional packets must remain in the bounded queue. The program asserts the sum of per-request `queued` snapshots equals N; if the scheduler changes this, the benchmark must fail.
3. **Compiler elimination:** every timed response is fed to `black_box`. Queries are immutable and independently timed over all operations, with no hidden consuming side effects.
4. **Performance tradeoff:** O(log R) membership does not mean constant cost at all queue sizes. Per-packet enqueue bookkeeping may regress small queues, and total `request_work` still scans graph processes/queues. Compare **both** reads and fills, not only the best-case result.
5. **Hardware:** three interleaved repetitions on one shared runner reduce drift but cannot promise pinned CPU frequency/governor, zero thermal throttling or resource isolation. Preserve raw observations and include runner CPU, SHA and toolchain; do not overclaim a universal speedup.
6. **No closure execution:** this benchmark measures public request status/work query paths (with real committed queued packets), **not** end-to-end optional-input closure walk cost. Use results to decide whether to implement a separate closure-heavy workload. H5 remains partial even after favorable results.
7. **Correctness:** no magic speed thresholds; exact-state assertions and regular CI verify semantics independently of benchmark timing.
8. **Branch provenance:** PR base is the verified indexed #1276 source. The paired performance baseline explicitly jumps back to pre-index #1275 and is not inferred from the PR's direct base branch.

## Phase 3 / Phase 4 acceptance

Implementation: `packages/phenix-graphs/examples/fbp_request_benchmark.rs`;
orchestration: `.github/workflows/fbp-h5-benchmark.yml`. No production
code changes and no new abstractions. Record successful source/candidate
benchmark workflow URL, environment, paired CSV values and whether a
small-buffer admission regression outweighs query improvements. Verify
regular six-job Flake CI on the exact final commit before accepting.

## Actual observations and Phase 4 analysis

The benchmark ran successfully on 2026-11-12 under
[Actions #43321033238](https://github.com/matthis-k/phenix-ai/actions/runs/43321033238).
The job checked out the exact candidate `5b00589296482e7c36709386dd87b92b35b6b378`
and exact baseline `9911b42e3c0a3d84c7c60658090a51d426190dd1`;
the SHA256 of the public-API benchmark source in **both trees** was
`efdb99bf0a8782da6271d0c9cb147a99a4cf724ac9f246f805281806636cca63`.
Hardware reported **AMD EPYC 9V74 80-Core Processor** (GitHub-hosted
Ubuntu 24.04, shared/non-pinned runner); Rust **1.96.1**. The workflow
uploaded environment metadata, source hashes and 3 repetitions of 8
scenarios for each implementation: **48 CSV observations**. No compared
runtime source was copied between checkouts.

Values below are the **median of the three per-repetition means**, in
nanoseconds/op. A speedup ratio is pre-index / indexed (greater than
one means the indexed version is faster). Each run used **20,000**
queries per scenario, after 64 warmups.

| Queued packets | Filler scopes | Fill before → indexed (ns/packet) | Absent status before → indexed (ns) | Absent work before → indexed (ns) | Present work before → indexed (ns) |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 8 | 1 | 1,595.7 → 1,731.6 | 31.3 → 19.7 | 39.5 → 24.1 | 45.5 → 30.7 |
| 8 | 16 | 977.0 → 988.1 | 34.3 → 23.1 | 44.9 → 30.1 | 50.5 → 35.5 |
| 64 | 1 | 452.4 → 436.8 | 90.2 → 17.9 | 109.0 → 25.1 | 114.1 → 30.3 |
| 64 | 16 | 458.5 → 465.5 | 93.4 → 25.9 | 113.6 → 32.8 | 120.2 → 37.7 |
| 256 | 1 | 383.5 → 376.8 | 508.1 → 18.4 | 585.8 → 24.2 | 590.5 → 30.9 |
| 256 | 16 | 338.0 → 320.7 | 485.2 → 25.0 | 582.6 → 32.8 | 590.4 → 37.8 |
| 1,024 | 1 | 393.2 → 360.1 | 2,091.1 → 18.3 | 2,340.4 → 24.1 | 2,322.8 → 30.4 |
| 1,024 | 16 | 273.1 → 266.7 | 2,089.5 → 25.4 | 2,352.3 → 31.8 | 2,317.0 → 37.9 |

**Observed scoped benefit:** at depth 1,024, the request-status query
was approximately **114× faster** with one filler scope or **82×**
with sixteen; absent-work queries were **97×** and **74×** faster;
present-work queries were **76×** and **61×** faster respectively.
At depth 8, read-side improvements were only around **1.4–1.6×**.
The result supports the original H5 membership-index rationale for
deep buffered queues without claiming whole-graph closure speedup.

**Tradeoff and risk:** the index inserts/removes map counts during packet
admission/disposal. At depth 8 with one filler scope the median fill
measurement worsened from 1,595.7 to 1,731.6 ns/packet (about 8.5%),
while at larger depths the paired medians were similar or marginally
lower; they do not establish a robust fill throughput improvement.
Fill only admits N+1 packets per case, so cold-start and runner jitter
can materially affect this column. Do not present this as an allocator
budget or sustained throughput measurement.

**Unmeasured:** `request_input_closed` still walks upstream producers,
`refresh()` still scans the process inventory, and these runs did not
execute a closure-heavy graph, inspect RSS/heap allocations, simulate
deferred jobs or use pinned CPU cores. Benchmark correctness asserts
that request-scoped work counts are unchanged by read-only inspections.
The existing Flake CI provides integration correctness; benchmark
numbers alone are not a safety proof.

**Disposition:** H5 remains **Partial**, but now has measured evidence
for its request-membership subproblem. A closure-heavy paired workload
and an H4/H5 mixed-activity benchmark are the appropriate next evidence
before another runtime scheduling abstraction. The graph's core
runtime/API is unchanged by this benchmark PR.

