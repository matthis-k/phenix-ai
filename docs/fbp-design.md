# Phenix Native FBP Design

Status: the native engine replaced the old graph engine at `16e77f6c6`; Kernel,
Plugin and StorageGraph use native networks. The current lifecycle/request-lane
candidate is on `fix/fbp-lifecycle-request-lanes-20261114` (PR #1302), initially
committed as `0d145f3` after incorrectly relying on stale local remote-tracking
impl head `16e77f6c6`. Actual remote impl `89e00d0` includes later foundations.
After fetching, exact verified PR #1300 head
`516e0883875d5ec483e3595d50b77b3d5e8979c4` is now being merged to preserve
the complete stack and scoped gates; full new-head acceptance is pending. See its
[design/evidence record](fbp-hardening-campaign-20261114.md). The historical cutover
verification and immutable Nix snapshot checks are recorded in the report. See
[migration](fbp-migration.md), [review](fbp-critique.md), and the executable
[API examples](../packages/phenix-graphs/API-EXAMPLES.md).

## Historical Baseline

Inspected clean checkout `32bbe60e6cd2ebd34a705486b4ef96e85245ee42`, branch
`impl/graph-consolidation-20261110`. PR #1260 was rechecked using GitHub and still
had head `671956269a2655084ae90eb0b1e09e506177d71c`. The checkout is six commits
ahead of that reference. It already includes action/state consolidation and
checked matching projections; those changes must not be replaced with the older
PR implementation.

This paragraph records the initial inspection, not the present worktree. Exact
baseline measurements and CI provenance remain in the migration ledger.

## Implemented Model

The package remains `phenix-graph` in `packages/phenix-graphs`. It has one native
definition/compiler/composer/runtime path, not an FBP adapter over waves. Core has
no Kernel, Plugin, Storage or DFIR backend dependency. Distributed execution,
durable transport, live state migration and automatic cycle recovery are excluded.

| Authority | Current contract |
|---|---|
| `Interface` / `PortDef` | One canonical named input/output boundary; exact sets and identities, including zero ports |
| `ComponentDef` / `Body` | Primitive immutable factory or composite immutable `NetworkDef`, with explicit claims and origin |
| `NetworkBuilder` / `ComponentRef` | `node`, `module` and `add` return the same erased handle; typed endpoints are checked against actual scope, revision, direction and type |
| `NetworkDef::compile` | Validates and flattens nested projections to deterministic physical processes/connections with compile limits; no executable module wrapper |
| `Contract` | Explicit semantic membership, exact definition equality and restricted binding; matching shape never supplies a claim |
| `Composer` | Replays owned edits from frozen base plus trusted registrations, validates/compiles the candidate, then publishes atomically |
| `NetworkSession` | Persistent bounded FIFO transport, owned cooperative jobs, request accounting, captures, wakes and cancellation |

`ComponentRef<Ports>` was a proposal, not the implemented type. `InputKey<T>`,
`OutputKey<T>`, typed refs and `Ports` descriptors encode payload/direction and
names, not a generic owner proof. Keys cannot grant builder or runtime authority.
Typed `connect` unifies the Rust payload type. Independently declared portable
types use `connect_dynamic` and complete schema validation. Local `TypeId`, wire
schema name/revision/structure/codec, semantic claims and contributor origin remain
separate checks. Plugin snapshots are Rust objects, not a dynamic-library ABI.

## Process And Publication

`Process: Send` implements `step(&mut self, &mut ProcessContext) -> Result<Step,
String>`; `Sync` is not required. `ProcessFactory: Send + Sync` spawns fresh process
state per physical instance/session. Captured shared capabilities remain explicit
application sharing; factories cannot prove their captures are isolated.

`Step` is `Emit`, `Yield`, `WaitInput`, `WaitInputs`, `WaitWake`, `Complete` or
`Finished`. One logical activation retains its reservation across continuation
steps. `Emit` transfers one publication, not activation success; `Complete`
settles success. The process cannot resume while its emission is pending. No
unbounded emission batch, context-owned output queue, alternate futures API or
callback replay path is retained. Steps must return cooperatively; cancellation
cannot preempt application code or undo external effects.

`ComponentDef::source`, `map` and `sink` use this same path. A source creates a
fresh iterator and emits at most one item per step; `NetworkBuilder::start`
selects its instance explicitly. A map clones a borrowed local `T` (`T: Clone`)
once per activation, emits, then completes. A sink borrows `&T` and has no output.
Custom processes support move-only inputs and stateful continuation.

An `ActivationJob` owns the process and prepared inputs, not borrowed session
queues. `ProcessDispatcher` returns the same owned protocol locally or later from
an external runner. Tickets prevent foreign/stale completion and same-process
reentry. `take_dispatch_return` recovers a refused foreign completion/rejected job
after a dispatcher protocol violation; the session stays failed. Shutdown of
abandoned external jobs remains the runner's responsibility. Execution guards
cover delegated work, including StorageGraph worker-thread query execution.
Kernel's session lease is transferred without holding its mutex through a process
callback. The Kernel's stable session identity is propagated through the existing
execution-guard boundary so a synchronously joined dispatcher worker cannot wait
on its own session lease. This does not make arbitrary plugin-created threads,
resource-lock cycles, or non-yielding callbacks deadlock-free.

`SessionOptions::execution_guards` is one ordered Vec. Kernel identity is entered
first, then StorageGraph's host guard, then application/dispatcher guards.
`ActivationJob::with_guard` appends rather than replaces. Guard tokens are torn
down in reverse order and stay active through process panic-payload cleanup, so
application-token destruction cannot remove Kernel identity prematurely.

Only `SessionOptions.deferred_timeout` configures the opt-in cooperative watchdog
(default `None`). Existing `overdue_jobs(now)` inspects passively and
`supervise_deferred(now)` applies expiry to exact outstanding tickets. A direct
`next_job` starts timing at handoff; `run_until_idle` starts timing after its
dispatcher returns `Deferred`. Driver/completion operations supervise cooperatively;
`state(&self)` remains passive under C9, with no readiness callbacks or timeout
mutation. `RunReport::next_deferred_timeout` bounds the Kernel driver's idle wait,
not callback duration. Timeout cannot stop a worker, undo effects, or release its
owned payload/code leases; request restart may overlap that worker's effects.

Each connection is a nonzero-capacity FIFO. Publication validates the entire
selected fanout and prepares it once before committing to all destinations.
Full waits without repeating process effects or preparation callbacks. One
pending emission per process bounds scheduler retention; earlier committed
emissions survive a later failure. Queue item counts do not bound arbitrary
payload bytes or process-private allocations.

`ProcessEmission::new` broadcasts; `select` chooses existing input `PortPath`s,
including flattened module aliases, never new edges or process calls. Invalid,
duplicate, overlapping or unconnected selections fail atomically. Captures remain
observations even for an empty process selection. Unconnected emissions require
an exported capture, explicit discard permission, or intentional empty selection.
`Option`, `Result` and `()` are data, not control signals.

## Layout And Typed Ports

`#[derive(Ports)]` describes the same canonical interface as manual `PortDef`s.
Renamed/raw fields, generic records and nested `#[input(flatten)]` input records
produce selectable named leaf ports, not hidden grouped queue slots or another
interface authority. `InputGroup<T>` namespaces leaves; `InputRecord` and
`ProcessContext::input_record` reconstruct only reserved leaves.

The private `Value::map_local` helper
attaches source leases to the returned envelope and supports moving a unique
leased, move-only leaf. Shared move-only leaves fail rather than clone. Explicit
Arc leaves and encoded reconstruction retain source envelopes. This is scoped
reconstruction, not permission to detach raw payloads via callback side effects.
Typed Ports rename/nested-inputrecord and leased ownership regressions pass. Generic
flattening is limited to input-record leaves; arbitrary old recursive layout
syntax and output-record reconstruction are not promised.

## Closure, Requests And Recovery

Closing ingress/outgoing connections preserves accepted queued packets for drain.
EOS is empty-and-closed across all feeding connections, not a synthetic packet or
global idleness. Readers wake for EOS without a data reservation. Early reader
finish and impossible required joins fail/account accepted undelivered packets;
explicit disposal is not processing success. Pending fanout to a closed reader
fails without partial delivery. Cancellation invalidates tickets/wakes and
accounts disposed queued/pending packets; committed effects do not roll back.

`begin_request`, `inject_request`, `start_request_source`, `close_request`,
`request_status`, `take_request_output`, `validate_capture` and `release_request`
make request admission/closure explicit. Session/generation-qualified request IDs
cannot be manufactured from labels. Settlement accounts for queued, reserved,
dispatched, continuation and pending-publication work, not just the first capture.
Capture identity includes the exact export and actual producer origin.

Default shared-FIFO head-only correlation preserves FIFO, including head blocking
on crossed requests. There is no silent deeper-candidate search or general
arrival-skew equivalence.
Request-local optional absence and staged closure are tested against conservative
reachable upstream-work accounting. A live required producer must not be replaced
by optional EOS; exhausted required groups fail. General cycle absence/progress
proofs and a production arbitrary-skew demultiplexing guarantee remain pending.
The candidate contains an opt-in fixed-lane composite adapter.
Its explicit scope-skew bound covers distinct request identities, not repeated
packets from one request. Its demonstrated independent progress is restricted to
finite joins with at most one packet per request/input before an incomplete join
becomes runnable. More repeated packets can fill a lane and intentionally retain
head-of-line blocking; strict FIFO is preserved and no arbitrary-stream fairness
claim is made. This remains a bounded adapter, not a general demultiplexer.

`SessionOptions::request_queue_policy` defaults to `SharedFifo`, preserving shared
connection and capture ordering. Opt-in `BoundedLanes` lazily allocates one bounded
FIFO per active request for each compiled connection, public ingress edge, and
capture alias. A lane is reused only after request settlement/cancellation and
release; stale ticket validation prevents an old worker from publishing into a
reused lane. The total packet-slot envelope is checked as
`max_requests * (sum(connection capacities) + sum(ingress capacities per target) +
capture_capacity * exported_capture_count)`. This bounds packet counts, not payload
bytes. It does not create request-local process state: a staged continuation or
blocked atomic publication still owns the one physical process handoff, and a
nonconsuming reader can fill its own capture lane. The guarantee is queue-level
cross-request isolation, not unconditional request progress or fairness.

`RequestFailurePolicy::FailSession` is the default. Opt-in
`RestartAffectedProcess` respawns affected active process state on request failure
or cancellation, without resetting unrelated state, resources or external effects.
Budget exhaustion is resumable. `SessionState` (not proposed `NetworkState`)
distinguishes `Runnable`, `Quiescent`, `Waiting`, `Completed`, `Cancelled`, `Failed`.
Open idle ingress is not completion. Physical cycles are legal; recursive
definitions are not. Blocked queue/cycle diagnostics are not progress guarantees.

## Composition And Replacement

`NetworkPatch` owns immutable `Edit`s, not callbacks. Trusted host `Registration`
separately supplies ID, dependency set, integer priority, exact origin and grants.
Only root public declarations are editable; private nested writes reject even
with an exact-looking grant. Grants name a component, exact connection endpoints,
directional export or source start; they never imply incident edges, descendants,
namespace prefixes or semantic membership. Payload origins, including nested
definitions, must be absent or exactly the registration origin.

Dependencies must exist and form a DAG. They order writes; independent overlapping
writes require unequal priorities, otherwise reject. Higher-priority selection
defeats the losing registration as a whole; defeated prerequisites deactivate
dependents, which cannot suppress eligible independent patches. Independent
splices of one anchor remain ambiguous even with priorities. Ordered splices
require explicit grants for the anchor and every removed/generated edge.

Replacement changes the definition at a stable symbolic path, preserving adjacent
edges/exports but revoking old authoring revisions. Full public interface equality
and inherited explicit obligations are required in both body directions. Every
Add/Replace checkpoints claims/private requirements, even within one patch.
Candidate errors/panics cannot publish partial topology or registry changes.
Replay never spawns factories or executes processes. `Composer::revoke` recomposes
from base and deactivates transitive dependents, not inverse-edit callbacks.

Kernel binds selected plugin revisions, approved grants, exact offers/imports and
leases outside patch payloads. The verified whole-drain retirement gate and
exclusive blocking-driver ownership remain: external publication waits for the
whole admitted producer drain, not explicit session work; same-drain publication
is rejected. Host self-destruction marks retirement and rejects a late poll event,
while destruction is deferred outside locks. All Kernel session calls fail fast
on cross-thread contention with busy/`WouldBlock`; same-kernel callback/worker
re-entry also fails fast. Retry after release preserves the verified lock-inversion
contract, not the initial, superseded waiter policy. `ExclusiveSlot` uses only
`Mutex<Option<T>>`, `try_take` and idempotent release, without Condvar or waiters.
A caller holding a plugin lock must not wait behind a callback that needs it.
Retirement does not unload code retained by old compiled handles, sessions or jobs,
nor revoke explicit old-generation calls.
`Kernel::drain` returns `Result<Vec<ProducerEvent>, DriverError>`: manual drains
serialize, recursive drains reject, and a live blocking driver excludes manual
polling even for `drain(0)`. The initial no-wait/empty-return policy is discarded,
not layered over the verified driver guarantees.
StorageGraph preserves one bounded pending producer notice, explicit overflow/
rescan semantics, subscribe-before-bootstrap, exact query origin and dispatch guards.

## Leases And Inspection

Compiled clones, detached factories, directly spawned processes, jobs, emissions,
captures, typed payloads and retained producer/plugin failures pin their code and
application owners through destruction. `Value` representation and `Packet`
payload mutation are private. `borrow` exposes only a borrow; owning typed
extraction uses `Leased<T>`, not a bare `T`/`Arc<T>` escape. Private reconstruction
preserves leases on success, error and unwind. Cloning application data through a
borrow remains the application's responsibility.

Captures are bounded FIFOs (`SessionOptions::capture_capacity`) that must be
drained to release publication credit. Global `take_output` stays FIFO;
authenticated `take_request_output` can extract that request's first already-
buffered capture behind another request, preserving within-request FIFO. It cannot
bypass publication blocked by an occupied full queue. Native metadata inspection
does not spawn factories. `RunReport` exposes state, steps, publications, disposed packets,
discarded emissions and budget exhaustion, with queue/wait/last-step diagnostics.
Legacy wildcard/full-payload tracing, wave reports and exact baseline report
counters are not retained. No unbounded trace history, complete event ledger or
performance-equivalent telemetry is claimed; those telemetry limits are accepted.

## References And Evidence

Morrison FBP motivates stateful processes and bounded FIFO packets; NoFlo motivates
one primitive/composite boundary; NiFi motivates capacity diagnostics; Timely
motivates explicit work accounting, not invented global frontier guarantees.
DFIR inspired validate-once lowering, but no DFIR backend dependency or experimental
spike was adopted. The native engine is the chosen implementation. Historical
scoped measurements remain in the [report](fbp-report.md) and verified stack;
no new merged-head throughput, allocation or performance acceptance is claimed.

The [regression ledger](fbp-regression-ledger.md) records M6 removals and named
native tests, including independent randomized composition models. Final counts
belong to [measurements](fbp-measurements.md), not estimates here. Remaining gaps
and final verification status are recorded in the migration/review documents.
The [33-issue tracker](fbp-issue-progress-estimates-20261113.md) is authoritative:
all scores stay at verified `516e088`, with no new increases or closures. M7/M8
and missing production-agent, live-migration and OS-isolation adapters remain
incomplete; queue isolation is not independent process state or universal fairness.
