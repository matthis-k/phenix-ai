# Q6: Bounded Current-State Explanations

Status: the original bounded Q6 scope is implemented, independently reviewed and
exact-head verified at `89171a843`.

## Scope

This implementation explains current native scheduler conditions, not future
execution. It adds two passive methods on `runtime::NetworkSession`:

```rust,ignore
pub fn explain_process(&self, index: usize, now: Instant) -> Option<ProcessExplanation>;
pub fn explain_request(&self, id: RequestId, now: Instant)
    -> Result<RequestExplanation, RequestError>;
```

`now` is entirely caller supplied. Neither query reads a clock, drives the graph,
supervises a deadline, reserves input, clears a wake, or invokes application code.
Query results own metadata, including strings, IDs, indices, counts, tickets and
timestamps. They contain no packets, values, policies, factories, guards, resources
or leases. Cloning, formatting and dropping a record cannot call application code
or keep an application payload alive.

Q6 here means current native conditions with explicit `UnknownCustom` and external
future events. This is not a Q4 liveness proof, an H5 speed claim, a fairness bound,
a preemption mechanism, an execution history, or a promise of eventual admission.

## Process Contract

`ProcessExplanation` keeps readiness separate from execution state. Read
`session.terminal`, `handoff`, `done`, `scheduled`, `source` and `owner` before
interpreting `readiness`. `scheduled` is actual ready-deque membership;
`queued_flag` separately exposes the scheduler's internal flag, which can remain
set after terminal cleanup. `Available` never means that a job was admitted, that
a worker owns it, or that a future drive will execute it. For example, a process
waiting for a wake can have a complete next input group buffered.

The record includes session identity/generation/cause, physical path, live owner
lane, queue policy, exact outstanding ticket (also with supervision disabled),
optional dispatch timestamp and timeout, and the same overdue condition as
`overdue_jobs(now)`. A future `now` reports overdue without expiring the job.
Startup preflight/factory failures still have an actual runtime handoff, usually
cancelled by failure cleanup. These are reported with the session cause, not an
invented activation. `None` means only an out-of-range compiled process index.
The reported policy is the runtime's stored policy; a startup fallback is not a
claim that the unavailable factory was evaluated.

Wake `active` and `pending` use Acquire loads without swaps. `resume_eligible`
describes the handoff shape (`WaitingWake`, or idle compiled source), not a
promise of resumption: the pending bit, session terminal and done state still
matter. A `ProcessNotifier` is readiness invalidation, not a wake. An idle source
without a pending wake is not automatically activated.

Session queues/handoffs are inspected under the caller's existing serialized
session ownership. Wake producers can race the query. Individual Acquire loads,
the session-state observation, and repeated records are **not** one globally
atomic snapshot. With concurrent notifications, fields may reflect different
instants. Queries do not drain the scheduler's notification set.

The readiness declaration distinguishes initial policy, `WaitInput(name)` and
`WaitInputs(policy)`. Built-ins are explicit (`Any`, `All`, `CorrelatedAll`,
`AtLeast`, `Optional`); custom policies, including fixed-correlation adapters,
remain opaque. Available-port thresholds count distinct ports, not packets or
parallel physical queues feeding one port.

`input_order` and the candidate vector preserve actual cursor/candidate order,
including bounded-lane rotation. `scheduler_candidate` distinguishes native
candidates from extra evidence-only entries for active scopes and unscoped
traffic. Evidence-only absence does not manufacture admission. Exact `WaitInput`
has only its owner's candidate. Each candidate has per-port selectable
`(queue, offset)`, closure proof, and freshness. Queue records distinguish
physical emptiness from an unscoped head and an authenticated request head.
Correlated scoped offsets can be behind the physical head; unscoped packets
remain head-only.

`ReadyAssessment` is `Available`, `Blocked`, `UnknownCustom`, or `Invalid`.
Blocked reasons identify missing required ports, optional ports still awaiting
input/closure, distinct-port threshold deficits, absent candidates, and closure
notifications with no fresh observation. Per-candidate records retain reasons
even when a later candidate wins. Staged exhaustion and already-observed exact
closure use the execution selector's validation. Custom queries stop at
`UnknownCustom` without calling `select`, even with every head available. They
also never call `closure_ports` or `notification_driven`; the latter is the
startup-cached declaration, and closure names come from admission-time metadata.
Custom results and any subsequent global-EOS fallback that depends on those
results are intentionally unknown. With no native candidates, execution does
not call custom selection at all, so the known global-EOS fallback is reported.

The callback-free `idle_inputs_cannot_progress` fact is reported separately.
Idle completion and EOS-stranding facts are present only for known-empty native
selection in idle/input-wait state when the execution fast path does not skip
selection. They reuse the real finish predicate and `stranded_at_eos`, not a new
lifecycle engine. Unknown selection gives no such conditional conclusion.
After failure, the session cause explains an already-applied stranded join;
the record does not mislabel that terminal process as simply awaiting inputs.

Retained publication records enumerate **all selected targets**, not just full
queues, including captures. Retained packet count is actual prepared ownership.
Assessment follows commit precedence: first closed target error in target order,
then `Full`, then `CommitEligible`. Reader closure wins writer closure on the
same target. Native retained transactions were already type/topology validated;
this is not a general-purpose transport validator. An empty selected fanout is
successful commit-eligible work, not a missing publication. `Some([])` still
retains every compiled capture; without captures its empty transaction can be
observed between completion and the next publication attempt.

## Closure Contract

`ClosureProof` and metadata-only `ClosureWitness` are produced by the same
iterative proof used for execution's boolean projection:

1. Actual global EOS wins, including for unsealed requests.
2. Otherwise a sealed scope can prove `ScopedRequestAbsent` even while global
   writers remain open.
3. `NotProven` gives the first obstruction: buffered global input, open global
   writer, unsealed request, indexed matching request packet, upstream owner,
   or registered upstream closure obligation.

The proof retains the existing visited set, upstream reachability pruning,
indexed request packet checks and conservative treatment of cyclic closure
obligations. It does not recursively explain upstream processes. Witnesses link
by process/queue IDs. It emits no strings or diagnostic trace on execution's
hot path. Closure truth and observation freshness are separate: an already
delivered closure remains proved but cannot be replayed as fresh work.

## Request Contract

The outer `Result` authenticates the session/generation-bound request ID first.
Foreign and released IDs fail before constructing any record. Successful
authentication retains `request_status` as an inner result, so `InvalidCapture`
and `CaptureCount(n)` still have explanations.

The request record includes sealed state, expected capture name, **observed**
capture count, invalid-capture flag, actual queue policy/lane, terminal origin,
current failure text, exact `RequestWork`, disposal and invalidated ticket
inventory. Observed captures are host-consumed captures, not queue occupancy.

The owners vector enumerates every current `p.request == id`, without assuming
its handoff state. The queue vector enumerates every physical queue with nonzero
indexed `request_count(id)`, including capture queues. The obligation vector
enumerates exactly the registered, unobserved obligations counted by
`request_live`, with process and port IDs. Matching queue counts sum to
`work.queued`; obligations count to `work.closure_pending`. These are the actual
live-ownership terms, not a speculative graph reachability report. Terminal
status can override live terms while cleanup has not yet disposed them.

A queued input's consumer may currently belong to another request. Such entries
are explicitly `OtherRequest(owner)` and are not charged to this request's
activation counters. Reachability alone is never a settlement blocker.

`TerminalOrigin::Local` means this scope initiated its terminal transition;
`InheritedSession` means the session fixed its outcome. One private
`RequestScope.inherited_terminal` bit, false on admission and assigned once on
first materialized termination, preserves that distinction through explicit
post-failure cleanup. Failure text is not used to infer provenance. The bit does
not alter outcome selection, recovery, disposal or the public error protocol.

A head request ID's `live_lane` is obtained with `requests.get(id)`. A released
SharedFifo capture can retain a stale ID with `live_lane: None`. Its physical
queue lane does not grant authority, resurrect a scope, or authorize indexing
the requests map. Request credit/lane reuse never reuses request identity.

## Implementation Choices

`diagnostics.rs` exports the metadata API from private
`diagnostics/explanation.rs`. `readiness.rs` shares callback-free order,
candidate discovery, packet lookup, snapshot construction, built-in selection,
staged validation, name validation/freshness, exact wait and global-EOS fallback.
Execution retains the sole `Custom.select` call. Existing cursor updates,
head-only rules, correlated offsets, lane round robin and EOS fallback are
unchanged. The diagnostics-only evidence collections are not built by execution.
`closure.rs` supplies one common witness-producing iterative proof.
`mod.rs` changes only exports and extraction/use of the idle-finish predicate.
`request.rs` changes only read visibility plus provenance initialization;
`recovery.rs` assigns that bit at the first materialized terminal transition.
No `process.rs` accessor was necessary.

Alternatives rejected:

- Running `selection()` would invoke arbitrary custom code and could change
  application state; catch-unwind would not make it passive.
- Copying built-in selection/closure algorithms would create a second
  interpreter that could drift on cursor ordering, fresh closure or cycles.
- A history/event log would add persistent ownership, allocation and lifecycle
  semantics unrelated to a bounded current-state query.
- Guessing terminal origin from text would lose inherited provenance after
  explicit cleanup and confuse diagnostic messages with authority.
- Treating every reachable process as unfinished work would contradict actual
  settlement and charge foreign owners to the wrong request.

The API intentionally uses several small metadata types instead of printable
free-text summaries. This increases public surface, but allows consumers to
distinguish empty/unscoped/stale heads, readiness versus handoff, closure versus
freshness, and local versus inherited termination without parsing prose.

## Bounds

Records contain only currently admitted requests, compiled processes/ports,
allocated bounded queues and current pending targets/tickets. No recursive
record expansion or past-event collection occurs. For a process with P ports
and R active scopes, candidate evidence is O((R + 1)P), in addition to its
physical input/target queue metadata. Each scoped proof may walk the current
upstream graph iteratively; the query can repeat those walks per candidate/port.
This is bounded by current graph/session sizes, not a constant-time or speed
claim. Execution does not allocate explanation records or collect witnesses
into a trace. The proof's existing visited/stack allocations remain.

## Verification

The focused `fbp_explanations` suite currently has 22 real-session tests. It
checks all built-ins against subsequent reservation/logged process inputs,
multi-queue distinct-port thresholds, both request queue policies, FIFO foreign
heads/correlated offsets and mixed unscoped traffic, exact/group staged waits,
closure proof versus freshness, global EOS precedence, scoped absence with an
open writer, indexed packet/upstream owner/cached custom obligation witnesses,
a cycle, and a 2,048-node iterative upstream walk.

It also covers direct and Deferred jobs with/without supervision and future
caller time; wake/notifier distinction and unconsumed pending wakes; retained
internal/capture fanout and closed-before-full precedence; empty selected
success; status Open/Pending/Settled, capture count and invalid capture errors;
terminal retained captures, inherited provenance after materialized cleanup,
foreign/released/reused IDs; and startup failure metadata.

Adversarial policy/factory/worker/dispatcher/guard/codec/payload probes are armed
**after** setup. Repeated query/format/drop comparisons preserve observable
queues, cursors, closure freshness, handoffs, outcomes, tickets and wake bits,
with no callback or payload destruction. Separate wake tests verify that the
next drive consumes the existing wake. No public test inspector was added.

Final results on the current worktree, after the no-native-candidate Custom/EOS
precision fix and its regression:

| Check | Result |
| --- | --- |
| `cargo test --locked --offline --workspace --all-targets --quiet` | Pass, including all 22 explanation tests and existing readiness/lane/deadline/cycle regressions |
| `cargo test --locked --offline --workspace --no-default-features --lib --tests --quiet` | Pass, including all 22 explanation tests |
| `cargo test --locked --offline -p phenix-graph --no-default-features --lib --tests --quiet` | Pass, including all 22 explanation tests without workspace feature unification |
| `cargo test --locked --offline --workspace --doc --quiet` | Pass |
| Graph no-default doctests | Pass in the final full Nix check; 9 ordinary and 17 compile-fail |
| `nix develop --command cargo clippy --locked --offline --workspace --all-targets -- -D warnings` | Pass |
| `nix develop --command cargo clippy --locked --offline -p phenix-graph --no-default-features --all-targets -- -D warnings` | Pass |
| `nix develop --command cargo fmt --all --check` | Pass |
| `git diff --check` | Pass |
| Full `nix flake check --no-update-lock-file --print-build-logs --max-jobs 2` on a filtered worktree source | Pass, all checks |

The Nix invocation deliberately included untracked new files, rather than using
the default tracked-only Git flake view. The source argument was constructed
with `nix eval --impure --raw --expr` of `toString (builtins.path { ... })`, using
this repository as `path` and excluding only entries named `.git` or `target`.
That store path was passed as `path:<store-path>` to `nix flake check`.
The final all-package archive was
`/nix/store/cn0rfra8a6nl6ks36l9rh8gpxdpp0m0k-source`.
The evaluated final graph derivation was
`/nix/store/j1siwjb4z9wpg4gcn7czp2757mh6zans-phenix-graph-0.1.0.drv`, and
no-default was
`/nix/store/7v0v36mi61jbf51z8c2bbrfch7v4nysj-phenix-no-default-0.1.0.drv`.

Full Nix coverage included `graph`, `kernel`, `storage`, `derived`,
`integration`, `no-default`, `clippy`, `format`, `workflow` synchronization
validation and `kernel-smoke`. The first pre-final full build hit the tool's
10-minute timeout without a test failure; a longer retry passed. The final
post-fix full build also passed. Existing schema retention measurement remains
ignored by default; it was not silently treated as executed. Bench executables
ran because of all-target checks, but their concurrent debug-build timings are
not performance evidence for this change.

Only owned runtime files, the new test file and this document were edited.
The pre-existing dirty `flake.nix` and other agents' new tests/docs were left
unchanged and included in the workspace checks. No commit, push or remote
workflow execution was performed; local `workflow` validation did not modify or
trigger a workflow.

### Gaps And Limits

The broad acceptance matrix is not an exhaustive state-space proof. In
particular, the new differential test is a finite input matrix, not randomized
model checking of every cursor/lane rotation. Existing lane/cycle regressions
cover additional execution cases but are not all new explanation-specific
oracles. No concurrent-wake atomic-snapshot guarantee, future custom-selection
prediction, worker progress guarantee, fairness measurement, or runtime speed
benchmark is claimed. The required local workspace/CI-equivalent commands
completed, but those results do not substitute for an exhaustive
interleaving/state-space proof or a remote exact-commit CI result. No remote
workflow run or commit/push is part of this task.

Independent final review of the actual runtime/helper/provenance changes and the
real-session tests found no actionable defect and supports a whole-original-Q6
closure candidate for bounded current-state explanation. The integrating owner
also passed full assembled default/no-default workspace tests, isolated graph
no-default tests/doctests, formatting and strict workspace Clippy. These findings
do not extend the stated liveness, callback or concurrent-snapshot limits, and do
not replace exact committed-head hosted acceptance.

That gate subsequently passed at implementation
`89171a8437c6fc2aebcd9029eb119d8c7505d389`: all fifteen workflows are successful,
including the [shared exact-head acceptance gates](fbp-lifecycle-acceptance-20261116.md#exact-head-acceptance).
Q6 is scoped verified for bounded current-state explanation, not future execution
or Q4 static liveness. This evidence-only follow-up needs its own head's checks.
