# H5/H6 bounded lane-local closure traversal

Base: local `fix/fbp-staged-poll-source-20261123`, `3bdccbe9b`.
Worktree: `/tmp/opencode/fbp-h5-lane-closure-20261123`.

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

## Results: paired measurements

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
