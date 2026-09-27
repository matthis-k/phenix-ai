# Selectable Harness generations

status: proposed
depends_on:
  - spec/runtime-topology-generation.md
  - spec/configuration-frontends.md
  - spec/plugin-runtime-bridges.md
  - spec/environment-workspace-boundary.md

## Purpose

Keep the Phenix runtime and kernel stable while allowing multiple resolved Harness generations to remain available for explicit root execution.

This supports plugin development without turning reload into a second runtime model. An agent may build a candidate generation, exercise it, compare it with the current generation, and promote it without gaining new authority or silently changing its Environment.

## Decision

The kernel remains the long-lived owner of host state.

A `ResolvedHarness` remains one immutable, internally coherent runtime topology. Plugins, Components, provider bindings, Layers, Listeners, skills, resources, and configuration are resolved as one generation.

The runtime may keep more than one compatible generation resident:

```text
Runtime / Kernel
|
+-- fixed root constraints
|   +-- authority ceiling
|   `-- pinned host bindings
|
+-- generation A
+-- generation B
`-- generation C [default]
```

A root execution selects exactly one generation. Every nested call, task, event, worker, and callback created by that root remains pinned to that generation.

Harness selection never mixes nodes from different generations.

## Ownership

The stable runtime owns:

```text
persistence backend
task/event infrastructure
trace/provenance sinks
artifact/runtime-provider infrastructure
resident generation registry
default-generation pointer
root execution constraints
```

Each resident generation owns:

```text
ResolvedHarness
Plugin instances
Plugin invocation endpoints
resolved service/component topology
generation-local Listener subscriptions
generation-local tasks and process/runtime handles
```

Durable state remains owned by Plugin/resource identity according to the existing persistence contracts. A generation does not receive a private durable namespace merely because it is resident beside another generation.

## Root execution constraints

Generation choice is inside a fixed root constraint set.

The constraint set contains at least:

```text
authority ceiling
pinned resolved host bindings
```

The runtime derives effective authority monotonically:

```text
effective root authority
  = initial authority ceiling
  ∩ caller/request authority
  ∩ selected generation policy
  ∩ downstream Plugin/call attenuation
```

Selecting another generation cannot add a capability absent from the initial ceiling.

### Pinned host bindings

Some resolved imports define the execution world rather than ordinary replaceable application behavior. The host may pin those bindings when it creates the root constraint set.

A pin identifies a canonical resolved binding, not only a Plugin ID. Its identity must cover the resolved importer/interface/provider endpoint and composition inputs that can change the binding's semantics.

Environment is the primary example.

If the original root is bound to one Environment, selecting another Harness generation must preserve that Environment binding. A candidate that changes the pinned Environment provider, artifact/configuration semantics, or another pinned host binding is incompatible with that root.

Changing the Environment requires a separately authorized root constraint set. Harness selection is not an Environment-switch operation.

Core should expose this as a generic resolved-binding constraint. Core must not hard-code an Environment concept.

## Selection boundary

Generation selection occurs only when a root execution begins.

Examples include:

```text
application request
agent turn root
worker root
explicit evaluation run
plugin-development trial run
```

After selection:

```text
root -> generation B
  |
  +-- model call -> B
  +-- tool call -> B
  +-- nested Plugin call -> B
  +-- task -> B
  `-- emitted Event -> B
```

A call cannot switch to generation C halfway through its causal tree.

A model-facing tool may request a generation for a new root if policy allows it. It does not receive a mutable dispatch handle that can choose a different generation per nested service call.

## Default and resident generations

The runtime has exactly one default generation.

A request that does not explicitly select a generation uses the default.

Other compatible generations may remain resident for explicit roots. Residency means the runtime can execute them. It does not make them global ambient topology.

```text
generation A [resident]
generation B [resident]
generation C [resident, default]
```

This distinction prevents a trial generation from duplicating global side effects.

## Ambient events and controllers

Only the default generation receives unscoped ambient runtime delivery.

Examples include timers, external input feeds, default controllers, and other events that are not causally attached to an explicit root generation.

Events emitted inside a selected root carry that root's generation and dispatch to that generation's resolved subscriptions.

This rule prevents:

```text
A listener handles external event
B listener handles the same external event
=> duplicate side effect
```

Generation-local controllers and background tasks remain pinned to the generation that created them. A non-default resident generation may run work only when that work is explicitly rooted in that generation or is a descendant of such work.

## Promotion

Promotion changes the default for future roots and ambient delivery.

```text
candidate B resident
  -> validate root constraints
  -> validate durable compatibility
  -> activate required generation-local state
  -> atomically set default = B
```

Failure before the pointer change leaves the previous default unchanged.

Already-running roots remain pinned to their starting generation.

Promotion does not broaden the root authority ceiling or replace pinned host bindings.

## Retirement

Retirement has two phases:

1. stop admitting new roots to the generation;
2. release generation-local runtime state after its leases drain or cancellation policy terminates them.

Retirement removes generation-local Plugin instances, invocation endpoints, Listener subscriptions, tasks, and runtime handles.

Shared persistence and other kernel-owned host state remain alive.

A generation referenced by an active root or task cannot disappear underneath that work.

## Durable-state compatibility

Concurrent residency does not imply persistence isolation.

Two generations may be resident against the same durable state only when their durable schemas and migration expectations are compatible.

A candidate that requires a migration which would make the current generation invalid cannot be trial-resident against the same live persistence state.

Such a change requires one of:

```text
promotion with the existing migration transaction
an explicitly separate persistence binding
a dedicated test/staging Environment
```

The runtime must reject incompatible concurrent residency rather than let the old generation continue against state it no longer understands.

## Plugin replacement

Plugin replacement remains a Harness-generation operation.

```text
generation A
  |
modify Plugin P
  |
resolve generation B
  |
make B resident
  |
run explicit roots on A and B
  |
promote B
  |
retire A when no longer needed
```

There is no separate reload lifecycle in Core.

`phenix-plugin-dev` may provide the agent-facing convenience API for this flow. It does not own generation semantics, authority, Environment constraints, persistence compatibility, or Plugin lifecycle.

## Agent-facing development flow

A development plugin may expose operations such as:

```text
plugin.inspect
plugin.build
plugin.trial
plugin.promote
plugin.rollback
```

Those operations lower to ordinary Workspace/Environment, build, resolution, residency, selection, and promotion mechanisms.

The agent should not construct raw reconciliation requests when a smaller semantic operation is sufficient.

Rollback selects or promotes a previously resident compatible generation. It is not a second rollback state machine.

## Required Core/runtime shape

The current single-generation Kernel stores active Plugin state by `PluginId`. Selectable resident generations require generation-local runtime state.

Target shape:

```text
Kernel
|
+-- shared host state
|
+-- generations: GraphGenerationId -> GenerationRuntimeState
|     |
|     +-- resolved runtime generation
|     +-- Plugin states
|     +-- Plugin instances
|     +-- invocation endpoints
|     `-- Listener subscriptions
|
`-- default_generation
```

Runtime dispatch selects `GenerationRuntimeState` once at root entry. Existing `CallScope` generation pinning then carries that selection through nested execution.

The existing single-generation reconciliation API should remain a convenience path equivalent to:

```text
prepare candidate
-> make resident
-> promote
-> retire previous when unleased
```

Stable mode may keep exactly one resident generation.

## Event and task changes

The Event bus must distinguish generation-local subscription sets. Causally scoped events dispatch within their pinned generation. Ambient delivery targets the default generation.

The task runtime already records Graph Generation identity. Retirement should use that identity to drain or cancel only work owned by the retired generation.

## Binding compatibility

Before a generation becomes selectable under a root constraint set, the runtime validates every pinned binding against the candidate's resolved component graph.

The comparison uses canonical resolved binding identity.

A missing or changed pinned binding rejects selection before Plugin behavior from the candidate runs.

This keeps host constraints outside agent-controlled Harness composition while leaving the mechanism generic.

## Invariants

- One long-lived runtime/kernel owns shared host state.
- A `ResolvedHarness` is one immutable coherent topology.
- A root execution uses exactly one Harness generation.
- Nested execution cannot switch Harness generation.
- Multiple compatible generations may be resident.
- Exactly one generation is the default for new unqualified roots and ambient delivery.
- Generation selection cannot widen initial authority.
- Generation selection cannot change a pinned Environment or other host binding.
- Harness generations never mix providers dynamically across generation boundaries.
- Plugin instances, listeners, tasks, and runtime handles are generation-local.
- Durable persistence remains kernel-owned and is not forked implicitly.
- Incompatible durable migrations prevent concurrent residency.
- Promotion is an atomic default-pointer change after validation.
- Retirement never invalidates an active generation lease.
- `phenix-plugin-dev` is optional agent tooling over these runtime semantics.

## Implementation sequence

1. Add a generation-local runtime-state container keyed by `GraphGenerationId`.
2. Move Plugin state/instance/invocation maps from global Kernel fields into that container.
3. Add root dispatch selection and a default-generation pointer.
4. Make Listener subscription storage generation-aware; keep ambient delivery default-only.
5. Reuse existing task generation attribution for drain/cancellation.
6. Add generic pinned resolved-binding constraints plus authority-ceiling validation.
7. Split reconciliation into prepare/resident/promote/retire operations while keeping the current one-shot replacement API.
8. Add durable-compatibility checks for concurrent residency.
9. Add regressions for A/B explicit roots, promotion, rollback, fixed authority, fixed Environment binding, Listener isolation, task pinning, and retirement.
10. Add `phenix-plugin-dev` only after the runtime contract is complete.

## Acceptance regressions

The implementation is complete when tests prove:

- A and B can be resident simultaneously and explicit roots receive their respective Plugin implementations.
- one root cannot invoke part of A and part of B.
- promoting B changes only future unqualified roots.
- an A root that spans B promotion completes against A.
- selecting B cannot gain a capability absent from the original root authority ceiling.
- a B generation with a changed pinned Environment binding is rejected for the existing root constraint set.
- trial B does not receive unscoped ambient Listener delivery while A is default.
- an Event emitted inside a B root reaches B subscriptions, not A subscriptions.
- A and B generation-local tasks do not cancel each other.
- retiring A waits for or explicitly cancels A leases without affecting B.
- a failed B activation leaves A default and usable.
- incompatible durable-schema changes reject concurrent residency.
- promotion and rollback preserve shared kernel persistence rather than constructing a second runtime.
