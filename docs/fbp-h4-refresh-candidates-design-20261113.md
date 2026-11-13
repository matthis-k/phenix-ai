# H4: contiguous refresh candidate union — measured experiment

## Phase 0 — constraint and baseline

The H4 hybrid scheduler uses three sets: dirty process indices, always-polled indices, and sealed-request readiness watchers. The current refresh path **moves the dirty BTreeSet and inserts every always/scoped candidate into it on every drive**. Even with no change in the permanent watchers, this creates and later frees B-tree nodes, and performs O(log K) insertions. This is separate from unavoidable legacy Custom polling and closure proof costs.

Requirements: preserve ascending compiled-index visit order, deduplicate overlap, keep newly dirtied processes for the *next* driver pass unless the current pass finishes a node, retain generic full-refresh fallback, and never silently skip request-local closure or externally notified readiness. No new public types, persistent cache, scheduler queue or authority.

## Phase 1 — alternatives and choice

| Option | Strength | Weakness |
| --- | --- | --- |
| No change | Proven, simple | Reallocates tree entries for permanent readers each refresh |
| Cache a persistent scratch bitset/vector | Can avoid allocation in steady state | Adds state and teardown/invalidation obligations |
| **One contiguous vector, sorted and deduplicated** | One batch allocation; contiguous iteration; same deterministic index order | Sort cost, possible tiny-graph regression; worst-case transient space O(K) |

Choose the third **only as a benchmark-gated experiment**. A small internal `collect_refresh_candidates()` drains the existing dirty set, extends with always/scoped indices, sorts and deduplicates once. It runs on initial mixed refresh and repeat-after-finish. A full refresh still takes the same fallback path; no policy eligibility or scheduling admission rules change.

## Phase 2 — adversarial critique

1. **Ordering/duplicates:** BTreeSet union visited ascending indices. `sort_unstable` followed by `dedup` is equivalent for plain unique process IDs, even when dirty and both watcher sets overlap.
2. **Events during a refresh:** `mem::take(dirty_processes)` still drains the old dirty set before processing. Any new queue or notification changes generated inside `refresh_process` go to the new set and are processed only on finish repeat (or the next drive), as before.
3. **Fallback:** threshold-based full scans and generic notifier snapshots run before building the vector. The vector cannot convert a full-scan into a partial scan.
4. **Request closure and staged inputs:** `always_poll` and `scoped_readiness_poll` are read on each candidate collection, including repeat-on-finish. No watcher scope or lifetime changes.
5. **Allocation and cost:** vector reserves the full *upper-bound* size, even when entries overlap; if many indices overlap, temporary memory grows. Sorting has O(K log K) comparisons and may regress very small K; this is not assumed faster without measurements.
6. **Concurrency:** the session driver owns mutable refresh; external notifiers publish only to the independently synchronized wake signal. The vector is local and does not alter notifier linearization.
7. **Failure:** allocation failure behavior is no stronger than existing collection allocations. Ordinary policy exceptions and terminal propagation retain the same code paths.

The protocol default cannot accept a measurable negative across primary mixed-agent, EOS and H5 workloads simply for elegance. If the paired comparison shows a credible regression without a compensating primary workload benefit, revert this experiment.

## Phase 3/4 — implementation and acceptance

Code: `packages/phenix-graphs/src/runtime/mod.rs`. Benchmark: `.github/workflows/fbp-h4-refresh-candidates-benchmark.yml`.

Acceptance evidence:
- exact final-head Flake CI with full graph/native test shards and Clippy/format;
- same-source paired release comparison against exact parent `b584e22656418c6ef61210d42000b6f68cb349ed` on one pinned allowed CPU, 3 interleaved rounds;
- both legacy-polled and notified modes of the persistent 11-step agent workload with 0/16/64/128 unrelated services; EOS and scoped-closure controls;
- raw per-run CSV, source checksums and process peak RSS; avoid extrapolation to real model/SSH I/O and do not claim allocation-event measurement from RSS.

**Disposition: experimental, not accepted.** Do not update issue status or merge without reviewing final-head test and paired performance evidence.
