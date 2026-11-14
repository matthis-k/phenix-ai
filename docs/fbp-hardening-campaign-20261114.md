# FBP Hardening Campaign: Dependency And Policy Record

Status: verified-stack merge reconciliation for branch
`fix/fbp-lifecycle-request-lanes-20261114`, PR #1302 based on
`impl/graph-consolidation-20261110`. The campaign initially relied incorrectly on
stale local remote-tracking `origin/impl/graph-consolidation-20261110` at
`16e77f6c6`, plus inherited callback/correlation work. First commit `0d145f3`
therefore did not contain the later foundations already on actual remote impl
head `89e00d06498979a2f231dfc8ca780eba44140d27`. After fetching, we are now
merging exact verified PR #1300 head
`516e0883875d5ec483e3595d50b77b3d5e8979c4`, preserving the full stack and
all scoped gates. Earlier descriptions of a separate candidate missing that stack
were true only of the initial tree, not the merge target.

No entire issue or milestone closure is claimed. The authoritative
[33-issue tracker](fbp-issue-progress-estimates-20261113.md) compares all scores to
verified `516e088`: every delta is +0.0 pp, H3b 78.0%, Q5 74.0%, mean 55.3%,
and classifications 10 scoped verified / 7 partial / 16 open. The incoming
[issue ledger](fbp-issue-ledger-20261111.md) remains historical detail, not a second
score authority. M7 and M8 remain incomplete, including missing P1/P2 production
model/provider, conversation, local/SSH tool and frontend adapters.

Integrated implementation head `a87fd5a824496c4db04ebe11cbf8827b4f29e9ea`
passed [six-job Flake CI 43611241004](https://github.com/matthis-k/phenix-ai/actions/runs/43611241004),
[session Loom 43611240644](https://github.com/matthis-k/phenix-ai/actions/runs/43611240644),
[wake Loom 43611240338](https://github.com/matthis-k/phenix-ai/actions/runs/43611240338),
[eight-repeat native Q5 43611329715](https://github.com/matthis-k/phenix-ai/actions/runs/43611329715),
and [in-flight recovery 43611329804](https://github.com/matthis-k/phenix-ai/actions/runs/43611329804).
Its H4/H5/H6 workflows also completed, but the paired comparisons used older
optimization parents; they do not establish incremental acceptance against
`516e088`. Subsequent baseline-provenance corrections require their own exact-head
CI and measured comparison. All scores remain unchanged.

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
wake Loom, thread-stress and paired benchmark gates must also be retained by the
merge. Their historical passes apply only to their exact SHAs, not the new head.
The pinned CI library documents dependency-derived
PR impact selection and exact-head Nix check sharding as experimental. Do not
configure them here without validating the generated workflow and its required-
check semantics retain full main/manual acceptance.

## Stage A Design Protocol

### Phase 0: constraints and no-change baseline

Kernel owns producer/session synchronization and admission. Plugin callbacks must
not run under those mutexes. The existing session mutex serializes mutable graph
state; moving the session out of that mutex must not allow same-thread or synchronous
worker re-entry to wait on its own exclusive lease. The reconciled host policy
retains the verified whole-drain gate: external publication waits for an admitted
producer drain, but never for explicit session work. The initial broader no-wait
publication constraint is superseded; a nonreturning producer callback can delay
publication. Panic containment must restore or retire ownership
before returning. Existing tickets, producer retry, code leases, and request IDs
remain authoritative.

Historical Phase 0 checkout: `impl/graph-consolidation-20261110` at `16e77f6c6`,
with uncommitted changes and a stale remote-tracking reference. PR #1300 at
`516e0883875d5ec483e3595d50b77b3d5e8979c4` passed its configured checks and
is now the exact incoming merge head. Those checks do not accept the new merge.

### Phase 1: alternatives

- **Existing implementation unchanged:** rejected for this batch. Producer
  admission called `NetworkSession::inject` while holding the producer-state
  mutex; a custom readiness callback that re-entered `drain` could deadlock.
  Caller-thread-only session re-entry tracking also missed a joined dispatcher
  worker.
- **Smallest extension:** release the producer mutex after a `retired` check and
  then inject. Rejected because publication could win between the check and graph
  admission. A second check after injection would not undo a committed packet.
- **Initial proposal, reconciled:** perform admission without the producer inventory
  mutex, retaining the verified whole-drain gate across polling and eager admission.
  External publication waits for the entire admitted drain; publication from that
  same drain returns `LifecycleError::ReentrantDrain`. Host self-destruction marks
  retirement without waiting on its own gate, rejecting a late poll result; an
  admission already permitted before self-retirement can finish. The initial
  independent no-wait admission-permit policy is not the merged contract.
  Propagate a stable Kernel session identity using the existing process
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
stream and Kernel session passes belong to the initial candidate verification
record, not a rerun of the new merged head. This remains candidate-local evidence;
full exact-head CI and independent post-fix review are still required for the new
merge. All Kernel session calls fail fast on cross-thread contention, preserving
the verified contract; callback/worker re-entry also rejects. `lease_session`
always delegates to `try_lease_session`. The original
`concurrent_session_calls_fail_fast_without_kernel_lock_inversion` regression is
retained without a synthetic producer callback marker: an ordinary caller holding
a plugin mutex must not wait behind a session callback that needs that mutex.
`native_sessions::concurrent_session_calls_fail_fast_and_can_retry_after_lease_release`
expects busy while the lease is held and successful retry after release. Initial
waiter claims are historical and superseded, never an accepted Kernel lock cycle.
Plugin-created thread/join and external lock cycles remain outside the guarantee.

`ExclusiveSlot` is now only `Mutex<Option<T>>` with `try_take` and idempotent
release/RAII restoration, without Condvar or waiter bookkeeping. The revised
[session Loom verifier](../verification/fbp-session-loom/src/lib.rs) exercises three
bounded properties: competing `try_take` calls retain one exclusive owner; a busy
caller cannot steal ownership and can retry successfully after release; early
release is idempotent and publishes mutated state to the next owner. These are
current source/test obligations, not new-head acceptance inferred from initial
`0d145f3` passes.

## Lifecycle Policy Decisions

### C3: retired generation authority (decision required)

Current tested behavior distinguishes host ownership from explicit capabilities:

- External publication waits for the whole admitted producer drain before retiring
  host-owned polling. Its in-flight poll results may enter the old session before
  publication; no new drain polls after retirement. Same-drain publication is
  rejected rather than waiting on itself.
- Host self-destruction within a producer callback instead marks retirement and
  defers producer destruction until the enclosing drain/registration releases its
  locks. A poll result returned after that retirement is discarded. Admission
  already permitted before self-retirement may finish. Explicit session work does
  not block publication; this is not a no-wait guarantee for producer callbacks.
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

- **No change to the stale starting tree:** zero clock/ticket bookkeeping, but an
  accepted external job can remain pending indefinitely. This historical option is
  not a description of verified `516e088`, which already has the watchdog API.
- **Opt-in per-handoff deadline (reconciled with the existing stack API):**
  Only `SessionOptions.deferred_timeout` exists and defaults to `None`.
  `overdue_jobs(now)` is passive inspection; `supervise_deferred(now)` performs
  explicit host-driven mutation. `state(&self)` remains a passive C9 snapshot:
  it neither supervises expiry nor runs readiness callbacks. `run_until_idle` starts
  timing only after its dispatcher returns `Deferred`; public `next_job` starts at
  handoff. The deadline is accepted by the session, so work completed earlier but
  delivered after the deadline is overdue. Supervision invalidates the exact ticket
  and uses existing request failure policy; unscoped work fails the session.
  `RunReport::next_deferred_timeout` lets `Kernel::run_until` bound idle waits by
  the earliest outstanding deadline, but cannot preempt callbacks already running.
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
manual session is never driven; `state`/`request_status`/`request_work` remain snapshots
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

Under default `SharedFifo` there is one FIFO per compiled connection, shared
across request IDs. `head_for`
never skips a packet; it only selects a matching request at a connection head.
Public input admission atomically fans out to the compiled ingress queues. Each
export has one shared bounded capture FIFO by default. The verified stack already
supports authenticated request-specific extraction of a non-head buffered capture;
that does not reserve extra storage or free an indefinitely retained full queue.
The initial claim that `take_request_output` was always head-only was stale.
Each compiled physical process has one mutable
`ProcessHandoff`; a staged continuation or pending fanout is a serialization point.
`max_requests` bounds active request records, not all queue payload bytes or
application-private state.

### Phase 1: alternatives

- **Keep current shared FIFO:** preserves the strongest simple ordering contract
  and has no request-lane memory multiplier. Crossed arrivals and a slow capture
  can block later publication; this is correct backpressure, not fairness.
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
requires merged-head integration verification; the initial-candidate local passes
listed below do not discharge that obligation.

### Phase 3: implementation candidate

Candidate tests cover crossed join arrivals, a slow request capture with another
request completing, internal-edge routing, lane reuse after settlement and
cancellation, and aggregate packet-capacity overflow. The existing shared-FIFO
behavior remains the default, and its head-of-line regressions remain. These tests
do not prove arbitrary repeated-stream fairness or process-state isolation; H1/H7
remain partial, with their scores unchanged pending merged-head acceptance and
broader adversarial workspace and StorageGraph integration review.

## Post-Implementation Review

Merge-specific adversarial review also found and fixed resolved factory policies
escaping startup validation, `CorrelatedAll` falling through the lane adapter, and
leased readiness wrappers losing `notification_driven`. Their regressions live in
`fbp_merge_integration`. The original cross-thread Kernel lock-inversion test is
preserved without adding a synthetic producer-callback marker; all Kernel session
contention remains fail-fast.

Late timeout-invalidated dispatcher returns now detach and dispose completion/job
pieces with panic containment, holding code leases through all cleanup. A teardown
panic is appended to the originating request's failure, not attributed to an
unrelated replacement. The timeout suite exercises both return variants, both queue
policies, scoped/unscoped failures, cleanup order and replacement progress.

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
`SessionOptions` queue-policy fields and detailed invalid
closure declarations; `RequestError` is no longer `Copy`. Exhaustive downstream
struct literals/matches require migration. No compatibility shim is retained.
Timeout supervision reuses only `SessionOptions.deferred_timeout`,
`overdue_jobs(now)` / `supervise_deferred(now)` and `RunReport::next_deferred_timeout`;
the initial candidate timeout field is not a parallel API. C9 `state(&self)` stays
passive rather than becoming a supervision operation.
The request-lane example in `packages/phenix-graphs/API-EXAMPLES.md` is Rustdoc-tested.

### Verification Record

Local checks for the initial `0d145f3` candidate sources passed:

- Default workspace tests, Rustdoc examples, and compile-fail tests.
- No-default-feature workspace tests and graph-only no-default-feature tests.
- Strict workspace all-target Clippy (rerun after test-only lint fixes).
- Three bounded Loom properties over the initial production `ExclusiveSlot`
  variant; not acceptance for the revised try-only properties described above.
- Native request-lane, timeout, producer/session, worker-guard and generation suites.
- StorageGraph host queries under both queue policies, local/offloaded execution,
  observer drain, failure recovery, one-request credit reuse and retained-value leases.
- Local Nix checks, including `nix flake check --no-update-lock-file --print-build-logs`.

These passes are not acceptance for the new merged head. PR #1302's remote
workflows initially conflicted; manual
[Flake #43588157944](https://github.com/matthis-k/phenix-ai/actions/runs/43588157944)
targets initial commit `0d145f3`, not the final merge. **New full acceptance is
pending.** The accepted commit and GitHub run URLs must identify the exact new
head. Earlier local or PR #1300 runs must not be relabeled as its evidence.

The six-job Flake inventory remains the full gate, alongside all scoped gates
from the verified stack. The added path-scoped session Loom workflow is a separate
failure domain, not a substitute for the wake verifier, native concurrency,
benchmark or main/manual acceptance gates. The earlier documentation reconciliation
did not edit workflows. No new paired performance, allocation or RSS
acceptance results are claimed.

### Paired Benchmark Baseline Correction

The existing green H4 EOS endpoint, targeted wake, optional idle, scoped sparse,
H5 closure and H5 request-index evidence workflows used older baselines:
`43564cfacc6905e218fc49f11ba3485d63655668` or pre-index
`9911b42e3c0a3d84c7c60658090a51d426190dd1`. Those passes do not establish a
comparison against the immediately verified PR #1300 head. Their baseline checkouts
now pin exact `516e0883875d5ec483e3595d50b77b3d5e8979c4`; new paired runs are
required against that baseline. Byte-identical benchmark copying, hardware
methodology and flags are unchanged. Same-runtime polled/notified, mixed-agent and
allocation workflows remain mode comparisons, not before/after evidence, and are
unchanged. This correction claims no performance result or score change.

## Remaining Policy Boundaries

The initial no-wait publication proposal is explicitly superseded by whole-drain
retirement, as exercised by
[`generation_retirement`](../packages/phenix-kernel/tests/generation_retirement.rs)
and [`host.rs`](../packages/phenix-kernel/src/host.rs) tests. External
publication can wait indefinitely for a nonreturning producer callback; bounded
poll counts do not bound callback duration. All Kernel session calls instead fail
fast on cross-thread contention, as specified by the original lock-inversion test
and [`native_sessions`](../packages/phenix-kernel/tests/native_sessions.rs) busy/retry
regression. Waiting for a session lease while holding a plugin lock is not accepted;
this scoped guarantee is not universal plugin-lock deadlock freedom.
The initial no-wait/empty-return competing-drain policy is also discarded.
[`lib.rs`](../packages/phenix-kernel/src/lib.rs) retains manual-drain serialization,
typed `ReentrantDrain` rejection, and exclusive blocking-driver ownership. Its
`live_driver_exclusively_owns_polls_and_releases_manual_drain_after_exit` test checks
that even `drain(0)` returns `AlreadyRunning` while a driver owns polling;
`in_flight_direct_drain_denies_driver_without_registering_wake` checks the inverse
`DrainInProgress` rejection. These are source/test contract anchors, not new passes
reported by this documentation-only reconciliation.

The earlier `Kernel::run_until` Rustdoc error-attribution mismatch is fixed in the
main reconciliation: a manual drain encountering a live driver returns
`AlreadyRunning`; driver startup encountering an admitted manual drain returns
`DrainInProgress`. It is no longer an unresolved documentation contradiction.

Any stronger no-wait publication or arbitrary plugin-thread/lock deadlock-freedom
guarantee would need a separately approved policy/API change and new acceptance,
not a documentation claim. C3 strict revocation versus leased archival still
requires owner decision;
automatic C6 live-state migration and a C7 OS isolation adapter remain unimplemented.
