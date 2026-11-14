# FBP Hardening Campaign: Dependency And Policy Record

Status: implementation/evidence record for branch
`fix/fbp-lifecycle-request-lanes-20261114`, based on `16e77f6c6` plus the inherited
callback/correlation worktree. This is not the PR #1300 stack. No entire issue or
milestone closure is claimed; the 33-issue tracker keeps all baseline scores.

## Dependency Map

| Batch | Shared authority | Depends on | Evidence boundary |
|---|---|---|---|
| Stage A: callback/lifecycle | Kernel producer state, exclusive `NetworkSession` lease, `ProcessExecutionGuard`, plugin/code leases | Graph runtime callback contracts and host generation ownership | Real-thread races, panic/drop adversaries, bounded Loom model, exact-head CI |
| Stage B: request progress | Request IDs, FIFO transport, readiness snapshots, closure accounting | Stage A callback ownership; explicit correlation bounds | Crossed requests, slow captures, per-request memory, blocking diagnostics |
| Stage C: composition API | Typed component interface, `NetworkPatch`, grants, provenance, deterministic composer | Compiler/replacement authority | Public plugin examples and executable independent-contributor tests |
| Stage D: graph/storage | Storage library identity and capabilities, StorageGraph adapters | Stage C authority and Stage A leases | Cross-plugin same/different identity, scope, revocation, reload and persistence |
| Stage E: scale | Scheduler dirty state and closure proofs | Stable Stage A-C semantics | Paired representative runtime, allocation, RSS and latency evidence |
| Stage F: agent integration | Ordinary plugin/composite composition and explicit model/tool/storage capabilities | Stage C-D integration | Deterministic P1/P2 end-to-end tests; no kernel agent special cases |

The current flake has one exact-head Nix inventory, a shared cache writer, and
three independently executed check shards. Graph, Kernel, Storage, StorageGraph,
no-default-feature, integration, format and workspace Clippy checks already
partition the main failure domains. This candidate adds a path-scoped session
ownership Loom workflow over the actual Kernel ownership primitive. PR #1300's
different wake Loom, thread-stress and paired benchmark checks apply only to that
PR's exact SHA, not this branch. The pinned CI library documents dependency-derived
PR impact selection and exact-head Nix check sharding as experimental. Do not
configure them here without validating the generated workflow and its required-
check semantics retain full main/manual acceptance.

## Stage A Design Protocol

### Phase 0: constraints and no-change baseline

Kernel owns producer/session synchronization and admission. Plugin callbacks must
not run under those mutexes. The existing session mutex serializes mutable graph
state; moving the session out of that mutex must not allow same-thread or synchronous
worker re-entry to wait on its own exclusive lease. Host publication must not wait
for arbitrary plugin callbacks. Panic containment must restore or retire ownership
before returning. Existing tickets, producer retry, code leases, and request IDs
remain authoritative.

The checkout is `impl/graph-consolidation-20261110` at `16e77f6c6`, with uncommitted
changes. PR #1300 at `516e0883875d5ec483e3595d50b77b3d5e8979c4` is open and its exact
head passed its configured checks, but it is not this candidate.

### Phase 1: alternatives

- **Existing implementation unchanged:** rejected for this batch. Producer
  admission called `NetworkSession::inject` while holding the producer-state
  mutex; a custom readiness callback that re-entered `drain` could deadlock.
  Caller-thread-only session re-entry tracking also missed a joined dispatcher
  worker.
- **Smallest extension:** release the producer mutex after a `retired` check and
  then inject. Rejected because publication could win between the check and graph
  admission. A second check after injection would not undo a committed packet.
- **Proposed extension:** claim admission while holding the producer mutex, release
  it before session injection, and treat the permit as the admission linearization
  point. Retirement does not wait for a permit already issued; a late poll cannot
  obtain one. Propagate a stable Kernel session identity using the existing process
  execution-guard interface, composing rather than replacing the caller's guard.
  Validate request closure declarations before installing request state and reuse
  one readiness validator before correlation wraps a built-in policy as custom.
- **Redesign:** actorize the Kernel or add a callback executor. Rejected because it
  adds another scheduler/authority and cannot preempt arbitrary callbacks or undo
  external effects.

### Phase 2: independent adversarial critique

An independent read-only review found: admission/readiness under the producer
mutex; same-kernel re-entry from a joined worker; unknown closure names being
ignored; built-in readiness constraints bypassed by correlation wrapping; panic
payload destructors able to unwind before producer reconciliation; and repeated
same-scope packets able to fill a lane and block later scopes. The first five have
candidate code or test changes. The last is an explicit adapter limitation, with a
negative FIFO test; the API does not claim arbitrary repeated-stream fairness.

Residual boundary: a plugin callback may still create a separate OS thread, call
back into the same Kernel there, and synchronously join it. Thread-local execution
identity is propagated only through the existing `ActivationJob` execution guard;
arbitrary plugin-spawned threads and plugin-owned/resource locks are not under
Kernel control. In-process Rust code is trusted, not sandboxed.

### Phase 3: implementation and verification status

Candidate regressions cover request admission callbacks, local and worker process
re-entry, closure-policy panic/invalid names, readiness wrapper validation,
producer retirement during poll, and panic-on-drop panic payloads. The focused graph
stream and Kernel session tests pass locally. This remains candidate-local evidence;
full exact-head CI and independent post-fix review are still required.

## Lifecycle Policy Decisions

### C3: retired generation authority (decision required)

Current tested behavior distinguishes host ownership from explicit capabilities:

- Publication retires host-owned producer polling and rejects a poll result that
  returns after retirement.
- An event with an admission permit acquired before retirement may finish admission;
  publication never waits for its callback.
- A caller that retains `Arc<Generation>` may continue explicit old-generation
  `inject`, `run_steps`, and `with_session` calls. Those calls can affect only the
  old session, whose plugin/code/resource leases remain alive. They do not target
  the successor generation. Old compiled/session/job handles can outlive the host
  according to their own leases.
- When the final explicit generation and outstanding value/job leases are dropped,
  the archived generation and code can be reclaimed.

Alternative strict revocation would reject new explicit calls after publication,
while allowing already-entered session calls to finish. It cannot forcibly stop a
currently executing callback or revoke independently cloned compiled handles/jobs
without adding a shared revocation token throughout the graph API. Existing tests
deliberately use an old explicit generation after replacement. The project owner
must approve changing that externally visible contract before C3 can be closed as
revocation rather than explicit archival.

### C5: deferred-job supervision

- **No change:** zero clock/ticket bookkeeping, but an accepted external job can
  remain pending indefinitely.
- **Opt-in per-handoff deadline (implemented in this candidate):**
  `SessionOptions.deferred_job_timeout` defaults to `None`. `run_until_idle` starts
  timing only after its dispatcher returns `Deferred`; public `next_job` starts at
  handoff. The deadline is accepted by the session, so work completed earlier but
  delivered after the deadline is overdue. Supervision invalidates the exact ticket
  and uses existing request failure policy; unscoped work fails the session.
  `Kernel::run_until` waits no longer than the earliest outstanding deadline.
- **Dedicated watchdog thread/executor:** rejected because session recovery must
  still acquire the exclusive session lease, adding a timer thread cannot preempt a
  synchronous dispatcher call, and it creates a competing lifecycle authority.

Independent critique required the timer to bind to the exact `StepTicket`, clear on
every handoff transition, supervise before accepting late completion/rejection or
exposing request output, preserve rejected objects, and avoid turning a timed-out
request's expected late return into a session-wide dispatcher protocol failure.
The implementation does so and tests both default terminal policy and request-local
restart with an unrelated request continuing. A late worker still owns its process,
payloads and code lease until it actually drops them. The timeout cannot stop the
worker, undo effects, bound a blocking dispatcher call, or enforce expiry if a
manual session is never driven; `request_status`/`request_work` remain snapshots
until a supervision operation. Explicit cancellation wins if serialized before
supervision. These limits are part of the contract, not security or liveness claims.

### C6: replacement state policy

Generation replacement is restart-only for process-local state: candidate factories
spawn fresh process state (tested by `packages/phenix-kernel/tests/generations.rs`)
and the host does not export/import arbitrary `Process`
state. Continuity belongs in explicit plugin-owned, versioned durable resources;
those resources require their own schema/migration and access-grant contract. A
failed candidate does not replace the current generation. Cancellation/restart of
an in-flight process does not undo external effects. Automatic in-memory state
migration is not supported.

### C7: plugin trust boundary

Every native Rust `PluginInstance`, preparation/activation callback, `Producer`,
`ProcessFactory`, `Process`, readiness policy, dispatcher, and resource callback
executes in-process and is trusted with the host's address space. Typed imports,
storage grants, provenance, and leases constrain accidental authority and lifetime;
they are not an OS security sandbox. Untrusted native plugin code requires a
separate process and a serialized capability protocol. That isolation adapter is
not implemented by this FBP kernel.

C3 still requires owner approval before changing the tested old-generation API
contract. C5 uses an opt-in timeout, preserving indefinite behavior by default.
Neither item blocks independent graph, composition, storage, CI, performance, or
agent-integration work.

## Stage B Design: Request Demultiplexing

### Phase 0: current contract

There is one FIFO per compiled connection, shared across request IDs. `head_for`
never skips a packet; it only selects a matching request at a connection head.
Public input admission atomically fans out to the compiled ingress queues. Each
export has one shared bounded capture FIFO, and `take_request_output` cannot bypass
another request at its head. Each compiled physical process has one mutable
`ProcessHandoff`; a staged continuation or pending fanout is a serialization point.
`max_requests` bounds active request records, not all queue payload bytes or
application-private state.

### Phase 1: alternatives

- **Keep current shared FIFO:** preserves the strongest simple ordering contract
  and has no request-lane memory multiplier. Crossed arrivals and a slow capture
  can block later requests; this is correct backpressure, not fairness.
- **Partition only public ingress/captures:** independent bounded lanes at these
  endpoints would fix ingress/capture head-of-line cases, but internal compiled
  connection queues remain shared. It must not be called general demultiplexing.
- **Bounded virtual transport lanes:** allocate/reuse a queue lane for each active
  request across the affected compiled edges and captures. Preserve strict FIFO
  inside each lane and keep ordinary unscoped traffic on the existing queues. Queue
  state is bounded by active request credit, but the envelope scales with
  `max_requests × (connections + ingress fanout + captures) × capacity`.
- **Separate process state per request:** this is required to run another request
  through a process whose previous request owns a staged continuation or blocked
  publication. It changes current one-process-per-session state semantics, factory
  call counts, resource sharing, recovery, and external side-effect ordering; it is
  not implied by queue partitioning.
- **Drop, reorder, or retain unbounded spill buffers:** rejected. These would weaken
  strict FIFO or bounded-memory guarantees and can silently duplicate/loss effects.

The selected candidate is opt-in queue-only virtual lanes, exposed as
`SessionOptions::request_queue_policy = BoundedLanes`; `SharedFifo` remains the
default. Lanes cover every compiled edge, public ingress target, and capture alias.
Their packet-slot bound is checked as `max_requests × (sum(connection capacities) +
sum(ingress capacities per target) + capture capacity × exported captures)`. Lanes
are allocated lazily and reused only after scope release. A nonconsuming reader can
fill its own bounded capture lane, and a single process with a staged continuation
or blocked publication remains a serialization point. This is not unconditional
request progress or fairness.

### Phase 2: independent critique

An independent review confirmed that queue lanes only promise that one request's
head/full queue does not consume another lane's queue credit. They cannot promise
progress through a process already holding another request's continuation or
pending publication. The implementation preserves unscoped queues, global EOS,
all-or-nothing fanout, alias capture authentication, and session/generation ticket
validation. `take_output` remains the shared unscoped FIFO; scoped lane captures are
retrieved with `take_request_output`. Cross-order between scoped lanes and unscoped
traffic is not promised. StorageGraph's request-specific capture draining still
requires integration verification.

### Phase 3: implementation candidate

Candidate tests cover crossed join arrivals, a slow request capture with another
request completing, internal-edge routing, lane reuse after settlement and
cancellation, and aggregate packet-capacity overflow. The existing shared-FIFO
behavior remains the default, and its head-of-line regressions remain. These tests
do not prove arbitrary repeated-stream fairness or process-state isolation; H1/H7
remain open pending adversarial workspace and StorageGraph integration review.

## Post-Implementation Review

Independent reviews reproduced and drove fixes for premature global EOS, missing
global endpoint closure propagation, lane reuse reopening finished readers,
inaccessible retained captures after session failure, default shared-FIFO queue
cursor bias, and scoped/unscoped/closure-only round-robin biases. Lane creation now
copies canonical queue closure; reuse never reopens permanent endpoints. Global EOS
includes all allocated queues; scoped failure captures remain drainable before
release. These are covered by `fbp_request_lanes`, not accepted as documented
limitations.

Execution guards are now one ordered `SessionOptions::execution_guards` list.
Kernel identity is entered first, StorageGraph's host-only guard follows, then
application/dispatcher guards. `ActivationJob::with_guard` appends; tokens remain
active through process panic-payload disposal and unwind in reverse order. This
replaces the single replaceable guard slot rather than adding a compatibility API.
`worker_guards` tests dispatcher append, application-token teardown, panic-payload
re-entry, and retained admission-error payload/code leases. Producers now attach
the Kernel lease to values before an admission error can escape.

Timeout failure attribution also checks the original dispatch ticket: if sibling
expiry already restarted its request during a synchronous dispatch, a later panic
cannot fail the replacement process's unrelated request. `fbp_deferred_timeout`
tests `Completed`, `Rejected`, and panic returns crossing sibling expiry, both with
and without queued replacement work.

### API Migration

Before: `options.execution_guard = Some(guard)` could be displaced by another
caller or dispatcher. After: `options.execution_guards.push(guard)` composes in
entry order with reverse teardown. Other public source changes include the new
`SessionOptions` fields, `RunReport::next_deferred_timeout`, and detailed invalid
closure declarations; `RequestError` is no longer `Copy`. Exhaustive downstream
struct literals/matches require migration. No compatibility shim is retained.
The request-lane example in `packages/phenix-graphs/API-EXAMPLES.md` is Rustdoc-tested.

### Verification Record

Local lower-tier checks on the assembled working sources passed:

- Default workspace tests, Rustdoc examples, and compile-fail tests.
- No-default-feature workspace tests and graph-only no-default-feature tests.
- Strict workspace all-target Clippy (rerun after test-only lint fixes).
- Three bounded Loom properties over the production `ExclusiveSlot` source.
- Native request-lane, timeout, producer/session, worker-guard and generation suites.
- StorageGraph host queries under both queue policies, local/offloaded execution,
  observer drain, failure recovery, one-request credit reuse and retained-value leases.

The final accepted commit and GitHub run URLs belong in the draft PR's exact-head
check rollup. Earlier local or PR #1300 runs must not be relabeled as this head's
evidence. The unchanged six-job Flake inventory remains the full gate; the added
path-scoped session Loom workflow is a separate failure domain, not a substitute.
No new paired performance, allocation or RSS acceptance results are claimed.
