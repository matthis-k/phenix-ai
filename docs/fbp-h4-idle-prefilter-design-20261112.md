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
