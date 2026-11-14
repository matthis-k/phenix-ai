# H4 EOS endpoint-driven completion — design revision

## Phase 0 — baseline, problem and constraints

Parent: targeted wake PR #1287 at `5b6c06aecede2272015f2ae087269c5ee5eccdc9` (separate verification pending). Verified common baseline is #1286 `b4c8c55cefa52f4fc0489db6fb2c6102d540d201`. The existing `finish(index)` closes outgoing writers and incoming readers. These exact queue IDs identify all processes whose scheduling conditions changed. But `Step::Finished` unconditionally sets `force_full_refresh`, and auto-finish in `refresh_process` forces `refresh_all_processes` repeatedly after a single process finishes. Consequently EOS propagation in sparse or long chains can entail graph-wide repeated scans rather than local event propagation.

Required semantics: atomic packet/publication ownership; no synthetic EOS before `ConnectionQueue::is_eos()`; no abandoned accepted input; preserved `Handoff` finish transition; retain deterministic ascending-order first pass and FIFO ready queue; upstream publishers with blocked fanout must wake after reader disposal; capture EOS remains visible. Preserve unaddressed notifier, recovery, failure and terminal full-scan exceptions.

## Phase 1 — options

| Option | Benefit | Risk |
| --- | --- | --- |
| Baseline `force_full_refresh` | Proven global convergence | O(V) on every completion |
| Only dirty direct EOS readers | Cheap | Can miss upstream pending publishers after `close_reader` |
| Mark both endpoints of every writer-close and reader-close + incremental finish passes | Local propagation, reuses existing dirty index | Must handle in-scan events and cyclic closure without infinite loops |
| Dedicated EOS dependency graph / reachability cache | May reduce indices further | New abstraction and stale-state risk |

Choose direct queue endpoint invalidations and reuse existing `dirty_queue` and `refresh_process` machinery. Add no new public API or state.

## Intended implementation

1. In `finish(index)`, after checking for unconsumed input, collect that process's output writer queues and input reader queues, perform the *original* close operations, and call `dirty_queue(queue)` for each touched queue. This marks **both** producer and consumer process endpoints; capture and ingress endpoints remain passive.
2. In the `Step::Finished` completion path, remove the forced global scan: local queue-event invalidation already covers EOS, reader disposal and pending publication retry.
3. When an idle process finishes because its EOS inputs were all delivered during `refresh_process`, do **not** replace the rest of refresh with a full scan. Instead, repeat an ascending union of **newly dirty endpoints** and **always-polled special processes**, and repeat only when another process actually finishes (mirroring the former finish-only fixed point). A dirty event created after a process's turn is retained for the next driver call if no process finishes, matching the baseline one-pass behavior.
4. The global full-scan path retains its exact old repeat-until-no-finish logic, including cases of `prefer_full_refresh`, global notifier, recovery or initial startup. No second execution body; the same `refresh_process` applies to both.
5. If a process terminates the session or fails, terminate the pass immediately. Do not re-run after terminal state. Marked dirty nodes are merely scheduler invalidations, not new tickets or lifecycle privileges.

## Expected complexity and evidence

Sparse finish: O(number of queues closed × log V) to index affected peers rather than O(V) full inventory. EOS chain: O(number of actually affected endpoints + special poll overhead) rather than O(V²) full scans. Worst-case all-special/fully connected network still O(V) per pass. No unjustified constant-time guarantee.

Add new targeted test: a 32+ node graph with one closed branch, unrelated open sinks, bounded upstream publisher and lossless capture; prove no abandoned packet, no missing EOS, no duplicate activation. Existing staged waits, all-optional cycles, request-scoped EOS, fanout backpressure and cancellation must remain green. Add paired EOS-specific microbenchmark comparing the same source example against exact parent and H4/H5 regressions; use separate draft PR so wake and EOS changes can be accepted independently.

## Protocol default accepted limitations

`state()`, `request_status()`, `close_input()` and special-poll paths may still perform O(V). EOS closure across an all-special graph legitimately requires visiting each process. No global fairness or cancellation of external effects is introduced. No limitation contrary to request provenance or transport correctness may be accepted without review.
