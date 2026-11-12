# Graph consolidation design

Baseline: PR #1261, fetched head `d758291c1db77abcff60ac3ac0e167c0f59261bd`.
Its exact-head CI failed formatting in operation.rs and matching_projection.rs;
the other two shards passed. Historical checks are not evidence for this branch.

## Batch A: action lowering

### Phase 0: problem and constraints

lib.rs has six private NodeAction adapters: typed, contextual, encoded plain,
local atomic, local structural and multi-port variants (typed/contextual repeat
the same body). Their differences are preparation and return conversion, not
execution authority. Leaving them separate requires each new path to reproduce
invocation/lowering. NodeAction and external completions must remain object-safe
untrusted Vec<Emission> transports; scheduler validation remains authoritative.

### Phase 1: design

Delete the private action structs. Reuse the existing closure implementation of
NodeAction with one private generic function composing a preparation closure and
an invocation/return-conversion closure. It owns the context, prepares through a
mutable borrow, then calls the handler with the remaining shared context. The
converted Option<Emission> becomes the transport vector at this single boundary.
No new public trait, node kind, erased intermediate, or dynamic dispatch is needed.
Existing LocalReturn and NodeOutput express return conversion. Typed constructors
delegate to the contextual layout constructor rather than duplicating its logic.
Factory calls still construct fresh handlers per physical instance. Multi-port
dispatch remains a heterogeneous port adapter, not a second action executor.

Local structural preparation still consumes raw selections only after reservation
and uses the declaration's frozen slots. Encoded arguments remain prepared before
admission. Option/Result payloads, source triggers, suppression and routing do not
change. Plain and directed sources use the same lowering helper where practical.
No public migration is required. Tests: existing typed/contextual/layout/factory,
multi-port, source, output, invalid completion, cancellation and codec tests.

Phase 2 revisions: preparation here means extraction from an admitted context;
encoded decoding/validation remains in the port-owned pre-admission builder.
The helper returns an impl NodeAction from `prepare(&mut InvocationCtx) -> A`
and `invoke(A, &InvocationCtx) -> Result<Option<Emission>, String>` functions.
It imposes no Clone/Sync bound on A. Constructor mapping: function/flattened/
with_input_layout extract prepared A and emit encoded O; typed/with_layout and
typed_factory extract prepared A and convert NodeOutput; local_function takes
raw slot 0 and converts LocalReturn; local_with_layout consumes raw selections
with frozen slots and converts LocalReturn; multi_port selects its registered
dispatch and converts NodeOutput; sources prepare unit without adding a port and
convert LocalReturn. Factory creation remains inside the physical action factory.

## Batch B: authoritative scheduling state

### Phase 0: problem and constraints

CompiledGraph stores identical edge vectors in by_node and by_output despite
requiring canonical output slot zero. WaveActivity maintains counts, nodes and
active_partitions used only by diagnostics/tests, alongside the producer and
pending indexes that actually drive scheduling. These are duplicate consistency
obligations, not useful policy power.

### Phase 1: design

Keep one outgoing adjacency and reuse it for node traversal. Preserve edge order,
sparse node IDs and cyclic ordering fallback. Remove unused eager activity
aggregates; derive diagnostic assertions from authoritative buffers/producers
without removing their test intent. Keep dispatched tickets separate from admitted
producers, pending partition acceleration, reservations and settled colors.
If straightforward, consolidate selected/retired buffer removal and source/port
admission updates, without moving validation or transaction boundaries.

Do not introduce Decision: select proposes picks, retire explicitly discards
alternatives, incomplete describes terminal obligations including reservation-only
stages. A Wait/Invoke/Fail replacement alone cannot express these independently.
Keep policy expressiveness and engine-owned atomicity. Tests must preserve staged
readiness, custom picks, rejected cohorts, color filtering, skipped destinations,
exact cyclic reachability, ticket lifecycle and independent waves.

Phase 2 revisions: retain by_output, remove by_node and its construction and
traversal reads. Invalid OutputAddr slots therefore remain invalid. Diagnostic
state must scan actual nonempty buffers and admitted producers; independent
transition assertions also compare ready/dispatched work and pending indexes.
Do not consolidate admission or buffer-removal in this batch: these are separate
possible follow-ups requiring their own ordering design and critique.

## Batch C: checked construction mutations

### Phase 0: problem and constraints

Module::expose omits existing-export identity checks already present in
expose_matching. Custom patch footprint observation visits after-only declarations
and additions-only routes, so destructive edits can evade conflict detection.
This is conflict accounting, not kernel authorization. Candidate-clone rollback
alone cannot fix a successfully admitted unobserved write.

### Phase 1: design

Reuse existing projection validation for manual and bulk projection, preserving
bulk full preflight before commit. Reject incompatible existing exports before
mutation. Complete change observation or fail closed to Graph for changes the
existing vocabulary cannot represent, including removals/configuration changes.
Keep the existing patch planner and atomic candidate commit. Do not add an IR.
Tests reproduce destructive empty-footprint patches, same-name routing changes,
and projection mismatches, asserting unchanged graph metadata/topology on failure.

Phase 2 rejected the patch-observer design as insufficiently specified: symmetric
conflict overlap is not declared-write coverage. That portion is blocked pending
a field-level observation design and second independent review. Projection
preflight reuse is independently accepted and can proceed without observer edits.

## Ownership alternatives and scope decisions

Fluent configuration requires Weak access to a builder-owned Rc<RefCell> pending
map, frozen address books and revision tokens. Pre-registration Node::with_layout
already handles one-shot configuration and move-only structured arguments, but
removing fluent edits would remove existing registered-declaration mutation and
custom LocalLayoutRetarget behavior. Preserve that expressiveness here rather
than trade it for shorter syntax internals without a complete replacement.
Address books are frozen selection metadata; declaration revisions revoke stale
capabilities. They are not independent compatibility authorities. Consider small
internal consolidation only when it removes an actual duplicate owner.

Operation already supplies the shared typed Node/Module boundary. GraphUnit is
authoritative inspection, including output-optional roots; merging the two or
adding Unit would obscure ownership. Keep explicit projections and compile-time
expansion, independent child readiness and the existing cross-kind replacement.

Local and encoded markers encode genuinely different compatibility relations.
Generics end at authoring; actions/policies erase at heterogeneous collections.
Canonical schema identity remains Graph-owned and does not certify codec honesty.
Deterministic manifests describe declarations, not captured closure behavior; a
new behavioral identity mechanism would be speculative in this consolidation.

These scope decisions preserve the stated use cases and invariants; they do not
accept reduced graph expressiveness. Affected costs remain dynamic stale/frontier
checks, pending declaration staging and retained policy history. No new execution
budget, DAG restriction, native policy, plugin matcher or runtime module is added.

## Protocol

Independent critique is recorded separately before implementation. Each batch's
actual diff review, regression evidence, measurements and remaining opportunities
must be recorded after implementation. Exact final commit checks and remote CI
belong in the PR delivery evidence, not inferred from baseline results.
