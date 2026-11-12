# H5 paired request-index benchmark — protocol and evidence

Date: 2026-11-12. Comparison is **not yet measured** in this document.

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

**No measured numbers until the workflow actually runs.**
