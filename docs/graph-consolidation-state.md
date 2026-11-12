# Batch B Scheduling State

Implementation evidence for Batch B only, against the Phase 2 revisions in
`graph-consolidation-design.md` and conditions in `graph-consolidation-critique.md`.
Recorded 2026-11-10 in the shared, uncommitted workspace. Baseline HEAD is
`d758291c1db77abcff60ac3ac0e167c0f59261bd`.

## Scope And Ledger

Changed files owned by this batch:

- `packages/phenix-graphs/src/lib.rs`: adjacency/compiler areas only, 3 added and
  6 removed lines. No action-constructor/lowering edits by this batch.
- `packages/phenix-graphs/src/scheduler.rs`: 1 added and 56 removed lines.
- `packages/phenix-graphs/src/tests.rs`: 378 added and 39 removed lines, including
  test-only diagnostics, independent transition checks and four new tests.
- This new evidence document.

Removed four stored collections: `CompiledGraph::by_node` and
`WaveActivity::{counts,nodes,active_partitions}`. Removed `ActivityCounts`, the
old summary-backed `state` method, `refresh`, four calls to `refresh`, and the
four occupied/emptied/admitted/finished count updates. No replacement production
cache or diagnostic scan was introduced.

All removed adjacency uses are accounted for: graph initialization, edge insertion,
field declaration, partition-index compilation, structural reverse-index
compilation, topological ordering, and the test's forward reachability reference.
The three production traversals now read `by_output`; the reference traverses
`by_output[OutputAddr { node, slot: 0 }]` when present.

Deployment resolution, scheduler edge-adapter registration, publication, and
manifest edge lookup already use `by_output`; inspection required no changes.
The incoming node/port indexes remain deliberately distinct reverse lookup indexes.
Admission, selected/retired buffer removal, policy interfaces, transaction phases,
and all action lowering are outside this batch.

## Field Ownership

| Field / Fact | Owner And Update Boundary | Derivation / Tradeoff |
|---|---|---|
| `by_output` | Compiled graph; validated edges appended during build | Sole outgoing adjacency, unchanged order and edge IDs; address key preserves invalid-slot rejection. |
| Removed `by_node` | Previously compiled graph; duplicated every inserted outgoing edge | Traversals reuse `by_output`. Canonical slot zero makes source-key ordering equivalent; no rebuilt cache. |
| `buffers` | Wave state; delivery preflight/commit and reservation consumption/retirement | Authoritative delivery queues; empty queues can remain. Diagnostics inspect nonempty queues, never map-entry or delivery counts. |
| Removed `counts.buffered_slots` | Previously incremented/decremented on occupied/empty slot transitions | Pending diagnostic state scans nonempty buffers; test oracle independently groups occupied slots by port. No production aggregate maintenance. |
| Removed `counts.admitted` | Previously incremented on admission, decremented on completion | Diagnostic executing state scans admitted producers, including queued undispatched work. Producer presence takes precedence over buffered inputs. |
| Removed `nodes` | Previously maintained by `refresh` | Test-only active-node union of nonempty buffer owners and producer nodes. Historical reservations and settled outcomes are not activity. |
| Removed `active_partitions` | Previously updated on active-node transitions | Test-only grouping of distinct active nodes by structural partition, not invocation count. Scans/temporary allocations are paid only in tests. |
| `pending_ports` | Wave activity; occupied/empty slot transitions | Retained accelerator counting nonempty slots per port, not deliveries. Oracle reconstructs it from actual queues. |
| `pending_by_partition` | Wave activity; first/last occupied slot for a port | Retained completion-targeting accelerator. Oracle reconstructs it directly from buffers, not from `pending_ports`. |
| `producers` | Wave activity; source/port admission through completion | Retained open-color authority for both ready and dispatched work. Oracle compares full partition/ID/node/cohort membership with ready jobs plus tickets plus explicitly held manual work. |
| `dispatched` | Wave activity; `next_ready` issues, completion removes | Retained completion authority, not an admission summary. Duplicate/foreign tickets remain rejected; cancellation invalidates tickets. |
| `ready` | Wave state; admission enqueues, dispatch dequeues | Independent work owner used by the oracle, including producer activity before tickets exist. |
| `reservations` | Wave state; committed policy selections | Retained staged history, including future obligations after all inputs are consumed. Cannot derive from live buffers/tickets. |
| `rejected_cohorts` | Wave state; malformed selection/preparation rejection | Retained cohort exclusion, independent of activity summaries; no update changes. |
| `failed_ports` | Wave state; terminal port failure | Retained failure suppression, not inferred from buffers; no update changes. |
| `settled_colors` | Wave state; completion records actual delivered targets or failure | Retained terminal policy history, with exact node-to-port reachability and cohort semantics unchanged. |
| Structural partitions / reachability caches | Compiled structural index | Same reverse reachability algorithm and quotient filtering; only outgoing adjacency input changes. Cycles and fresh-cohort paths remain expressible. |

## Actual Diff Review

Performed a separate self-review of the actual `git diff` after implementation,
not just the design. This is not an independent implementation reviewer signoff.

- Confirmed all Batch B `lib.rs` hunks are compiler/adjacency hunks. Concurrent
  Batch A action changes and unrelated authoring changes were left untouched.
- Confirmed the scheduler diff deletes only unused summaries and their maintenance.
  The remaining pending-index and producer operations keep their previous order;
  no completion, reserve, decode, cohort, ticket or publication phase was moved.
- Audited canonical-slot validation during graph build and output lookup, plus
  address-keyed adapter registration and deployment resolution. Slot 1 does not
  alias slot 0.
- Confirmed topological ordering still seeds every declared node, deduplicates node
  successors and appends cyclic leftovers stably. Sparse and zero-outdegree nodes
  do not require adjacency entries.
- Preserved each existing diagnostic assertion's expected state/cardinality.
  Replacements derive from buffers/producers, not the pending accelerators.
  The manual capture harness explicitly supplies popped in-flight work to the
  independent oracle; it does not invent production tickets.
- The independent oracle checks exact pending-port counts, partition membership,
  producer membership and uniqueness at stable boundaries and manual publication
  boundaries. It does not reconstruct expected work from producer indexes.
- Kept the independent forward cyclic reachability reference and existing
  malformed pick/retirement, decoder rejection, reservation-only staged,
  routed-away, partial fanout, independent-wave and cycle assertions intact.
- No defects found in this self-review. Independent implementation review and
  project-required remote checks remain delivery gates, not claimed results here.

## Regression Evidence

Final code verification commands:

```sh
nix develop -c cargo test -p phenix-graph --lib --locked
nix develop -c rustfmt --edition 2024 --config skip_children=true --check packages/phenix-graphs/src/lib.rs packages/phenix-graphs/src/scheduler.rs packages/phenix-graphs/src/tests.rs
git diff --check
```

Results: **198 passed, 0 failed, 0 ignored**; formatting and whitespace checks
passed. Final test command reported 15.06 seconds build time and 0.85 seconds
test execution. These are observations on a shared incremental build, not a
baseline-relative performance result. An earlier run before the differential
test passed 197 tests. No build-lock conflict occurred in these runs.

Four new tests cover:

- Independent index/work oracle through queued, dispatched, completed, duplicate
  completion, foreign-wave completion, cancellation and independent-wave states.
  Multiple invocations of one node still count as one active node, and retained
  empty queues do not count as pending slots.
- Sparse ordering, duplicate node successors on different input slots, stable
  cyclic leftovers, zero outdegree, edge order/ID distinction and invalid output
  address rejection at output lookup and adapter registration.
- Targeted versus conservative reevaluation: 12 cases (three graph shapes times
  four input sets), covering delayed branches, skip/failure, fresh cohorts, mixed
  cohorts and feedback. Both drivers run the independent oracle and compare
  existing normalized report semantics, remaining buffer values and diagnostics.
- Storage-size reporting (one test).

The existing randomized reverse-cache/forward-reference test still compares all
source/target-port relations in 24 cyclic multiport graphs. The library command
does not run integration tests, doctests, full-workspace checks or remote CI.
Concurrent agents' source changes were present during verification; these results
describe that shared working-tree snapshot, not an isolated Batch B commit.

## Measurements

Command:

```sh
nix develop -c cargo test -p phenix-graph --lib --locked consolidation_storage_measurements -- --nocapture
```

Observed on this x86_64 development target:

| Measurement | Bytes |
|---|---:|
| Current `CompiledGraph` inline size | 352 |
| Current `WaveActivity` inline size | 96 |
| Deleted adjacency map header | 24 |
| Deleted three activity collection headers together | 72 |
| `Edge` | 12 |
| Deleted per-node counts value (`usize`, `usize`) | 16 |

These are `size_of` measurements, not allocator profiling. Removing the duplicate
adjacency also eliminates one stored edge copy per logical edge, one vector per
source with outgoing edges and its B-tree entries. The five-node/four-edge topology
probe has three outgoing source keys: its deleted live edge payload alone is
4 * 12 = 48 bytes, excluding spare vector capacity and map allocation overhead.
The three activity trees and their entries are gone entirely; unlike the old
counts tree, no diagnostic node-count entries persist after activity ends.

No runtime speedup, allocator-byte total, binary-size improvement or compile-time
improvement is claimed. Diagnostic scans are linear in buffers/producers plus
temporary set/map construction, deliberately outside production scheduling paths.
Further admission/buffer-removal consolidation remains unapproved and was not done.
