# Agent runtime orchestration

status: implemented
coverage:
  - rust/crates/phenix-harness/src/application.rs
  - rust/crates/phenix-acp-stdio/src/transport.rs
  - rust/crates/phenix-core/src/runtime/residency.rs
  - rust/crates/phenix-core/src/runtime/trace.rs
  - rust/crates/phenix-core/src/plugin_management_regression.rs
  - rust/crates/phenix-core/src/plugin_build_loading_regression.rs
depends_on:
  - spec/application-interface.md
  - spec/selectable-harness-generations.md
  - spec/plugin-runtime-bridges.md
  - spec/runtime-inspection.md
  - spec/environment-workspace-boundary.md

## Purpose

Allow one running agent execution to create and drive other Phenix sessions and to test a changed Plugin in a resident Harness generation before promotion.

The supported workflow is:

```text
controller session S0 on generation G1
  |
  +-- edit Plugin P
  +-- build P
  +-- stage candidate generation G2
  +-- create session S1
  +-- prompt S1 against G2
  +-- create session S2
  +-- prompt S2 against G2
  +-- inspect executions and memory behavior
  +-- promote G2 or retire it
```

Memory is a primary use case. A memory implementation should be testable across independent sessions without replacing the runtime that controls the test.

This feature does not introduce a second session implementation, Plugin lifecycle, agent loop, or runtime registry. Agent controls lower to the existing application operations and Core generation-management mechanisms.

## Existing mechanisms

Phenix already owns most required semantics:

| Need | Current owner |
| --- | --- |
| durable sessions | `phenix.sessions` |
| create/list/resume/close/prompt application operations | `phenix-application-interface` and Harness application worker |
| agent execution and tool loop | execution and agent-loop Plugins |
| Plugin build/load/unload/reconcile | `GraphReconciler::manage` |
| resident Harness generations | Core generation residency |
| explicit root invocation in one generation | `Kernel::invoke_in_generation` |
| promotion and retirement | `GraphReconciler` resident operations |
| authority attenuation | `Authority` and `RootExecutionConstraints` |
| runtime inspection | `phenix.inspect` |

The composition layer exposes these operations to an authorized model execution as one development workflow, while keeping session ownership in the application layer and generation ownership in Core.

## Decisions

### Agent and session are different identities

A session is durable conversation state. An agent execution is an actor operating inside one root execution.

One agent may hold handles to many sessions:

```text
agent execution E0
  |
  +-- S0 controller
  +-- S1 test
  `-- S2 verification
```

Phenix does not add an "agent owns session" durable relation. Session access is controlled by execution authority and application policy.

### Generation belongs to a root execution

A session is not permanently bound to a Harness generation.

A session may contain executions from different generations over time:

```text
S1
  |
  +-- E1 -> G2
  +-- E2 -> G2
  `-- E3 -> G3
```

Each root execution selects one generation. Every model call, tool call, nested Plugin call, task, and causal Event under that root remains pinned to the same generation.

This reuses the invariant from `spec/selectable-harness-generations.md`.

### Agent control reuses application semantics

Agent session control must call the same create, prompt, resume, and close semantics used by an external client.

Do not implement a second agent loop inside a session-control Plugin.

The application worker remains the owner of:

- session projection;
- prompt journaling;
- root execution allocation;
- context preparation;
- model input construction;
- progress projection;
- final assistant output.

The model-facing control is an adapter to those operations.

### Plugin development reuses reconciliation

Agent Plugin development must lower to `GraphReconciler`.

The agent does not receive mutable `Kernel`, resident-generation registry, EventBus internals, persistence handles, or raw Plugin instances.

The development API owns workflow policy only.

## Session control

Expose an opt-in model tool named `phenix.session`.

The tool is available only when the current execution has the session-control capability.

The first operation set is:

```text
create
prompt
resume
close
list
```

The model tool uses one descriptor so related session controls do not consume several eager tool schemas.

### Create

Input:

```text
operation = create
working_directory
title?
```

Result:

```text
session_id
working_directory
title?
```

Creation delegates to the canonical application create-session operation.

### Prompt

Input:

```text
operation = prompt
session_id
content
generation?
```

`generation` is optional.

For an external application caller, omitted generation means the runtime default generation.

For an agent caller, omitted generation means the caller's current root generation. This keeps nested orchestration in the same execution world unless the agent explicitly selects a resident trial generation.

A selected generation must already be resident and compatible with the caller's captured root constraints.

The operation waits for the child root to reach a terminal state. It returns:

```text
session_id
execution_id
generation
stop_reason
through_sequence
assistant_message?
```

Returning the terminal assistant message makes integration tests deterministic without requiring a second read for the common case. The durable session journal remains authoritative.

### Resume

Resume returns a session snapshot after an optional sequence watermark. It is the normal way to inspect detailed child execution history.

### Close

Close uses the existing session transition and journal semantics.

### List

List returns the application session registry. It does not create an agent-specific session registry.

## Cross-session execution scheduling

The application worker currently tracks active work by `SessionId`. Agent orchestration should use that structure directly.

Rules:

- at most one active prompt per session;
- prompts for different sessions may execute concurrently;
- a prompt targeting its own currently active session fails with a typed conflict;
- cancellation remains session-scoped;
- progress and completion are projected by `session_id + execution_id`;
- application state mutation remains serialized by the worker event loop.

Concurrent independent sessions are required for synchronous parent-to-child orchestration. If S0 had to finish before S1 could start, `S0 -> prompt S1 -> wait` would deadlock.

The worker should therefore admit S1 while S0 is active. It should continue to reject or defer another prompt for S0.

## Root constraints and generation selection

Every application root captures one `RootExecutionConstraints` object.

Agent-created child roots inherit that object. Child authority may be attenuated further, but it cannot be widened.

Generation selection validates:

```text
requested generation is resident
requested generation satisfies pinned bindings
requested generation fits the original authority ceiling
durable schemas are compatible
```

The child root then starts through the selected generation.

All root setup that has generation semantics must use the selected generation. This includes execution allocation, execution-resource setup, context preparation, tool discovery, and agent-loop dispatch.

Session persistence remains shared according to the existing persistence contracts.

### Pinned host bindings

The controlling root may have host-pinned bindings such as Environment.

A child generation must preserve every inherited pin.

Selecting a Harness generation is not an Environment switch. Testing a candidate under another Environment requires a separately authorized root.

## Plugin development control

Expose an opt-in model tool named `phenix.plugin`.

The tool is policy over existing Core mechanisms.

Operations:

```text
inspect
build
trial
promote
rollback
retire
```

### Inspect

Returns:

- active generation;
- resident generations;
- Plugin IDs and artifact revisions;
- compatibility state where available.

This is management metadata. Detailed graph and execution inspection remains in `phenix.inspect`.

### Build

Build materializes a Plugin artifact using the existing typed build plan.

Build does not change the active or resident runtime.

The result identifies:

```text
plugin
artifact revision
build evidence
```

### Trial

Trial performs:

```text
materialize artifact
-> resolve complete candidate Harness
-> validate root constraints
-> validate persistence compatibility
-> make candidate resident
```

It does not change the default generation.

Result:

```text
generation
plugin
artifact revision
diff from active
```

The raw `PluginManagementRequest` stays internal. The agent supplies the semantic development request, not a reconstructed desired runtime graph.

### Promote

Promotion validates the caller's inherited root constraints again, then atomically changes the default generation.

Already-running roots remain on their original generation.

The previous default stays resident as a rollback target.

### Rollback

Rollback promotes a compatible previous resident generation.

Rollback is not a second state machine.

### Retire

Retire stops admission to a non-default generation and releases it through the existing generation-aware retirement path.

The default generation cannot be retired.

## Preparing a trial candidate

Current `GraphReconciler::manage` is the canonical one-shot Plugin management path. Trial residency needs the same build and resolution logic without immediately replacing the active generation.

Refactor management preparation into one internal operation:

```text
prepare Plugin management request
  |
  +-- authorize
  +-- build/materialize artifact
  +-- apply desired Plugin change
  +-- resolve candidate Harness
  `-- return prepared candidate + build report
```

Then choose one commit path:

```text
stable management
  prepared candidate
    -> activate candidate

trial management
  prepared candidate
    -> make candidate resident
```

Build, artifact validation, desired-set mutation, and candidate resolution remain single-source semantics.

## Harness ownership

Core already supports several resident generations, but `PhenixHarness` currently stores one separate `ResolvedHarness` value.

That becomes incorrect once Harness exposes resident-generation workflows because the reconciler also owns active/resident resolved Harness values.

The Harness should own:

```text
PhenixHarness
  kernel
  reconciler
  application agent-tool registry
```

The active resolved Harness is `reconciler.active()`.

This PR starts with that prerequisite. It removes the extra active `ResolvedHarness` copy before model-facing lifecycle control is added.

## Tool visibility after Plugin changes

A child root must construct its model-tool catalog from its selected generation.

If G2 adds tool `memory.debug`, then:

```text
S0 / G1 -> does not see memory.debug
S1 / G2 -> sees memory.debug
```

Tool discovery must not read the runtime default when a child root explicitly selected another resident generation.

The same rule applies to skills, context providers, listeners, and callable entry triggers.

## Memory integration test

A complete memory self-test can run as:

```text
S0 / G1 controller

plugin.trial(memory change)
  -> G2

session.create()
  -> S1

session.prompt(
  session = S1,
  generation = G2,
  "Remember that project codename is Helios."
)

session.close(S1)

session.create()
  -> S2

session.prompt(
  session = S2,
  generation = G2,
  "What is the project codename?"
)

phenix.inspect execution/dag for S2 result

expected:
  answer resolves Helios
  memory recall path is observable
  no G1 tool/provider is used inside either G2 root
```

A stronger persistence test may restart or replace the model/session execution while keeping the same permitted memory persistence binding.

A migration test that makes G1 and G2 persistence schemas incompatible requires an isolated persistence or Environment binding. Core must reject shared-store concurrent residency in that case.

## Authority

Use separate capabilities for session orchestration and Plugin development.

Suggested capabilities:

```text
application.session.control
runtime.generation.select
runtime.plugin.inspect
runtime.plugin.build
runtime.plugin.trial
runtime.plugin.promote
runtime.plugin.retire
```

The exact names should follow the repository's existing capability naming convention during implementation.

Rules:

1. A child root starts with parent authority, then applies any requested attenuation.
2. Generation selection cannot add authority.
3. Plugin build uses only the Environment/workspace authority granted to the parent.
4. Trial Plugin lifecycle is bounded by the admitting root's `RootExecutionConstraints`.
5. Promotion and retirement revalidate constraints at operation time.
6. Session control does not imply Plugin-management authority.
7. Plugin-management authority does not imply arbitrary workspace or network authority.

## Failure behavior

Failures are typed and leave durable state inspectable.

Important cases:

| Failure | Result |
| --- | --- |
| unknown session | not found |
| prompt on already-active target session | conflict |
| unknown generation | unknown-generation error |
| generation violates pinned binding | pinned-binding error |
| candidate widens authority | authorization error |
| incompatible durable schema | resident-generation compatibility error |
| Plugin build fails | build failure with evidence |
| candidate activation fails | active generation unchanged |
| promotion fails | current default unchanged; candidate stays resident |
| child model fails | child execution is failed; parent tool call receives typed error |
| parent cancelled | pending orchestration call is cancelled; child cancellation follows declared policy |

A failed child execution must not mark the parent execution failed unless the parent chooses to treat the tool error as fatal.

## Cancellation

The first implementation uses structured parent-child cancellation:

- parent cancellation requests cancellation of child roots started by the current tool call;
- an already completed child remains completed;
- closing a session does not cancel unrelated roots in other sessions;
- retirement of a generation follows generation lease and task rules from the selectable-generation spec.

Detached child sessions can be added later as an explicit operation. They should not be the default because they make execution ownership and user expectations less clear.

## Observability

Orchestration actions emit metadata-only `RuntimeTraceEvent::Orchestration` records carrying the controller session and execution, operation kind, target session when present, child execution when present, selected graph generation, target Plugin generation when relevant, success, and an error descriptor on failure.

Execution inspection remains generation-aware. The orchestration trace uses the existing runtime diagnostic stream rather than creating another log format.

Parent tool results include stable session, execution, and generation identifiers where the operation produces them so `phenix.inspect` can continue the investigation.

## Public application API

The external application API remains compatible.

Existing `Prompt` continues to mean "prompt this session using the default application root policy."

Generation selection is an agent-development operation. External application clients continue to use the default application root policy. A future public generation-selection operation, if needed, must reuse the same internal root request rather than introducing a second dispatch path.

Do not overload a durable Session record with a generation field.

## Acceptance tests

Regression coverage proves:

- S0 can create S1 while S0 has an active execution.
- S0 can synchronously prompt S1 and receive the terminal result.
- S0 and S1 never have more than one active prompt each.
- S0 on G1 can prompt S1 on resident G2 without promoting G2.
- every nested call under S1's child root stays on G2.
- S1 sees G2 tool entry triggers while S0 continues to see G1.
- a child generation cannot change a pinned Environment binding.
- a child root cannot gain authority absent from S0.
- failed G2 activation leaves G1 active and usable.
- promotion changes only future unqualified roots.
- rollback promotes the retained previous generation.
- retirement cannot remove the active generation.
- incompatible durable schemas reject shared-store trial residency.
- memory written through S1/G2 can be recalled through independent S2/G2 when memory scope permits it.
- execution inspection shows the expected memory and tool path for the child execution.
- cancelling S0 cancels a synchronous child orchestration request without corrupting either session journal.

## Non-goals

This feature does not add:

- a second runtime process per child session;
- mutable per-call provider switching inside one root;
- implicit Environment switching;
- an agent-private session database;
- an agent-private Plugin registry;
- persistence cloning for every trial;
- unbounded detached worker spawning;
- direct model access to mutable Kernel internals.
