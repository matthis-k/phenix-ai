# H4: contiguous refresh candidate union — measured experiment

## Phase 0 — constraint and baseline

The H4 hybrid scheduler uses three sets: dirty process indices, always-polled indices, and sealed-request readiness watchers. The current refresh path **moves the dirty BTreeSet and inserts every always/scoped candidate into it on every drive**. Even with no change in the permanent watchers, this creates and later frees B-tree nodes, and performs O(log K) insertions. This is separate from unavoidable legacy Custom polling and closure proof costs.

Requirements: preserve ascending compiled-index visit order, deduplicate overlap, keep newly dirtied processes for the *next* driver pass unless the current pass finishes a node, retain generic full-refresh fallback, and never silently skip request-local closure or externally notified readiness. No new public types, scheduler queue, policy authority or closure assumptions. A session-owned scratch buffer may be retained if measured CPU wins justify its high-water memory cost.

## Phase 1 — alternatives and choice

| Option | Strength | Weakness |
| --- | --- | --- |
| No change | Proven, simple | Reallocates tree entries for permanent readers each refresh |
| **Reused contiguous vector, sorted and deduplicated** | One initial allocation, contiguous iteration and deterministic order | Retains high-water O(K) memory per session; sort remains |
| Fresh vector per refresh | No session field; simple ownership | Extra allocation on every tiny-path refresh |

Choose the reused vector **only as a benchmark-gated experiment**. A small internal `collect_refresh_candidates(&mut Vec<_>)` drains the existing dirty set, extends with always/scoped indices, sorts and deduplicates once. The session moves its scratch buffer out for the duration of `refresh()`, then restores it after the driver pass; early terminal returns may drop the buffer without changing semantics. A full refresh still takes the same fallback path; no policy eligibility or scheduling admission rules change.

## Phase 2 — adversarial critique

1. **Ordering/duplicates:** BTreeSet union visited ascending indices. `sort_unstable` followed by `dedup` is equivalent for plain unique process IDs, even when dirty and both watcher sets overlap.
2. **Events during a refresh:** `mem::take(dirty_processes)` still drains the old dirty set before processing. Any new queue or notification changes generated inside `refresh_process` go to the new set and are processed only on finish repeat (or the next drive), as before.
3. **Fallback:** threshold-based full scans and generic notifier snapshots run before building the vector. The vector cannot convert a full-scan into a partial scan.
4. **Request closure and staged inputs:** `always_poll` and `scoped_readiness_poll` are read on each candidate collection, including repeat-on-finish. No watcher scope or lifetime changes.
5. **Allocation and cost:** the scratch vector reserves the full *upper-bound* size even when indices overlap, and **retains that high-water capacity** for the session lifetime. Steady-state reuse avoids a repeated vector allocation; sorting remains O(K log K). If many sessions observe brief large bursts, retained memory can outweigh CPU wins.
6. **Concurrency/reentrancy:** the session driver owns mutable refresh; external notifiers publish only to the independently synchronized wake signal. The scratch is moved out of the session while active, so no borrowed buffer is exposed to plugin code. An exceptional terminal return can drop it safely; no notifier linearization changes.
7. **Failure:** allocation failure behavior is no stronger than existing collection allocations. Ordinary policy exceptions and terminal propagation retain the same code paths.

The protocol default cannot accept a measurable negative across primary mixed-agent, EOS and H5 workloads simply for elegance. If the paired comparison shows a credible regression without a compensating primary workload benefit, revert this experiment.

## Phase 3/4 — implementation and acceptance

Code: `packages/phenix-graphs/src/runtime/mod.rs`. Benchmark: `.github/workflows/fbp-h4-refresh-candidates-benchmark.yml`.

Acceptance evidence:
- exact final-head Flake CI with full graph/native test shards and Clippy/format;
- same-source paired release comparison against exact parent `b584e22656418c6ef61210d42000b6f68cb349ed` on one pinned allowed CPU, 3 interleaved rounds;
- both legacy-polled and notified modes of the persistent 11-step agent workload with 0/16/64/128 unrelated services; EOS and scoped-closure controls;
- raw per-run CSV, source checksums and process peak RSS; avoid extrapolation to real model/SSH I/O and do not claim allocation-event measurement from RSS.

### First paired trial — fresh vector (rejected iteration)

[Exact-head paired benchmark #43397474157](https://github.com/matthis-k/phenix-ai/actions/runs/43397474157) passed with three interleaved pinned-CPU repetitions and byte-identical benchmark sources. Medians of per-run mean event timings showed **17.7% less time for legacy-polled agent/128 idle services** and **14.5% less for legacy-polled/64**, but **2.3% slower for polled/0**, 1.5–3.5% slower for notified/0–128, **5.7% slower for EOS chain/128**, and **6.5% slower for H5 fan-in/64**. This **fails** the small/EOS/closure regression acceptance gate despite the polled-branch benefit.

Phase 1 revised: retain a single per-session vector scratch buffer, restoring it after each successful refresh rather than allocating a fresh vector per drive. Existing dirty trees and all other scheduler structures remain unchanged. Re-check the extra per-session high-water memory cost and all performance evidence; the initial trial's measurements do **not** validate the revised code.

**Disposition: experimental revision, not accepted.** Do not update issue status or merge without reviewing the *new* exact-head CI and paired performance evidence.

### Seven-run revised vector evidence and Phase 1/2 correction

[Seven-repetition pinned paired benchmark #43398635142](https://github.com/matthis-k/phenix-ai/actions/runs/43398635142) used byte-identical sources and interleaved baseline/candidate modes. Median of seven per-repetition means: legacy-polled 128 unrelated services **14.4% faster**, /64 **11.7% faster**, notified/128 **0.6% faster**. However EOS chain/128 was **5.3% slower** and H5 closure fan-in/128 **6.7% slower**. The H5 optional closure chain/128 stabilized at **0.5% faster**. Reusing the vector alone therefore **fails** the regression gate.

**New Phase 1 candidate: bounded adaptive selection.** Keep the original BTreeSet-union code for fewer than **8** always/scoped poll candidates; use the reused sorted vector only at or above that number. In the persistent mixed benchmark, 16 unrelated services imply approximately 8 permanently polled Custom policies plus a tool gate, where the prior trial already measured a 7.1% improvement. EOS propagation and H5 fan-in are sparse-poll workloads and should remain on the original branch. Do not add a second scheduler algorithm or any public policy mode: only the internal *representation of the same ascending candidate union* switches.

**Adversarial critique:** the two collections must be mutually exclusive per batch, so their chained iterator retains ascending order. Switching representations after a process finishes must rebuild from the new dirty set and current scope watchers, not reuse the old candidate list. The BTreeSet small branch must be byte-for-byte equivalent in membership to the pre-experiment scheduler. The vector remains private session scratch, empty on the small branch, and can retain prior high-water capacity; the threshold is a measured heuristic rather than a correctness boundary. A high count of overlapping watchers might choose the vector unnecessarily, but does not omit any candidate. Re-run all tests, the exact-parent mixed-agent/EOS/closure benchmark and RSS; reject if sparse regressions persist or the new branch fails semantic tests. The first seven-run measurement **does not validate** this adaptive revision.

**Disposition: conditional-adaptive experiment pending exact-head proof, not accepted.**
