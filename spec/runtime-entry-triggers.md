# Runtime entry triggers

## Status

Proposed architecture contract for graph-owned execution ingress.

This document separates three concepts:

- **entrypoint**: an executable component endpoint;
- **trigger**: a condition that starts a new execution at an entrypoint;
- **hook**: work that participates in an execution that already exists.

The first implementation target is model tool calls. The current application-owned `runtime_model_tools()` path for `bash` should migrate to an explicit `ToolCall` trigger declared on a graph entrypoint.

## Problem

The resolved runtime graph is not currently the source of truth for model-callable runtime tools.

Today the harness builds the runtime model tool list in `phenix-harness/src/application.rs::runtime_model_tools()`. The `bash` descriptor is constructed there, then dispatched through a separate `if callable_id == "bash"` branch.

This creates two independent descriptions of runtime capability:

```text
resolved runtime graph
  phenix.workspace@1
  phenix.memory@1
  ...

application model-tool list
  bash
  phenix.inspect
```

A component can therefore be loaded, resolved, and authorized without any inspectable relation to model-callability. Conversely, a runtime tool can be model-visible because application assembly added it to a side list.

The graph should describe why an execution may start.

## Core semantics

### Entrypoint

An entrypoint names one executable endpoint in the resolved component graph.

An entrypoint has:

- component identity;
- interface identity;
- request schema;
- response schema;
- authority requirements.

The existing component export model already carries most of this information.

### Trigger

A trigger answers:

> Why should a new execution start at this entrypoint?

A trigger never modifies the execution that caused it. It creates a new execution.

Initial trigger:

```rust
EntryTrigger::ToolCall {
    callable_id,
    description,
}
```

Expected later trigger classes include:

```text
ToolCall
ExternalEvent
Timer
ExecutionStarted
ExecutionSucceeded
ExecutionFailed
ExecutionCompleted
```

Execution-derived triggers allow graph relations such as:

```text
A.Completed -> start B
```

The failure of B does not retroactively change A unless another explicit policy couples them.

### Hook

A hook participates in an execution that already exists.

Examples:

```text
before invoke -> permission check -> continue / deny
before invoke -> transform request
after invoke  -> transform response
during invoke -> observe
```

Hooks may observe, augment, transform, or gate the execution they are attached to. Their ordering, failure behavior, and mutation contract belong to Core.

A handler supplied by a plugin may itself be implemented by invoking a graph entrypoint, but the hook remains part of the parent execution.

The practical distinction is:

```text
trigger = because X happened, start Y
hook    = while X is happening, run Y as part of X
```

A useful failure test:

- if Y fails and X remains complete, model it as a trigger;
- if Y fails and that failure can fail or alter X, model it as a hook.

Pure after-the-fact observation should prefer a trigger. Hooks should be reserved for execution coupling.

## Tool-call projection

Model tool discovery should become a projection of the resolved graph.

For one session or execution:

```text
resolved graph
  -> entrypoints with ToolCall triggers
  -> authority filtering / attenuation
  -> backend-neutral ModelToolDescriptor values
  -> session-admitted client tools
  -> provider request
```

The model integration layer owns conversion to provider-specific request formats. Core does not need OpenAI-, Anthropic-, or provider-specific tool schemas.

A `ToolCall` trigger should reuse the entrypoint interface schema:

```text
interface request schema  -> ModelToolDescriptor.input_schema
interface response schema -> ModelToolDescriptor.output_schema
```

The trigger declaration owns only metadata that is specific to the trigger, such as model-visible callable ID and description.

## Invocation

The inverse mapping must also come from the same resolved declaration.

```text
provider emits tool call
  -> resolve ToolCall trigger by callable ID
  -> target declared component entrypoint
  -> invoke through normal graph dispatch
  -> return result to the agent loop
```

Discovery and invocation must not maintain separate callable registries.

Duplicate effective callable IDs are a graph/session-surface conflict and must fail before provider invocation.

## Authority

A trigger is permission to describe a possible ingress path. It is not authority to invoke it.

Model visibility is the intersection of:

```text
declared trigger
+ resolved generation
+ active entrypoint
+ session/execution authority
+ integration compatibility
```

Invocation still runs under the captured execution authority and normal graph attenuation.

The trigger must not bypass the target entrypoint's required authority.

## Introspection

The runtime inspector should expose trigger state directly.

Example:

```text
component: phenix.application-agent-tools
entrypoint: shell-tool@1
trigger: tool_call
callable: bash
resolved: yes
authorized: yes
advertised: yes
```

For memory:

```text
component: phenix.memory
interface: phenix.memory@1
resolved: yes
tool_call trigger: none
advertised: no
```

This makes "loaded but not model-callable" an explicit graph fact instead of an inference from missing tool descriptors.

## First migration: `bash`

Do not attach `ToolCall("bash")` directly to the broad `WorkspaceInterface`.

The current tool is an adapter:

```text
{ command: String }
  -> WorkspaceCommand::Shell
  -> phenix.workspace@1
```

Preserve that boundary by making the adapter a narrow graph entrypoint owned by the application/tool integration component:

```text
ToolCall("bash")
  -> shell-tool entrypoint
  -> WorkspaceCommand::Shell
  -> phenix.workspace@1
```

The shell-tool entrypoint owns the model-facing request and response contract. The workspace plugin continues to own execution backend selection.

After migration, these application-specific paths should disappear for `bash`:

```text
runtime_model_tools() hard-coded bash descriptor
is_runtime_model_tool("bash")
if callable_id == "bash"
```

The generic trigger projection and trigger dispatch should replace them.

## `phenix.inspect`

Do not force `phenix.inspect` through the same migration in the first change.

Its current implementation depends on execution-local state, including the pinned execution/generation. It may eventually become a graph entrypoint, but only after that execution context has an explicit structural input or host-provided invocation context.

Keeping it host-owned temporarily is preferable to hiding execution-local dependencies behind a fake ordinary component service.

## Memory

This proposal does not make `phenix.memory` model-callable by default.

Memory remains internal unless a memory plugin explicitly declares a model-callable entrypoint with a `ToolCall` trigger.

For example, a future plugin could choose to expose:

```text
ToolCall("memory.recall") -> memory recall entrypoint
```

while keeping compaction, expansion, maintenance, and persistence operations internal.

Resolved service availability must never imply model exposure.

## Hooks and trigger-backed handlers

Core should own hook semantics:

- hook phases;
- ordering;
- synchronization;
- failure policy;
- guard / transform contracts.

Plugins own hook handlers and attachment declarations.

A hook handler may invoke ordinary graph entrypoints. This reuses graph dispatch without collapsing hooks into triggers.

The runtime graph may therefore contain distinct relation types:

```text
dependency edge
A -> B

trigger edge
A.Completed -> start B

hook attachment
H -> BeforeInvoke(A)
```

These relations should remain distinguishable in inspection and visualization.

## Validation

Resolution should reject:

- duplicate `ToolCall` callable IDs in one effective model surface;
- triggers targeting missing entrypoints;
- trigger schema derivation from an invalid or unresolved export;
- trigger declarations that exceed component/plugin authority;
- ambiguous trigger targets after generation resolution.

Session-specific client tools remain session admissions. They merge with graph-derived `ToolCall` entries at the existing model-surface boundary and retain duplicate-ID checks.

## Ownership

| Concern | Owner |
|---|---|
| Entrypoint identity and graph resolution | Core |
| Trigger declaration and trigger kind semantics | Core |
| Hook lifecycle semantics | Core |
| Trigger/hook declarations for domain behavior | Plugins |
| Backend-neutral model tool projection | Model/tool integration |
| Provider request encoding | Provider integration |
| Which plugins/triggers are enabled | Application/configuration |
| Workspace shell behavior | Workspace plugin |
| `bash` adapter schema | Tool integration component |

## Implementation order

1. Add typed entry-trigger metadata to resolved component topology.
2. Add `ToolCall` as the first trigger kind.
3. Expose resolved triggers through inspection.
4. Add a narrow shell-tool entrypoint to the application tool component.
5. Declare `ToolCall("bash")` on that entrypoint.
6. Project graph `ToolCall` triggers into `ModelToolDescriptor`.
7. Dispatch model tool calls back through the declared entrypoint.
8. Remove the hard-coded `bash` descriptor and dispatch branch.
9. Add regression coverage proving that a resolved component without a `ToolCall` trigger is not advertised.
10. Add regression coverage proving trigger authority filtering and duplicate callable rejection.

## Acceptance

The first implementation is complete when:

- `bash` appears in the provider tool surface without being named in an application-owned runtime tool list;
- the same resolved trigger declaration identifies the invocation target;
- removing the trigger from the graph removes `bash` from the provider tool surface without changing the shell implementation;
- `phenix.memory` remains resolved but absent from the model tool surface because it has no `ToolCall` trigger;
- inspector output explains both states;
- hooks retain their current execution-coupled semantics and are not reclassified as triggers.
