# H5/H6 bounded lane-local closure traversal

Base: local `fix/fbp-staged-poll-source-20261123`, `3bdccbe9b`.
Worktree: `/tmp/opencode/fbp-h5-lane-closure-20261123`.

Current lane benchmark command (after the CI harness separation below):

```sh
nix develop --command cargo run --locked -j 2 --release -p phenix-graph --example fbp_lane_closure_benchmark -- 21 16
```

The original combined-harness measurements remain archived below. The extracted
harness rerun in the final section supersedes them for the current executable.

## Phase 1: premise and correctness challenge

`scoped_closure_proof` walks canonical plus every allocated request lane at
each upstream input. `prepare_request_lane` already builds an immutable,
canonical-order `input_ports[(process, port)]` index for each lane. Request
admission, publication and packet selection use that lane exclusively.
Recycled lanes retain the topology, not request authority; request IDs never
recycle. Use the existing index, not another cache or mutable summary.

Keep `input_closure_proof`'s global EOS check first, even for unsealed requests.
An empty own lane is not global EOS: canonical and foreign queues can remain
open or buffered. Global EOS can still scan all physical queues; this change
does not claim to remove that cost. Unscoped proofs and SharedFifo stay intact.

Packet checks on the queried input precede traversal. Within each producer,
owner precedes closure obligation, which precedes its input packets. Preserve
port order and own-lane queue order and the existing LIFO DFS, relevance filter,
cycle/diamond membership and sparse/whole-graph reachability behavior. Missing
input-port entries mean no physical edges, as in `packet_for`; a missing admitted
lane itself is an invariant violation, not an excuse to prove absence.

Challenge before implementation: could deleting duplicate canonical/foreign
producer pushes change the first witness among multiple blockers? Check exact
proof/witness equality against both the original traversal and an independently
projected own-lane reference, not just closed/open equality. RequestPacket must
identify the actual own queue and exact count. Do not silently normalize away
different process or queue witnesses in the oracle.

## Phase 2: alternatives and unchanged baseline

Reject filtering `queue_lanes` while walking all physical queues: it retains the
lane-count traversal cost. Reject a new persistent cache, topology index, or
closure summary: invalidation and state are unnecessary. Choose one local
queue-slice selector reused for queried and upstream ports, borrowing the
existing lane map. No generic helper/API or transport/shared-FIFO changes.

Before the production edit, extend the existing closure example with a separate
lane-matrix mode. Use identical harness source in both builds, 1/4/16/64 live
overlapping sealed scopes, persistent reused lanes, Optional closure chains,
and SharedFifo controls including one lane. Exclude compile/start, admission,
sealing, validation and release from the timed execution interval. Record each
raw batch sample and process maximum RSS, without a fabricated acceptance
budget. Retain the old example's existing modes. No workflow edits/publication.

## Phase 3: bounded implementation and adversarial tests

Test-only frozen baseline traversal is the oracle, not another production
algorithm. Exhaust meaningful small topology/reachability/sealed/packet/owner/
obligation combinations, including diamonds, cycles, multiple input ports,
irrelevant roots, cancelled/reused lane slots and empty mappings. Compare exact
own-lane deterministic witnesses and exact original traversal proofs.
Public regressions must prevent downstream false absence while an upstream
packet, owner, or all-optional/custom closure obligation still exists, under
both queue policies. Exercise global EOS precedence and unscoped preservation.

## Phase 4: paired evidence and verification

Alternate unchanged-baseline and candidate binaries with identical harness,
arguments and environment. Report raw samples, ratios and limitations, not a
performance acceptance claim. Profile/characterize remaining full EOS scan.
Run Nix toolchain with two build jobs: targeted tests, full graph default and
no-default tests, formatting and all-target Clippy. Leave changes uncommitted
for independent review and publication. If the premise is unsafe or measured
benefit absent, stop rather than add speculative machinery.

## Results: implementation and correctness

The production change is confined to `runtime/closure.rs`: one local slice
selector, concretely reused for the queried port and each upstream input.
BoundedLanes borrows `request_lanes[scope.lane].input_ports`; SharedFifo borrows
the original input queues. No persistent state, new allocation, helper API,
transport change or workflow change. Foreign queues are not enumerated by the
scoped DFS, not merely excluded from `request_count`.

The witness-order challenge did not require a semantic relaxation. Each physical
input contains identical canonical-order edge blocks, followed by identical
lane copies. The final block already determines the LIFO producer order; earlier
duplicates are redundant under exact visited membership. Own request packets
can occur only in its block. The tests require exact original proof equality
including process/queue identity and count, for all tested multiple-blocker
states as well as single-blocker states. No differing incidental witness was
observed or excused.

`runtime/closure/lane_tests.rs` contains the test-only original traversal (with
independent BTreeSet visited membership), plus a projection implemented by
filtering the physical inventory rather than using the production map. The
exhaustive matrix covers all 64 directed three-node loop-free topologies,
all eight root subsets, sealed/unsealed, and all 64 combinations of clear,
owner, two-packet and Optional-obligation state across those three producers,
under both queue policies. Each state queries all four processes' three ports,
scoped and unscoped: over three million exact comparisons with each oracle.
An unrelated fourth node contains foreign and unscoped data; the lowest lane
is cancelled/released/reused while three foreign lanes stay live. Additional
tests cover diamonds with feedback/self-loops and all owner/obligation masks,
every internal/ingress own edge (including a nonzero lane) with exact two-packet
witnesses, empty input-port mappings, whole-graph relevance, and global EOS
precedence with late open/buffered foreign queues at 1/4/16/64 lanes.

`tests/fbp_lane_closure.rs` uses public runtime APIs only. Four overlapping scopes
share a root -> middle -> sink chain. Root waits for a host wake; other root
packets remain queued. A packet-bearing queued scope is cancelled and its lane
reused while the first owner remains waiting. Holding a zero-packet middle job
outside the runtime cannot let sink skip either that owner or foreign-owned
scopes' unobserved middle closure obligations. All scopes then settle exactly
once. The regression runs SharedFifo/BoundedLanes crossed with Optional, polling
Custom and notification-driven Custom readiness.

## Archived: combined-harness measurements

These measurements and hashes describe the original combined executable, not
the current benchmark source. Its `--lane-matrix` mode has moved to
`fbp_lane_closure_benchmark`. Retain this evidence unchanged for provenance;
use the extracted-harness rerun below for current commands, hashes and results.

Local machine: Intel i7-8550U, Linux x86_64, Nix devShell Cargo 1.96.2, default
release profile, `--locked -j 2`. No extra RUSTFLAGS. Both binaries were built
in the same worktree with the same harness source. The baseline binary was
built and measured **before** editing production closure code. The later
production edit was not applied to/rebuilt in its separate target directory.

- Harness SHA-256: `eea11a4205bf8f9abaac4c14ba79822e282557f903ae0e794091bc13342dbaca`.
- Baseline binary SHA-256: `cc5b3ee6fbc6c371c87b85216434c7499486d35b0114710fde4478fe82bf2b33`.
- Candidate binary SHA-256: `5afc70fc23fc711ed8d71bbe5ff12995cb164b31a6b45832962b216e0a1c5fa8`.
- Baseline target: `/tmp/opencode/fbp-h5-lane-baseline-target`.
- Candidate target: `/tmp/opencode/fbp-h5-lane-candidate-release-target`.
- Initial unchanged raw run: `/tmp/opencode/fbp-h5-lane-baseline-first.txt`.
- Paired raw samples and per-process RSS: [samples](fbp-h5-lane-closure-20261123-samples.txt).

Run each binary as `taskset -c 2 <binary> --lane-matrix 21 16` under GNU
`time -f 'RSS_KiB=%M user_s=%U system_s=%S elapsed_s=%e'`. Five rounds use
baseline/candidate order for odd rounds and candidate/baseline for even rounds.
The runner is `/tmp/opencode/fbp-h5-lane-measure.sh`; analysis is
`/tmp/opencode/fbp-h5-lane-summary.py`. No tests/builds ran concurrently with
the paired measurement. CPU frequency, thermal state and other host work were
not controlled. Two warmup batches per case allocate all lanes; subsequent
samples reuse those same physical queues. All live scopes overlap the same
16-node Optional closure chain. Timer brackets only `run_until_idle`; compile,
session creation, admission, sealing, validation, release and output are outside.
All samples check exact steps, hit counts, settlement and quiescence.

Pooled medians below use 105 raw batch durations divided by live request count.
Ratios are candidate round-median / matching baseline round-median; they are
not confidence intervals or a performance acceptance threshold.

| Policy | Live Lanes | Baseline ns/request | Candidate ns/request | Paired Round Ratios (1..5) |
| --- | ---: | ---: | ---: | --- |
| SharedFifo | 1 | 242970 | 156873 | 0.469, 0.513, 1.895, 0.993, 1.005 |
| SharedFifo | 4 | 440377 | 351006 | 0.714, 0.608, 0.746, 0.603, 1.591 |
| SharedFifo | 16 | 862104 | 780239 | 0.826, 0.752, 0.933, 0.950, 0.882 |
| SharedFifo | 64 | 1457557 | 1350679 | 0.970, 0.889, 1.058, 0.948, 0.879 |
| BoundedLanes | 1 | 177332 | 168251 | 1.081, 0.951, 1.043, 0.970, 0.864 |
| BoundedLanes | 4 | 568363 | 472550 | 0.826, 0.541, 0.799, 1.157, 0.985 |
| BoundedLanes | 16 | 1768773 | 1113093 | 0.699, 0.590, 0.715, 0.632, 0.569 |
| BoundedLanes | 64 | 4131788 | 1948277 | 0.489, 0.480, 0.450, 0.492, 0.483 |

The high-lane improvement repeats in every pair and exceeds the corresponding
SharedFifo control movement. One-lane and four-lane/control noise precludes a
small-case speedup claim. The SharedFifo algorithm is unchanged, but compiler
layout can move and the host is noisy; do not attribute its timing differences
to a new algorithm. This is a bounded execution-only workload, not an agent-wide
throughput claim or a proof that all H5/H6 costs are resolved.

Maximum process RSS in KiB, rounds 1..5: baseline `[4296, 4196, 4244, 4140, 4232]`,
candidate `[4140, 4300, 4224, 4240, 4236]`. This includes setup and allocator
retention for all cases, not per-proof live memory. There is no allocation-count
claim. The production change introduces no persistent storage or new allocation.

## Results: remaining global EOS cost

The normal lane-matrix leaves canonical writers open, so the global EOS
precheck usually exits on its first queue. It does **not** measure the worst
case where all canonical/foreign queues must be checked. The ignored test
`profile_global_eos_scan` isolates this unchanged path with one input edge,
all writers closed, all queues empty, unsealed scopes, and seven samples of
one million immutable proofs. Setup is outside the timer; black_box prevents
proof hoisting. Run with:

```sh
nix develop --command cargo test --locked -j 2 --release -p phenix-graph --lib profile_global_eos_scan -- --ignored --nocapture
```

The recorded executable was pinned to CPU 2. [Raw EOS samples](fbp-h5-lane-closure-20261123-eos-profile.txt)
give medians of 13.24 / 16.08 / 32.35 / 76.22 ns per proof for 1/4/16/64
lanes, inspecting respectively 2/5/17/65 physical queues. This is a candidate
cost profile, not a paired speedup claim; the global EOS algorithm is identical
to baseline. Retaining this scan is necessary to distinguish own-lane absence
from globally drained/closed input. The normal regression separately exercises
a last foreign writer or buffered packet so an own-lane-only EOS shortcut fails.

## Verification and handoff

Nix devShell, two Cargo build jobs, successful commands:

```sh
cargo test --locked -j 2 -p phenix-graph --lib runtime::closure
cargo test --locked -j 2 -p phenix-graph --test fbp_lane_closure --test fbp_request_lanes --test fbp_custom_closure_admission --test fbp_explanations --test fbp_scoped_optional_poll
cargo test --locked -j 2 -p phenix-graph --all-targets
cargo test --locked -j 2 -p phenix-graph --doc
cargo test --locked -j 2 -p phenix-graph --no-default-features --lib --tests
cargo test --locked -j 2 -p phenix-graph --no-default-features --doc
cargo fmt --all --check
cargo clippy --locked -j 2 -p phenix-graph --all-targets -- -D warnings
```

One ignored manual EOS profile is intentional and was explicitly executed in
release mode. Local full test logs are `/tmp/opencode/fbp-h5-lane-default-tests.txt`
and `/tmp/opencode/fbp-h5-lane-no-default-tests.txt`. No workflow was added or
modified. No commit, push or PR was made. Changes remain on
`perf/fbp-h5-lane-closure-20261123` for independent review/publication.

## Independent review follow-up

The maintainer reported that independent final review found no actionable
correctness defect and suggested two nonblocking regression additions. This
records that review result, not external approval or publication acceptance.
Both additions are in `runtime/closure/lane_tests.rs`, using the existing
fixtures/private access with no production or benchmark changes:

- A sealed request-owned zero-input source is delegated through an actual
  `DispatchResult::Deferred`. Its outstanding job retains an exact UpstreamOwner
  witness and prevents a closure-only downstream consumer from running until
  completion, under SharedFifo and BoundedLanes. Completion permits scoped
  absence, one closure-only consumer activation, and request settlement.
- A directly buffered own-lane request packet remains an exact RequestPacket
  witness after writer closure while a foreign queue stays open. Reader disposal
  clears the indexed count and permits scoped absence, not global EOS. Closing
  the foreign writer then proves GlobalEos even with the request unsealed.

Both tests compare exact proofs against the existing reference oracles.
Nix devShell verification was rerun with two Cargo build jobs: targeted closure
tests (9 passed, the manual EOS profile remains intentionally ignored), default
all-target tests, no-default lib/tests, both doctest configurations, formatting
and all-target Clippy. All passed. Follow-up logs are
`/tmp/opencode/fbp-h5-lane-review-targeted-tests.txt`,
`/tmp/opencode/fbp-h5-lane-review-default-tests.txt`, and
`/tmp/opencode/fbp-h5-lane-review-no-default-tests.txt`.
No commit or push was made.

## CI harness separation and rerun

The original implementation and review follow-up were subsequently published
as source `34c22302a` in PR #1390. [Run 45260722335](https://github.com/matthis-k/phenix-ai/actions/runs/45260722335)
failed during compilation of the copied closure benchmark on the workflow's
historical `516e0883875d5ec483e3595d50b77b3d5e8979c4` baseline. The log shows
E0432 for RequestQueuePolicy and E0560 for request_queue_policy: that baseline
predates lane APIs. The existing String-error cfg cannot make those APIs exist.

The correction separates executables, not runtime behavior or compatibility:

- `fbp_closure_benchmark.rs` is restored to its `3bdccbe9b` source except for
  `pub(crate)` visibility on `compiled_with_capacity` and `readiness_options`
  (and rustfmt's wrapping of the latter). Its historical standalone main,
  controls, tests and existing String-error cfg remain intact. There is no
  lane API, lane CLI mode, or dependency on another source file in that file.
- `fbp_lane_closure_benchmark.rs` includes the old benchmark through a path
  module and calls those two constructors. It contains the extracted matrix
  logic, unchanged timer boundaries and assertions, and a smoke test covering
  both policies and all four lane counts. A documented module-local
  `allow(dead_code)` covers the unused standalone main/other benchmark modes.
- No production source, committed closure test, or workflow changed. No new
  backward-compatibility cfg, shim, transport state or simulated workload was
  introduced. This follow-up is uncommitted and has not been pushed.

### Historical workflow reproduction

A fresh detached worktree at `/tmp/opencode/fbp-h5-legacy-516e088-20261123`
received **only** the corrected `fbp_closure_benchmark.rs`, exactly as the
workflow copies it. The new lane example/helper support files were not copied
there. The following build succeeded using that worktree's Nix devShell:

```sh
RUSTFLAGS="--check-cfg=cfg(fbp_benchmark_baseline_string_error) --cfg=fbp_benchmark_baseline_string_error" CARGO_TARGET_DIR=/tmp/opencode/fbp-h5-legacy-target nix develop --command cargo build --locked -j 2 --release -p phenix-graph --example fbp_closure_benchmark --example fbp_benchmark
```

The candidate also built both legacy examples with the workflow's candidate
`RUSTFLAGS="--check-cfg=cfg(fbp_benchmark_baseline_string_error)"`, using
`/tmp/opencode/fbp-h5-legacy-candidate-target`. One complete execution of each
legacy workload on each implementation succeeded with the workflow's exact
arguments, preserving all controls:

```sh
fbp_closure_benchmark 160 0 16 64 128
fbp_benchmark 3000 1 16 64 256
```

[Raw local legacy control output](fbp-h5-lane-closure-20261123-legacy-controls.txt)
records all isolated/overlap fan-in and chain cases and all sparse-scheduler
sizes. This verifies build/run compatibility, not a new legacy performance
claim. It is one local pass per variant, not the workflow's three repetitions
or a successful remote CI result. Historical build log:
`/tmp/opencode/fbp-h5-legacy-build.txt`.

### Current paired evidence

A separate fresh baseline worktree at
`/tmp/opencode/fbp-h5-extracted-baseline-20261123` is exactly `3bdccbe9b` except
for the two copied example files. It uses the original, unoptimized runtime.
Candidate source is `34c22302a` plus this example/documentation separation.
Both examples were copied byte-for-byte from the candidate, including the
workload module; source hashes match across these two worktrees. The corrected
legacy file also has that same hash in the historical worktree.

- Lane example SHA-256: `9edafbcec9c35353a4972c02b720864230badbe3487251a938d5289fbe5ecbb0`.
- Included legacy workload SHA-256: `73510f8d464c02e385c8613583c1cb5004687c0c85b2540a09fd78dadf025b83`.
- Baseline lane binary SHA-256: `82eceb08bedcd9b4c433821ae6cba234e9d769d2046cd20d13f8174555336938`.
- Candidate lane binary SHA-256: `d09fb21c88fdd388583449c9a2bd399c18fd2cf358a9155f64b2605082814e93`.
- Baseline target: `/tmp/opencode/fbp-h5-extracted-baseline-target`.
- Candidate target: `/tmp/opencode/fbp-h5-extracted-candidate-target`.
- [Raw extracted-harness samples and RSS](fbp-h5-lane-closure-20261123-extracted-samples.txt).

Both lane binaries were built with Nix Cargo 1.96.2, `--locked -j 2 --release`,
no extra RUSTFLAGS, on the same i7-8550U host as the archived measurements.
The new CLI is `<target>/release/examples/fbp_lane_closure_benchmark 21 16`;
there is no `--lane-matrix` flag. The runner
`/tmp/opencode/fbp-h5-extracted-measure.sh` pins each process to CPU 2 and uses
GNU time for RSS. It alternates baseline/candidate in odd rounds and
candidate/baseline in even rounds, five rounds total. No tests/builds ran
alongside measurement. The identical setup-excluded matrix still covers
1/4/16/64 overlapping live, reused lanes, both queue policies and single-lane
controls. Host frequency/thermal state and other host activity remain
uncontrolled. Analysis uses the same `/tmp/opencode/fbp-h5-lane-summary.py`.

Pooled medians use 105 raw batch durations divided by live request count.
Ratios are candidate round-median / corresponding baseline round-median.

| Policy | Live Lanes | Baseline ns/request | Candidate ns/request | Paired Round Ratios (1..5) |
| --- | ---: | ---: | ---: | --- |
| SharedFifo | 1 | 164873 | 191594 | 1.048, 0.574, 2.027, 0.989, 0.887 |
| SharedFifo | 4 | 368133 | 351093 | 0.879, 0.931, 1.169, 0.634, 0.961 |
| SharedFifo | 16 | 836760 | 823419 | 1.427, 0.869, 1.162, 0.878, 0.842 |
| SharedFifo | 64 | 1491773 | 1469117 | 1.133, 0.975, 1.047, 1.006, 0.865 |
| BoundedLanes | 1 | 182595 | 193850 | 0.631, 0.969, 1.620, 1.705, 1.058 |
| BoundedLanes | 4 | 553947 | 511152 | 1.120, 1.028, 1.201, 0.914, 0.759 |
| BoundedLanes | 16 | 1794928 | 1190799 | 0.792, 0.653, 0.571, 0.780, 0.651 |
| BoundedLanes | 64 | 3984133 | 2103610 | 0.654, 0.515, 0.481, 0.527, 0.586 |

High-lane benefit repeats in every pair; small-lane/control variability still
precludes a small-case speedup or performance acceptance claim. Do not combine
these samples statistically with the archived executable's samples.
Maximum process RSS in KiB by round: baseline `[4232, 4312, 4196, 4232, 4168]`,
candidate `[4168, 4272, 4168, 4124, 4180]`. This is whole-process peak RSS,
including setup and allocator retention, not per-proof allocation accounting.
The earlier global EOS cost profile remains relevant: its production path is
unchanged, and the extracted matrix still usually exits EOS on the first open
canonical queue rather than exercising a full EOS scan.

### Follow-up verification

Nix devShell, two Cargo build jobs: both example test targets (four legacy
tests, plus five tests in the lane target including its included workload
tests), the public `fbp_lane_closure` regression, and the targeted closure unit
tests (9 passed, 1 intentional manual-profile ignore) passed with default and
no-default features. `cargo fmt --all --check`, all-target graph Clippy with
`-D warnings`, and `git diff --check` passed. The closure implementation hash
remains `b1826d8d4241b01dd62e17024ae85cd3f7077a30fb7b91cc8be79ee416d2f3ec`.
Existing committed tests and raw measurements were preserved. No workflow
modification, commit, push, remote rerun or external approval is claimed.
