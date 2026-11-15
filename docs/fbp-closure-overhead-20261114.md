# Scoped Closure Overhead Correction

Baseline candidate: `a2d7b4d388bae7eddda61e09fad2cec7d88d9d67`, whose exact-head
CI passed but paired closure-chain timing regressed against verified `516e088`.
This is a narrow fix inside readiness/closure accounting, not another scheduler,
cache, public API, or revival of rejected optimizations #1293/#1295.

## Diagnosis And Correction

Callgrind and generated assembly localized the extra work to two changes:

- Adding cached Custom closure obligations had changed compilation of the existing
  Optional hot loop: it outlined an iterator helper and increased stack state even
  when no Custom policy executed. Optional and cached Custom declarations now feed
  one borrowed-name observed-input loop, preserving all reachability, terminal,
  required-empty and per-request observation checks.
- Every internal readiness snapshot looked up its request lane even when a built-in
  policy never used that metadata. The lookup now occurs immediately before the
  effective Custom callback, including staged Custom policies. Every scoped
  activation still receives `ProcessContext::correlation_lane` under both queue
  policies. No new lane cache or invalidation contract is added.

No-change retains the measured regression. Skipping repeated policy validation,
introducing cached closure proofs or changing traversal order would require a
different safety argument and is not part of this correction.

## Measured Local Evidence

Standard locked release builds, rustc 1.96.1 / LLVM 21.1.8, identical benchmark
source SHA256 `4674161e72412d9443518f676e3bf5556815d26d7392bd80cd6cd4af313ab7c7`.
Callgrind runs `fbp_closure_benchmark 1 128`; these executed-instruction counts
include both harness modes, setup/teardown, and 16 warmups per mode. They are not
wall-time or production-throughput measurements.

| Instructions | Verified `516e088` | Prior `a2d7b4d3` | Corrected Working Sources |
| --- | ---: | ---: | ---: |
| Whole program, breadth 128 | 1,391,745,817 | 1,463,848,179 | 1,406,769,142 |
| Closure accounting, breadth 128 | 429,282,045 | 485,473,471 | 441,977,815 |
| Selection self, breadth 128 | 192,239,060 | 208,116,312 | 194,531,867 |
| Input-closure self, breadth 128 | 91,190,584 | 91,190,584 | 91,190,584 |

Approximately 79.2% of prior excess instructions disappear; total instructions
remain about 1.1% above the verified baseline. Assembly confirms the outlined
Optional helper is gone. Local CPU2-pinned native timing remained noisy and is not
accepted as evidence that the hosted timing regression is closed. Raw profiling
files remain under `/tmp/opencode/h5-a2-diagnosis` on the development host; final
paired CI artifacts must supply durable same-head timing evidence.

## Hosted Counterevidence And Admission Scan

At `f608a3fc91abbcee67d928067c276c33f13feae0`, hosted correctness gates passed,
but [closure timing 43670026282](https://github.com/matthis-k/phenix-ai/actions/runs/43670026282)
still measured approximately +7.8% / +8.2% for chain64/128 against `516e088` on
EPYC 7763. Separate EPYC 9V74 controls were near parity. The instruction reduction
therefore does not establish removal of the architecture-dependent timing
regression, and H5/H6 performance acceptance remains open.

The admission audit also found a concrete new full-inventory scan: `begin_request`
visited every process just to discover Custom closure declarations. A private
construction-time index now contains only base Custom process IDs, including both
polled and notification-driven policies. Base policy and physical IDs are immutable
for the session; staged `waiting_policy` and process-state recovery do not change
that index. Callback results are not cached across requests: `closure_ports(lane)`
still runs, validates and caches once per admitted scope. With no Custom policy,
the empty index allocates nothing and no declaration-discovery inventory scan runs.
This removes known unnecessary work, not a claimed measured speedup. The
`fbp_custom_closure_admission` test covers zero-packet closure across repeated scopes
under both queue modes and Custom notification contracts.

## Verification

Focused closure, staged Custom, optional/multi-root, lane, obligation-dedup and
benchmark tests pass, as do workspace tests and strict all-target Clippy. The new
`fbp_readiness_lanes` test checks actual initial/staged Custom snapshots and
activation metadata for scoped lane 1 and unscoped traffic under both queue modes.
Independent adversarial review and final exact-head paired CI remain acceptance
gates; instruction counts alone do not close H4/H5/H6 or M7.
