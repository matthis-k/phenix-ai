# H4 — read-only idle-input prefilter, Phase 0/1

Baseline: `e1b4b99b33a2c5c22599f4668ff069721a1d3756`, stacked after the Q8/H6 measurement branch.
Relevant prior H4 measurements:
- [run 43201132241](https://github.com/matthis-k/phenix-ai/actions/runs/43201132241):
  1→256 processes, 0.68→31.9 μs/event (~47×) on AMD EPYC 9V45.
- [run 43201797923](https://github.com/matthis-k/phenix-ai/actions/runs/43201797923):
  1→256 processes, 1.06→59.6 μs/event (~56×) on AMD EPYC 7763.
Both use open, idle inputs surrounding one active ingress. Absolute numbers
across different runner models are not directly comparable.

## Phase 0 — problem and conservative alternative

`NetworkSession::refresh()` currently walks the process inventory and calls
the full readiness selection logic for every Idle/WaitingInput process, even
when all physical input queues have **no packets**, **no EOS**, and **all writers
still open**. Nothing can be reserved in that case for native data-driven
readiness policies, but repeated work allocates and probes request/queue state.
The existing bounded ready deque and closure logic are otherwise sound.

No change retains the tested native scheduling semantics at a measurable
sparse-activity cost. Moving immediately to a dirty worklist could eliminate
the O(V) scan but introduces extensive new invalidation/notification obligations
at ingress, completion, fanout, closure, request, custom policy and worker wakes.

## Phase 1 — compare choices

| Choice | Benefit | Risk | Decision |
| --- | --- | --- | --- |
| Do nothing | Stable semantics | Known sparse-input cost | Reject as last word |
| Skip *all* idle processes | Fast | Miss EOS, optional closure and stateful policy transitions | Reject |
| New queued dirty-process scheduler | Potential O(active) | Hard to prove EOS/wake/fairness/request invalidation across many mutation sites | Defer pending measured baseline and differential model |
| **Read-only no-work precheck inside existing refresh** | Skip expensive selection if no data/closure is possible; no new state/authority | Still O(process count) cheap checks; conservative exclusions | **Select** |

Only skip selection when **all** hold:
- process is Idle/WaitingInput, not source, not queued or pending publication;
- it has at least one input, and each input has at least one physical queue;
- every physical input queue is empty **and still open** (writer not closed);
- no active/retained request context exists (request-local closure is different);
- effective readiness is not a host-supplied Custom policy whose observable
  decisions may depend on external state.

The precheck is purely read-only; it does not consume wake bits, select packets,
create readiness events or mutate queue state. It can return false-positive
"needs full evaluation", never false-negative on a known changed packet or EOS.
It deliberately does not optimize the O(V) inventory walk.

## Phase 4 evidence required

- Full native graph/kernel/storage regressions, including custom policies,
  request-local closure, finite EOS, cancellation, wakes and bounded capture.
- Exact-head format, Clippy, and six-job Nix matrix.
- Same benchmark workload, 3 repetitions, exact source SHA and runner metadata,
  comparing within-run N=1 vs N=256 ratio to earlier baseline. No claim of
  machine-independent absolute speedup.
- Do not call H4 fully fixed unless measured and generalized activation-scaled
  scheduling is implemented; if the prefilter does not materially help, revert.

## Phase 4 — first measured evidence

[Optimized run #43202997957](https://github.com/matthis-k/phenix-ai/actions/runs/43202997957)
completed three release-mode 5,000-event repetitions with *exact* source
head `c579883359cfb26c9198d024601a4f347ac9624f`, AMD EPYC 7763,
Rust 1.96.1. Mean event latency median of three repetitions:

| Total physical processes | Before (same CPU model, run #43201797923) | After (run #43202997957) |
| ---: | ---: | ---: |
| 1 | 1,062.8 ns | 946.7 ns |
| 16 | 4,474.9 ns | 1,314.5 ns |
| 64 | 15,644.7 ns | 3,003.7 ns |
| 256 | 59,712.8 ns | 9,822.8 ns |

The 256-process throughput proxy improved by approximately **6×**
across different jobs using the same CPU model. Within-run scaling
from 1 to 256 fell from ~56× to ~10×. Different GitHub-hosted VM
instances and uncontrolled load preclude a precise causal speedup
claim; benchmark code and toolchain are unchanged. The change is a
low-risk **constant-factor improvement** that retains an O(V) scan.
H4 still requires a fully event-indexed scheduling design to close.

An integration regression now also verifies empty-but-open quiescence,
subsequent EOS completion and repeated Custom policy evaluation.
Exact final-head CI and a rerun of the benchmark are still required
after this evidence commit.
