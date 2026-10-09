# Declarative workflow topology

status: partial
scope:
  - reusable control-flow compilation and execution
  - provider-neutral agent-loop topology
  - Basic and Advanced composition

## Architectural contract

The kernel resolves compatible providers and owns dispatch, authority, generation
identity, Service Layers, Events, cancellation and lifecycle. Domain plugins
declare workflow topology and supply implementations of its named contracts.

A shared contracts package defines interface names and request/response types.
It does not install providers or impose an agent loop. A topology plugin declares
nodes, typed outcomes, transitions and entry points. Basic selects inexpensive
terminal providers. Advanced changes provider bindings, adds tools, Layers and
Event Listeners. Neither profile requires a different loop implementation.

A workflow node references a typed declared `InterfaceId` import, never a
concrete provider or its `ServiceId`. Node identity and import identity are
independent. A node implementation may come from a separate capability plugin
such as memory or compaction. Core compiles each import directly against the
owner's resolved component graph, with no string reparse or second resolver.
Provider and Layer selection remains in that canonical generation resolver.

## Implementation in this PR

Core now has an agent-independent `WorkflowTopology` and `CompiledWorkflow`.

Compilation validates:
- the entry exists and every node can be reached;
- every branch names an existing node or a finish edge;
- every node has declared, nonempty outcomes;
- service-backed loops may intentionally omit an exit; cancellation and opt-in limits govern their lifetime;
- each referenced service is bound, as verified by the caller against its
  resolved component imports.

Execution carries caller-owned state across node calls and follows declared
outcomes. `RootExecutionHandle::execute_workflow` takes request and response
adapters supplied by the caller. It invokes selected provider imports through
`RootExecutionHandle::invoke_import`, applying kernel Service Layers and
attenuated authority within the pinned root generation.

The executor does not select fallback providers or retry calls after failure.
Cancellation is checked before each call. Step limits are optional. The agent
adapter rejects any further node dispatch after a terminal response and checks
that the compiled workflow's exit matches the terminal response kind.

The provisional `WorkflowTopology::compile` callback and unbound `CompiledWorkflow::execute` dispatcher are internal/test-only. Compiling against resolved component imports and executing bound nodes are Core-private. External callers run a workflow through `RootExecutionHandle::execute_workflow`, which pins the generation and performs the canonical authority and Service Layer dispatch. This narrows the transitional API; it does not implement the RFC's final `Invoke/Fork/Join/Exit` IR.

The harness builder accepts `WorkflowDeclaration` through `add_workflow`.
`ResolvedGeneration::with_workflows` compiles declarations against the
canonical component imports before activation. Each compiled service node retains
its selected provider import handle. The declarations contribute to graph
generation identity and remain bound to the same generation during execution.
Import-only components may belong to resource-only topology plugins. Those
components cannot export executable services or install listeners. On a
candidate plugin-set replacement, an existing workflow is retained only when
its exact owning component and plugin manifests remain selected. Reusing a
component ID or changing its plugin revision retires that workflow until the
new artifact republishes its definition. This is a fail-closed transition,
not artifact-based discovery or a complete hot-reload mechanism.

The runnable kernel fixture checks model, tool, model, and finish under two
provider selections. Advanced adds a logging Service Layer. Both use identical
topology bytes. It also checks that a root rejects an import handle selected
by another generation.

A separate `phenix-plugin-agent-topology` crate now declares the standard
two-node turn graph. `AgentTurnStepInterface` handles one admitted model turn
and returns final, tool-calls, cancelled or failed. `AgentToolBatchInterface`
handles a complete batch, including continuation updates, and returns continue
or cancelled. `AgentTurnState` carries usage, observations, seen tool-call IDs,
active tools and model continuation across these providers. These contracts
live in `phenix-sdk`.

The topology package is available for explicit selection through the first-party
builder and is now a removable default of Basic Agent, inherited by Advanced.
Both profiles now select the same declarative topology and its Basic node
providers without automatically selecting the legacy loop. The legacy agent
contract remains explicitly selectable and authoritative when present.
Product-level tests require one compiled workflow in each profile and prove
provider defaults can be replaced or excluded.
The topology and Basic node providers do not depend on the legacy loop
plugin: they import invocation, control, tool-execution and progress interfaces
supplied by separately selected service providers. Excluding the legacy loop
therefore retains declarative defaults. Product tests prove Basic can resolve
and activate the declarative topology with no legacy loop installed, and that
foreign agent-loop substitution remains compatible. An independent embedded
turn provider can replace the Basic turn implementation through the canonical
`AgentTurnStepInterface` binding, execute a real model-turn workflow to a
typed final response, and retain the ordinary Basic tool-batch implementation
without loading `phenix.agent-loop`. The real declarative execution regression
suite likewise runs model/tool continuation parity without installing any
legacy agent loop. The ordinary selected Basic/Advanced profile now enters the
declarative application route, while `with_default_suite` and explicitly
selected legacy agent providers continue to use the legacy-compatible contract
during parity migration.
Selection requires both providers at resolution. The independently
packaged `phenix-plugin-basic-agent-nodes` supplies initial working
turn-step and tool-batch providers. They share agent policy, descriptor
validation, tool-call identity, and activation invariants through `phenix-sdk`.

The topology package also provides `run_agent_workflow`, a typed adapter
around the generic root workflow executor.
The Harness exposes `PhenixRuntime::run_declared_agent_workflow` as an
explicit entry over the active pinned generation. The application chooses its
execution contract before prompt admission. Basic and Advanced default to the
declarative route when no agent-loop contract is bound; explicitly selected
legacy providers still use the legacy route. Execution failures do not trigger
fallback to another route. A product-level
test activates Basic and Advanced and verifies cancellation occurs before
a node can invoke the model, with zero tool and model usage. The bundled CLI
has a regression asserting all expanded product defaults exist in its
first-party plugin manifest catalog, preventing silent profile/catalog drift. It transfers `AgentTurnState`
between the declared nodes, projects response variants onto declared branches,
and returns the existing `AgentLoopResponse` contract. It introduces no
additional model/tool implementation, retry policy, step cap, or timeout.
The Basic and Advanced selected product profiles now use the declared workflow
by default. This default migration is subject to full CI and product parity,
not evidence that the final Core IR or all cancellation semantics are complete.

The typed adapter additionally enforces cross-provider state ownership. Turn
providers cannot rewrite original input, previous tool continuations,
observations, tool catalog, or tool execution usage; tool IDs must remain
unique across all turns. A final model response cannot invent unexecuted
calls. Tool-batch providers must preserve model call usage and admitted IDs.
Successful batches must append exactly one continuation matching the pending
assistant output and ordered call/result identity, with consistent tool usage.
Replacement providers cannot silently omit results or forge a completed tool
execution. Cancellation remains terminal and never resumes partial batches. Cancelled tool-batch
providers must preserve continuation history and unrelated observations, keep
previously active tools, and report no more completed tool calls than the
pending batch contains. Partial tool usage is admitted, but a cancellation
cannot claim a completed model continuation. Changed observations must validate
and identify an admitted call from the pending batch; unrelated observations
remain unchanged, and existing observations cannot disappear. Each newly
changed observation consumes at least one reported completed tool call and
must refer to the completed prefix of a partially cancelled batch, not a
later pending call. A cancelled batch reporting zero completed calls cannot
activate new tools. The adapter also
checks model-call accounting by typed turn outcome: successful turns increment
once, cancellation and pre-invocation turn-limit failures do not increment,
and tool-call-limit failures account for their admitted model turn.
Pre-model model-turn-limit failures cannot introduce tool-call IDs and must
report a positive limit already reached by the recorded model usage. A
post-model tool-call limit failure must report a well-formed limit violation
and a canonical set
of newly admitted IDs whose size matches the declared actual call count.
These rules are tested as negative provider-substitution cases. A separate
application transport regression exercises the declarative route with a
substituted provider, including cancellation, final output, and journal state.
Full live-product streaming, event, and side-effect parity remain open gates.

The old Rust agent-loop implementation remains active for parity comparison.
A regression fixture using the *same existing mocked invocation and tool
providers* now runs the declarative Basic path and checks final responses,
tool continuation, activated tool descriptors, progress and cancellation
against the legacy path. These tests have not yet passed a full CI round.

The agent adapter still owns turn and tool-batch state checks. Core now
validates selected portable outcome projections against the canonical bound
service result schema and declared edges, freezes their canonical data in
generation identity, and checks the result before invoking the Rust adapter.
The adapter must agree with the selected branch. A mismatched branch fails
instead of choosing either adapter or projection output silently. General
plugin-artifact discovery and full portable plan activation remain separate
gates.

## Remaining migration work

Workflow declarations now enter a resolved generation through the harness
builder and its canonical component imports. Automatic discovery from arbitrary
plugin packages has not been implemented. Basic and Advanced select the
declarative application route by default, while explicitly bound legacy agent
providers retain the old driver. Full behavior parity and removal of legacy
hooks remain open gates.

The next integration must:

1. Discover workflow declarations from plugin artifacts, including resource-only
   topology plugins, without product-specific builder registration.
2. Complete portable contribution-based selection of versioned outcome
   projections. The first-party topology now binds them through the builder,
   with canonical startup validation and runtime enforcement. Artifact
   discovery and all portable entry authoring remain incomplete.
3. Replace the legacy agent-loop entry with the declared workflow after
   validating and closing gaps in cancellation, provenance, usage,
   tool-call identity, observation projection, continuation, error
   classification, streaming/progress and side-effect semantics.
4. Reuse the existing step runner and tool executor as bound implementations.
   Extract responsibilities only when ownership is clear.
5. Bind the real Basic and Advanced product profiles to the same declared
   agent loop. The current fixture already verifies provider replacement and
   logging Layer composition without changing topology.
6. Remove the legacy `phenix.hooks` default after its consumers migrate to
   Service Layers and Event Listeners.
7. Expose the resolved workflow and its chosen providers through runtime
   inspection, including the owning plugin and graph generation.

## Required acceptance tests

- Kernel-only and non-agent workflows execute without loading any agent crate.
- Startup rejects missing required providers, invalid transitions, unbound
  outcomes, and incompatible or unauthorized bindings.
- A loop with a conditional branch repeats and terminates correctly.
- A non-agent fixture runs the same topology under Basic and Advanced bindings
  and the Advanced logging Layer. Added; CI verification required.
- The real Basic and Advanced agent products run the same topology.
- A third-party context or model provider replaces the Basic one without an
  agent-loop source change.
- Adding a Service Layer changes only the resolved invocation chain, not the
  topology. The new fixture covers this.
- Adding an Event Listener changes only subscriptions, not the topology.
- A failed tool call is not implicitly replayed after provider fallback.
- Cancellation and progress events retain the root execution generation.
- Plugin removal retires all associated topology, Layers and subscriptions.
- Both old and new agent-loop paths pass the same model/tool/session fixture
  before the old path is removed. Shared fixtures added; CI pending.
- Basic node providers are independently packaged and run without importing
  executable code from the legacy agent-loop plugin.

## Guardrails

Workflows describe transitions, not arbitrary expressions or scripts.
Domain policies, retries, budgeting, maintenance and worker orchestration
belong to providers. The kernel handles one-shot continuations, authority,
failure propagation and lifecycle. A looping control-flow edge is valid;
a cycle in required provider imports remains invalid.

Existing `spec/runtime-topology-generation.md`, `spec/kernel-hooks.md`,
and `spec/composable-agent-runtime.md` remain authoritative for dispatch,
hook and profile semantics.

## Downstream composition boundary

This PR establishes provider-neutral **declaration and execution** of one
workflow. It does not yet compose structural modifications from separately
installed plugins. Do not mistake these distinct concerns:

- #726 declares nodes, service bindings and outcome transitions and validates
  the selected execution path.
- #728 defines the portable typed contribution envelope and static plugin
  authoring used by future graph declarations.
- #729 implements conflict-aware graph patches, stable edge/slot identities,
  explicit ordering/override policy and provenance. It depends on both #726
  and #728 and must not duplicate #726's provider resolver.
- #730 enables selected plugins to define new contribution kinds and lowering
  templates; its SDK metadata may evolve in parallel with #729 only after
  #728's envelope is stable.
- #731 migrates tool/skill consumers and proves that generic Core execution
  works without agent plugins; it follows #729 and #730.

Avoid adding AI-specific patch semantics to the Core workflow compiler merely
to close agent migration gaps. A per-tool invocation subworkflow and tool-facing
triggers should be expressed through generic graph operations when those
operations arrive. Until then, this PR remains a standalone generic workflow
foundation with explicit first-party topology wiring.

### Read-only workflow inspection

The resolved generation inspection now includes selected `WorkflowDeclaration` values and `workflow_node_provider(owner, workflow, node)`. The latter reads the same canonical resolved component import handle that the workflow executor uses, rather than independently choosing a provider. A regression fixture verifies the selected provider and missing-node behavior without starting a plugin. The inspection exposes declared topology and provider bindings, not yet a full contribution-level graph patch provenance view (owned by #729).

## Conditional live application dispatch (partial migration)

Application execution resolves its provider from the prompt's pinned generation
**before** journal admission. If the application import binds a compatible
`AgentLoopInterface`, it invokes the selected service and never silently
switches to the declarative graph. If no agent import is selected but a compiled
`phenix.agent-topology / agent.turn` entry exists, the same root executes the
typed workflow. An absent pair fails before the prompt is admitted. Execution
failure on either branch is terminal rather than a retry into another provider.

The declarative branch checks the same cancellation flag between graph nodes,
uses no implicit step cap, shares the same session-qualified command and
application tool adapter, and projects the typed terminal response through
the existing application session journal. Basic/Advanced still run the
legacy-compatible agent service when explicitly bound; the profile default
selects the declarative route instead.
The transport conformance fixture selects `phenix.api` and its `phenix.options` interface provider explicitly alongside Basic without needing a legacy exclusion. SDK contribution resolution requires those independently packaged plugins. Disabling the `AgentLoopInterface` provider while retaining the declarative entry yields no legacy-agent binding and selects the declarative path; rejection is correct only when neither execution entry exists. A channel-transport test now covers
application prompt admission, typed provider-initiated cancellation, subsequent
successful prompt submission, and completed response projection through an
independently authored turn provider with no legacy loop installed. Both
terminal states must appear in the durable session journal.

This is **partial integration**, not complete parity. Live progress stream
semantics, cancellation during effects, tool-error continuation, generation
promotion during a prompt, and durable crash/retry behavior need product-level
conformance before retiring the legacy path.

## Compile-time subplan inclusion

The selected workflow compiler now expands `WorkflowEdge::Include` within one
component owner's selected definitions. Each inclusion has a stable site ID and
an explicit mapping from child finish outcomes to parent transitions. Nested
inclusions get qualified node IDs. Missing children, recursive references,
duplicate sites, conflicting node IDs and incomplete mappings reject candidate
compilation. The expansion has fixed preparation-time depth and node-count
bounds. The executor still sees only Invoke and Exit steps and uses the same
pinned imports.

This is a partial implementation of the RFC subplan rule. It does not yet
provide typed input/output frame mappings, public slot identity, cross-owner
selection, or patch integration. The compiler version is included in generation
identity so an earlier compiled plan cannot be confused with this revision.

## Typed execution frames

Core now exposes versioned `WorkflowFrameSchema` and `WorkflowFrame`
values. Selected workflow owners attach a frame contract using
`ResolvedGeneration::with_workflow_frame_schemas`. Candidate preparation
checks the target workflow and schema, rejects duplicate declarations, and
includes the selected contract in generation identity. A frame-backed root
rejects an absent or mismatched contract before invoking a provider.

Frames are copy-on-write data snapshots with explicit initialization and
typed slot updates. Recursive validation excludes live callable and object
references, including values hidden under `Any`. Copying a frame never
copies host authority. `collect_from` selects named branch outputs,
checks schema identity, and rejects duplicate selectors. Failed response
projection reverts that node's frame edits without replaying side effects.
The ordinary workflow API is unchanged.

Core also defines the closed join settlement policies in `WorkflowJoinPolicy`.
The decision function accepts the admitted child identities and the scheduler's
monotonic settlement order. It validates duplicate, missing and foreign
observations, breaks simultaneous ties by child ID, handles early failure and
quorum impossibility, and reports when outstanding siblings need cancellation.
Core now lowers fixed named forks and bounded list map forks into the
same compiled `Fork` and `Join` step vocabulary used by the provider-neutral
`Invoke` and `Exit` dispatcher. Each admitted child runs on a copy-on-write
frame snapshot. Core advances one Invoke per runnable child in deterministic
round-robin order, reuses the root's pinned imports and authority, and checks
cancellation and optional step limits across branches.

The closed Join policies settle **in the same scheduling turn** as the child's
final Invoke, before an unnecessary sibling dispatch. A declared child `Fail`
is a normal, typed settlement; provider errors, denied authority, invalid
outcomes and adapter failures propagate as execution errors and never enter
an ordinary Join failure continuation. The parent receives only declared,
schema-checked branch outputs or a typed list of mapped results. Map admission
validates the explicitly authored positive fan-out bound and typed collection;
there is no implicit 256-child Core ceiling. Empty maps settle vacuously for
`All` policies without invoking child providers.

This remains **cooperative execution**: at a Join boundary there are no
in-flight child invocations. A root's generation lease covers each child
throughout dispatch. A recursive cursor scheduler now admits nested Fork/Join
scopes without starting new roots or selecting new providers. Each ready scope
receives at most one Invoke per scheduler turn, including when a nested child
loops; outer siblings remain schedulable. Frames are independently snapshot
per nested scope and only explicitly selected outputs reach the parent.
Structured nested admission has an explicit maximum active depth of 64,
reported as a typed error. **Native asynchronous child settlement and native
provider wakeup remain outstanding**, as does the stronger lifecycle guarantee
for native calls still running when a root is cancelled.

This is not yet full structured native-async execution. Forks inside inlined
subplans still require typed frame mappings and validated slot handoffs. Typed input/output handoff for inlined subplans, selected-plan
artifact discovery, runtime lifecycle rebinding, and Basic/Advanced product
streaming parity remain open. A new generation must select its own frame
declarations; reconfiguration does not inherit a former owner's frame grants.

## Typed compile-time subplan frame handoffs

A selected component may reuse another selected workflow through
`IncludeMapped`, which declares an inclusion-site identity, exact child
workflow, explicit source-to-target entry slot mappings and explicit
source-to-target return slot mappings. Core inlines the selected child before
compiling providers or starting a root. Handoffs become **metadata on
ordinary Invoke result transitions**, not another IR step or a second
executor. Child node identities and nested Fork/MapFork branch identities are
qualified by the stable inclusion-site prefix.

Candidate preparation checks each source and target field against the
selected versioned frame schema and rejects missing fields, incompatible
types, or two sources writing the same target. Runtime transfers read one
immutable pre-handoff snapshot, validate all target values and atomically
publish only the complete new frame. Failed transfers cannot partially
modify the frame or replay provider side effects. The unframed entry
rejects such workflows before invoking the first provider.

Returns from an included workflow rewrite **only root-scope Finish outcomes**
through the declared parent continuation. A child Fork's own Finish retains
its child-scope settlement, including bounded map fan-out. Normal nested
Fork continuations require an explicit child node, not an implicit subplan
return. Typed inclusion tests exercise real provider-pinned input/output
handoffs and child map Fork settlement within the same root.

Mapped output handoffs now work on an immediate parent Finish; the output
copy executes after the child Invoke and before root settlement. An explicit
parent Transfer can also compose with child outputs when the sources and
destinations are independent. Alias-dependent sequential return mappings
reject candidate compilation rather than reading stale values.

**Still outstanding:** private frame-slot qualification and per-subplan
published-slot/export visibility, cross-owner subplan selection, arbitrary
sequential return mapping composition, and portable EntryBinding activation. The current contract makes frame mapping explicit rather than
quietly sharing or defaulting unknown fields. It does not claim those
remaining name/ownership semantics are complete.

## First async and portable outcome contracts (implementation partial)

The Core now exposes a root/generation-correlated `WorkflowPendingTasks`
accounting contract: individual task tickets have a selected generation, scoped
identity and one settlement. Cancellation marks pending work as
`Cancelling` and returns each ticket for signalling only once. The tracker
refuses root closure until every native ticket actually settles, including
successful completions after cancellation. Late or wrong-generation callbacks
and duplicate settlements reject. Focused Core tests cover these rules.
This is task accounting, **not yet physical generation lease retention**.
The cooperative scheduler, callback dispatcher and native ABI still need to
attach the tracker to a pinned root and safely retire orphaned tasks; pending
native invocations cannot yet progress concurrently along this execution path.
This is a partial native-async lifecycle contract, not a claim of async parity.

The Core also exposes `WorkflowOutcomeProjection` revision 1. It is a
portable declarative normal-result selector for closed `Variant` tags,
typed `Table` string fields and direct strings. Candidate validation rejects
unsupported revisions, omitted or invented variant cases, and outcomes not
declared by the plan node. Runtime checked projection rejects incorrect
structural payloads and unknown discriminants instead of turning them into
normal recovery edges. Tests cover round-trip portable bytes, typed results,
and negative cases. The selected projection binding now validates against
the bound import response schema during candidate preparation, contributes
to generation identity, and is enforced on the active pinned execution route.
The first-party agent topology publishes its two normal-result selectors.
Its Rust adapter still handles typed state, tool usage and side-effect
conformance. Portable artifact discovery and replacing that adapter with
portable state handling remain open.

## Target-state Core IR boundary and migration gate

**Normative target: [kernel RFC #736](https://github.com/matthis-k/phenix-ai/pull/736).** This section describes the eventual architecture, not functionality implemented by #726. The current `WorkflowTopology`, hand-authored Rust adapter, and legacy loop remain migration scaffolding until parity is proved. Do not turn the interim workflow representation into a second permanent Core executor or claim semantic completion from the current direct workflow fixture.

- **Core owns one minimal ExecutionPlan IR and scheduler.** The closed step vocabulary is `Invoke`, `Fork`, `Join`, `Exit`, with typed frame slots, outcomes, edges, scopes and named extension slots. The agent topology plugin owns friendly concepts like turn and tool batch and lowers them to `Invoke` and generic transitions. Domain workflow authoring never becomes a Core node kind.
- Core must permit cyclic control-flow edges with a suspension point, fair scheduling, cancellation checks on each back-edge and one root/generation across iterations. Required provider/import dependency cycles still fail.
- `Fork` permits fixed branches or bounded finite map fan-out. `Join` policies are the closed set `All(CollectAll)`, `All(FailFast)`, `FirstCompleted`, `FirstSuccess` and `Quorum(k)`. Core owns admitted branch scopes, sibling cancellation, cleanup and generation leases. Internal provider tasks remain scope-owned but are not invented IR nodes.
- Frames carry typed data, never Core capabilities or credential handles. Data-only immutable references require a separate authorized dereference. Recoverable domain outcomes are declared normal result variants; cancellation, transport and authority failures are execution failures, not synthetic plan edges.
- Each versioned `ExecutionPlanDefinition` is a plugin-owned contribution. An `EntryBinding` is resolved at candidate preparation, then fixes one compatible plan and generation at plan-backed root admission. Ordinary service-only roots require no plan. A session does not dynamically choose another plan.
- Reusable subplans are inlined at preparation using stable inclusion-site-qualified identities and typed frame mappings. Recursive inclusion and runtime `CallPlan` are out of scope. Subplan expansion happens before Core IR patch composition.
- A selected plan declares its schema, IR semantic revision, owner, entry and public slots. Generic patch composition and frame extension belong to #729. Workflow authors may lower domain-specific patches to those IR edits, but cannot run a second graph-patch executor.

**Added semantic acceptance:** An agent-free plan with a back-edge, bounded map fork, each join policy, one inlined subplan, independent providers, a service-only root, and owner-controlled plan entry runs through canonical Core dispatch. Additional fixtures prove parity of real Basic/Advanced application streaming, usage, cancellation, progress and side effects against the legacy path before retiring it. #728 supplies portable plan/entry contributions; #729 supplies IR patch composition. Changes outside this PR's owned workflow files belong in those successor PRs.
