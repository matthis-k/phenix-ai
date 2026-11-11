# Phenix FBP Adversarial Critique

Status: baseline audit plus independent native implementation reviews. The M0
sections below refer to `32bbe60e6cd2ebd34a705486b4ef96e85245ee42`; the final
section records concrete implementation findings and regression fixes.
Paths below are relative to `packages/phenix-graphs` unless qualified. Decisions
refer to numbered sections of [the design](fbp-design.md). No numerical quality
rating is justified before implementation and regression evidence.

## Findings

### 1. Renaming The Existing Engine Would Not Implement FBP

`src/scheduler.rs:293-299` gives each execution session a wave-owned state.
`src/scheduler.rs:947-992` implements immediate-slot collision rejection and
unbounded deferred buffering, not bounded connection credit. Publication at
`src/scheduler.rs:1393-1430` rejects multiple emissions and nonzero output slots.
`src/lib.rs:2225-2237` gives a primitive exactly one output.

Design sections 1, 3, 5, and 7 must replace these authorities, not wrap them.
Required counterexample: inject twice into one live session, stop the sink with a
capacity-one connection, observe suspension, resume, and prove each effect ran
once. A session that internally calls `Engine::fire` twice fails this gate.

### 2. Fresh Factories Do Not Prove Absence Of Captured Shared State

The baseline already invokes factories per physical node in
`src/scheduler.rs:569-575`, and tests cover factory freshness. Some APIs deliberately
share actions. A new `spawn` method alone cannot prevent a factory closure from
cloning a shared mutex or store handle.

Design sections 1 and 11 remove implicit shared-action construction. Test two
instances and two sessions of one definition with mutable counters. Document
explicit shared capabilities. Do not claim Rust can inspect closure captures or
prove shared state has an authorized origin.

### 3. Output Retry Must Not Re-Execute A Process Or Lose Its Input

The blueprint's `try_recv` followed by `send` is unsafe if `Full` is simply
returned as an error: the process may already have consumed its input or changed
external state. A huge returned vector is equally unsuitable for bounded streams.

Design sections 2 and 5 choose one owned emission per cooperative step, retained
by the scheduler before further activation of that process. Test fanout with one
full destination, partial progress across several emissions, pure-preparation
failure, closed destinations, and cancellation while blocked. Verify retained
prepared values are dropped exactly once and retries do not redo preparation.
Private process state remains application-owned; a queue bound is not a universal
memory bound. Synchronous steps still cannot preempt a process that never returns.

### 4. Existing Executor Tickets Cannot Borrow A Mutable Process

`Executor` accepts `&dyn NodeAction` (`src/scheduler.rs:225-230`), while the new
process needs exclusive mutation. Delayed tickets are not a future-returning
executor. A borrowed session context cannot safely outlive the dispatch call.

Design section 2 proposes owned activation handoff. M1 must compile-check object
safety and Send constraints before runtime implementation. M4 must demonstrate
out-of-order completion across different processes, no same-process reentry,
wrong-generation and duplicate completion rejection, cancellation, budget stop,
abandoned jobs, and late completion drop. Keep executor customization and the
StorageGraph worker-thread query guard. Do not keep `NodeAction` as a second
permanent execution path to evade the ownership problem.

### 5. Public Projection Is Not Input Readiness

`src/authoring/operation.rs:927-1135` exposes aliases/fanout; it does not create a
shared readiness barrier. `src/lib.rs:1019-1089` contains real argument preparation,
not just schema descriptions. Actual slot frontiers can be independently selected.

Design sections 3 and 4 preserve reconstruction while mapping selectable leaves
to native ports. Migrate `module_projection_semantics`, `matching_projection`,
move-only layouts, explicit Arc fanout, structured local/wire inputs, and typed
compile-fail tests. A single opaque request port is not equivalent to a selectable
two-of-three slot policy.

### 6. Strict FIFO Can Conflict With Correlated Joins

The wave scheduler can search candidates by cohort. A connection FIFO in which
an unmatched cohort is at the head cannot legally deliver its later cohort first.
This is a real semantic tension, not a reason to create an invisible second
port-owned buffer or silently reorder packets.

Design section 3 requires head-preserving selection or explicit demultiplexing.
M2/M4 must inventory existing cases that select deeper candidates and provide
equivalent correlated adapters with bounded storage and explicit retirement.
Test crossed cohort arrival, optional alternatives, successful-completion-dependent
stages, fresh cohorts, and closed missing inputs. This proof is still outstanding.
Do not delete `CohortId`/color accounting until it is supplied.

### 7. Quiescence Does Not Close Streams Or Settle Correlation

`src/scheduler.rs:1664-1707` settles incomplete wave groups, including reservation
history after inputs were consumed. Persistent streams cannot use global idleness
as the same settlement signal: an external packet can still arrive.

Design sections 6 and 7 choose queue closure and explicit correlation settlement.
Specify terminal input handling, process finish behavior, wait-state precedence,
and history retirement before M4 passes. Completion must account for open external
sources, outstanding jobs/wakes, queued input, and publish-pending output. Test an
empty but open session, final close with stranded join, and a cycle whose queues
are drained but closure has not propagated. No global frontier framework is yet
justified by these cases.

### 8. Stable Replacement Must Revoke Handles Without Rewiring Edges

Cross-kind replacement currently rewrites node/module endpoint variants in
`src/authoring/substitution.rs:109-217`. Same-kind node replacement also compares
physical port/slot IDs (`src/authoring.rs:1174-1227`), more strictly than semantic
shape alone. A common wrapper around those variants removes neither problem.

Design section 9 replaces author-facing variants with stable symbolic endpoints.
Compare complete named interfaces and inherited explicit claims, compile the new
body, preserve adjacent symbolic edges, and revoke old handles. Migrate
`clone_lineage`, `operation_substitution`, private descendant request rejection,
`contract_dependency_identity`, and compiled generation binding tests. Shape
compatibility is not permission to erase an inherited obligation.

### 9. A New Interface Must Not Become Another Authority

The baseline has physical `Port`, `InputInterface`, `InterfaceShape`, address books,
compiler public projections, and `NodeSignature`. Some are necessary views; some
store overlapping declaration data. Their existence cannot justify deleting
preparation or restricted views, nor justify retaining every old representation
behind a new `Interface`.

Design sections 1, 3, 4, and 11 assign one boundary authority. During M1 review,
identify every surviving cached view and its invalidation/derivation rule. Remove
the pending mutable handle/layout store rather than cache contract checks against
it: `src/authoring/interfaces.rs:228-320` currently has to resolve live declarations.
Count all replacements, not just the size of the new facade.

### 10. Replay Cannot Be Guaranteed For Arbitrary Patch Closures

The current mutation observer (`src/authoring/observer.rs`) compares stored state,
including opaque action allocations; the revision log is explicitly incomplete.
The planner clones then observes (`src/authoring/patching.rs:756-808`). This protects
candidate-owned state, not captured external handles or side effects.

Design section 8 chooses owned edits and no core callback escape. Derive footprints
from edits, then prove authorization and conflicts for every variant. Removing
the observer is safe only after the public arbitrary-mutation path disappears.
Replacement writes the component, not all incident edges. Explicit rewires still
write their connection. Replay must not spawn factories or execute actions.

Dependency-ordered splice chains are simpler, but the current exact-position tests
demonstrate an independent-contributor use case. M3 must compare both approaches
before deleting `SplicePosition`; lexical tie-breaking is not a conflict policy.
Test defeat chains, missing/cyclic dependencies, ambiguous ordering, panic/error
atomicity, unload and transitive deactivation with no leftover connections.

### 11. Portable Identity Is Neither Native ABI Nor Semantic Membership

`src/lib.rs:236-248` distinguishes `TypeId` from canonical wire identity;
`phenix-schema/src/lib.rs` owns full descriptors. `src/interface.rs:340-359` and
`:819-930` require explicit semantic claims separately from complete shape.
The plugin manager is Rust-only, not a generic cdylib ABI.

Design section 10 preserves those separations. Preserve independent-consumer
compilation tests, mismatch cases for codec/revision/structure, forged local
labels, contradictory same-ID claims, and exact plugin-revision origin. Do not
let portable type lookup manufacture a semantic contract or authority.

### 12. Kernel Admission And Storage Overflow Are Different Bounds

`phenix-kernel/src/lib.rs:852-923` polls a producer and fires a synchronous wave.
`phenix-storage-graph/src/lib.rs:9-70` owns a pending storage notice tail.
`phenix-storage/src/lib.rs:1445-1495` deliberately replaces overflowing invalidation
queues with a rescan notice. Poll count is not FBP queue capacity.

M5 must retain one notice on blocked admission and stop draining that producer
until capacity/wake permits. Preserve round-robin fairness, subscription closure,
overflow provenance, subscribe-before-bootstrap, activation retry, and teardown
outside producer locks. Never turn storage overflow into silent packet loss or
claim the storage observer is durable event transport.

The existing producer mutex spans poll and execution. Persistent session mutation
requires deliberate synchronization, not holding a producer lock through arbitrary
action callbacks. Existing host retirement stops old producers while allowing old
explicit queries/waves; logical unload does not forcibly kill leased code. Any
stronger revocation rule needs a separately documented policy and tests.

## Abstraction Challenge

The design's section 11 is the proposed abstraction/deletion budget. Review every
implementation addition against four questions: which invariant needs this type,
why a surviving type cannot enforce it, which old authority disappears, and who
implements it. In particular:

- One `ComponentDef` plus an enum is preferable to two external component traits.
- One object-safe process contract is preferable to both activation and poll paths.
- Queue ownership and policy observation must not both hold transport payloads.
- Structured argument reconstruction may need existing traits; it is not a reason
  for another endpoint inheritance hierarchy.
- A dispatcher is justified by the existing delayed execution and guard use cases;
  it must dispatch the same activation contract as the local runtime.
- DFIR is a measured backend question, not evidence that a new backend is needed.

## DFIR And Evidence Limits

No backend experiment, throughput benchmark, or end-to-end FBP equivalence test has run.
The real comparison workload and acceptance gate are in design section 12.
A static sum pipeline alone cannot justify a backend for dynamic plugin patches,
stateful feedback, scoped storage, and cancellable owned activations.

The ordinary baseline tests skip manual performance fixtures. In particular,
`tests/inspection_cost.rs:26-36` instantiates packages without outputs, conflicting
with the current instantiated-module output rule in `src/lib.rs:4091-4097`.
This is a source-audit warning, not an executed failure or a new FBP regression.
Do not reuse historical inspection timings without validating those fixtures.

## Historical M0 Gate

The design is a direction with explicit proof obligations, not approval to claim
M1-M7. The largest unresolved items are exclusive-state dispatch, FIFO/correlation
equivalence, stream closure/progress retirement, and producer admission across
Kernel generations. Compile prototypes and regression tests must resolve those
before deleting their existing authorities. Do not leave permanent v1/v2 paths
or replace missing evidence with an architecture score.

## Independent M0 Review

A separate read-only review reproduced the headline 48,035 tracked Rust LOC and
633 lexical test attributes and rechecked the baseline/PR/CI provenance. It found
six design gaps: logical activation completion versus an emission step, owned
offloaded inputs, terminal reader/EOS transitions, correlation settlement
authority, crossed-cohort FIFO compatibility, and trusted patch revision/lease
binding. Design sections 2, 3, 6, 7, and 8 now specify those contracts explicitly.
These are documentation corrections, not experimental validation. The associated
compile prototypes and behavioral tests remain mandatory, especially the bounded
correlation adapter and fresh/staged request accounting.

## Initial M1 Probe Results

Two discardable standalone workspaces under `/tmp/opencode` exercised ownership
and correlation without adding a second runtime or public process API to the
repository. Commands and limits are recorded in the migration ledger. Their
source is local scratch evidence, not a checked-in regression suite or CI gate.

The owned-job probe passed eight unit tests and two compile-fail doctests with
the real `Value`: Send-only mutable process and prepared arguments cross a worker
thread; one outstanding job prevents reentry; publication commit is distinct from
logical `Complete`; foreign/duplicate/cancelled returns cannot replace live state;
panic disposal drops process/arguments once. This establishes an ownership shape,
not queue admission, wake races, cancellation supervision, or real plugin leases.

The lane probe passed ten tests including two-lane crossed-cohort routing, atomic
Full handling, bounded live scopes, stale tokens, closure and cancellation. It also
demonstrated deadlocks with insufficient scope credit or per-lane capacity and
hash-collision head blocking. No finite bound alone guarantees progress for
arbitrary ordered-source arrival skew. Fixed compiled lanes and shared exact
scope bindings avoid dynamic topology, but optional/staged settlement and
one-stateful-process lane arbitration remain production proof obligations.

Independent review of the production extraction found no actionable regression.
The semantic split removes old owner-module paths, while root facade reexports
continue to refer to the sole definitions. Bound promise lookup now uses the
structural snapshot accessors, with a small possible iteration/cloning cost;
no performance improvement is claimed. This review is not approval of full M1.

## Native Implementation Reviews

Independent read-only reviews exercised the compiler, composer, runtime and host
with additional probes, not only the existing happy-path tests. Actionable
findings were fixed and retained as native regressions:

- Mutable public packet values allowed payload substitution while retaining
  authenticated metadata. Payload fields are now private.
- Envelope-only leases could be detached by cloning/extracting a Value. Values
  and typed `Leased<T>` owners now retain leases independently; bare leased
  extraction rejects. Scoped input-record reconstruction pins leases on success,
  failure and unwind, including moving unique non-Clone payloads.
- Detached compiled networks/factories, directly spawned processes, callback
  errors and standalone handoff emissions could outlive exact plugin revisions.
  Each escaping executable/value/error ownership boundary now retains its lease.
- Early reader completion silently discarded accepted inputs, while impossible
  EOS joins could wait forever. Stranded inputs fail and disposal is accounted;
  discarded accepted input cannot be reported as successful completion.
- Optional closure could spin or resume a staged required group prematurely.
  Reservations require data or fresh closure, and required-group readiness is
  enforced before continuation. Proven impossible built-in groups fail explicitly.
- Unconnected outputs silently discarded emissions. Native publication requires
  destinations, explicit discard policy or an explicit empty selection.
- Wire declarations and codecs disagreed about overridden labels. Both now use
  the complete canonical schema identity.
- Moving a builder out of an aborted additions transaction escaped revocation.
  Transaction tokens follow moved builders, revoke their capabilities and block
  partial frozen builds; existing handles on the restored builder remain valid.
- The driver unwrapped foreign dispatcher returns. It now fails diagnostically
  and retains one recoverable refused object instead of panicking/dropping it.

The composer review and independent bounded reference models found no additional
authority/conflict defect within the exercised cases. The models cover 216
bounded dependency/priority cases, 512 generated replacement/rewire/export/revoke
decisions and ordered/ambiguous splice cases. They are not formal verification.
The source-ownership, structured-input and per-request recovery regressions also
pass after their integration fixes.

FIFO head blocking, conservative cyclic absence, cooperative cancellation and
application code that fails to yield remain real limitations. No DFIR experiment,
native performance baseline or arbitrary numeric quality score is claimed. See
the final report for verification and the regression ledger for changed syntax
and grouped replacement evidence rather than one-to-one historical equivalence.
