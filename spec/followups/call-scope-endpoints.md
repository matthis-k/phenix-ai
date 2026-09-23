---
status: planned
source: kernel-runtime-mechanism-audit-2026-09-23
base_sha: b7334e59159fcd4eeedb802d953a801138fbf43c
depends_on:
  - runtime-generation
  - resolved-dispatch-plan
---

# Explicit call scope and canonical invocation endpoints

## Goal

Make invocation state explicit and give plugin calls one canonical endpoint path.

`PluginHost` currently carries static generation data, live runtime services, caller authority, cancellation, three separate causal guard collections, prepared mutation state, provenance, and optional continuation state. `InvocationContext` repeats most of the same fields.

Prepared-mutation coordination also depends on the current OS thread. Plugin invocation has two paths: a whole-instance mutable lock and optional `SharedPluginInvocation`.

This PR consolidates those mechanisms after runtime generation and dispatch topology have one owner.

## Current host state

At the audited head `PluginHost` contains:

```text
graph_generation
component_graph
config
states
instances
plugin
authority
call_cancellation
call_stack
events
tasks
persistence
prepared_mutations
provenance
continuation
active_services
active_component_endpoints
```

`InvocationContext` repeats the generation, graph, config, states, instances, events, tasks, persistence, prepared mutations, and provenance.

Three collections encode one causal fact:

- active plugins
- active services
- active component endpoints

Prepared mutations add another ambient stack keyed by `ThreadId`.

## Target decomposition

Use three clear values.

### Runtime services

Long-lived mutable kernel services:

```rust
struct RuntimeServices {
    events: Arc<EventBus>,
    tasks: Arc<TaskRuntime>,
    persistence: Arc<Mutex<Box<dyn PersistenceBackend>>>,
    traces: Arc<dyn RuntimeTraceSink>,
    provenance: Arc<ProvenanceBuffer>,
}
```

Exact ownership may stay directly on `Kernel` if wrapping these values adds no code deletion. The important rule is that they are not copied field by field into every call object.

### Runtime generation

Immutable resolved topology from the earlier PR:

```text
RuntimeGeneration
  id
  config
  component graph
  dispatch topology
  resources
```

Every call borrows or pins one generation.

### CallScope

Per-root-invocation state:

```rust
pub(crate) struct CallScope {
    generation: Arc<RuntimeGeneration>,
    authority: Authority,
    cancellation: CallCancellationToken,
    stack: InvocationStack,
    transactions: TransactionContext,
    trace: CallTraceContext,
}
```

Nested calls derive a child scope. They do not rebuild the whole host from loose fields.

## InvocationStack

Replace `call_stack`, `active_services`, and `active_component_endpoints` with one typed stack.

Suggested model:

```rust
enum InvocationFrame {
    Plugin { plugin: PluginId },
    Service { service: ServiceId },
    Component {
        component: ComponentId,
        service: ServiceId,
    },
}

struct InvocationStack {
    frames: SmallVec<[InvocationFrame; 8]>,
}
```

A plain `Vec` is also fine if it keeps dependencies smaller. Do not add `smallvec` unless measurement or existing use justifies it.

The stack provides the queries required by current semantics:

- plugin is already active
- service is already active
- component endpoint is already active
- push a child provider frame
- preserve path for diagnostics

Keep the exact #494 re-entry rules. Consolidation must not turn distinct same-plugin endpoints back into forbidden recursion.

Prefer stack order over three cloned `BTreeSet` values. If fast membership becomes necessary, derive temporary indexes inside the scope. Do not store three canonical representations again.

## TransactionContext

Replace thread-ID coordination in `PreparedMutationScope`.

Current path:

```text
with_coordinator(plugin)
  -> thread::current().id()
  -> push (ThreadId, PluginId)
prepare()
  -> thread::current().id()
  -> search coordinator stack
```

Target path:

```text
CallScope.transactions.coordinator = current coordinator
  -> owner prepares mutation with &TransactionContext
  -> prepared record stores explicit coordinator
```

Suggested value:

```rust
#[derive(Clone)]
pub(crate) struct TransactionContext {
    coordinator: PluginId,
}
```

Nested coordinator operations derive a child transaction context explicitly. Prepared mutation storage remains scope-local and one-shot as required by #493.

Delete `ThreadId`, `PreparedMutationCoordinatorGuard`, `coordinators: Mutex<Vec<_>>`, and `with_coordinator`.

Cross-plugin mutation authority must remain owner approved. Explicit context changes how the coordinator is carried, not who may prepare or commit.

## Canonical invocation endpoint

#494 added `SharedPluginInvocation` so reentrant component calls can avoid locking the whole mutable plugin instance. The runtime still has two dispatch branches:

```text
shared_invocation available
  -> call immutable endpoint
else
  -> lock Box<dyn PluginInstance>
  -> call mutable instance
```

Converge invocation onto one endpoint representation after activation.

Target model:

```rust
pub trait PluginInvocation: Send + Sync {
    fn invoke(..., host: &PluginHost<'_>) -> Result<Vec<u8>, String>;
    fn invoke_component(...);
    fn invoke_layer(...);
}

struct ActivePlugin {
    invocation: Arc<dyn PluginInvocation>,
    lifecycle: Mutex<Box<dyn PluginLifecycle>>,
}
```

Exact trait split may reuse existing `PluginInstance` and `SharedPluginInvocation` during implementation, but the completed runtime has one invocation branch.

Lifecycle remains exclusive:

- prepare/start/stop may mutate lifecycle state
- ordinary service/component/layer invocation uses an immutable endpoint
- mutable domain state lives behind explicit interior synchronization owned by the plugin
- stop waits for generation-scoped live calls to quiesce as today

If a legacy mutable plugin cannot provide an immutable endpoint, convert first-party plugins or provide one explicit adapter at activation. Do not keep a per-call optional branch forever.

The adapter must not reintroduce a plugin-wide mutex across outbound nested calls. If that cannot be guaranteed, fail activation for unsupported legacy shape rather than recreate the #494 deadlock class.

## PluginHost after cleanup

Target responsibility:

```rust
pub struct PluginHost<'a> {
    runtime: &'a RuntimeServices,
    scope: &'a CallScope,
    plugin: &'a PluginId,
    continuation: Option<ContinuationCursor>,
}
```

If `RuntimeServices` is not a struct, equivalent narrow references are acceptable. The goal is to delete duplicate static fields and guard sets.

Host accessors derive:

- generation from scope
- authority from scope
- cancellation from scope
- causal checks from scope stack
- transaction coordinator from scope transaction context
- traces from runtime service
- dispatch plan from generation

## Continuation

Represent a layer continuation as a cursor into the already selected dispatch plan:

```rust
struct ContinuationCursor {
    plan: Arc<ResolvedInvocationPlan>,
    next_position: usize,
    used: Arc<AtomicBool>,
}
```

Do not clone a full `ResolvedServiceChain` into continuation state.

`continue_service` derives a child scope with attenuated authority and resumes the cursor. It keeps the same generation, cancellation token, transaction context, trace context, and causal stack.

## Error and panic behavior

Preserve existing behavior:

- user/config/runtime failures stay typed at core boundaries
- plugin panic becomes the current service/lifecycle failure
- cancellation is checked after plugin return before accepting output
- re-entry fails before acquiring an incompatible endpoint
- continuation stays one-shot

Do not use the refactor to weaken these checks.

## Files and mechanical changes

Primary files:

- `rust/crates/phenix-core/src/runtime.rs`
- `rust/crates/phenix-core/src/runtime/dispatch.rs`
- `rust/crates/phenix-core/src/runtime/host.rs`
- `rust/crates/phenix-core/src/runtime/kernel.rs`
- `rust/crates/phenix-core/src/runtime/listener.rs`
- `rust/crates/phenix-core/src/runtime/reconciliation.rs`
- `rust/crates/phenix-core/src/prepared_mutation.rs`
- SDK authoring/static dispatch where invocation endpoints are produced

Mechanical sequence:

1. Add `InvocationStack` and prove existing re-entry behavior.
2. Add explicit `TransactionContext`; convert prepared-mutation APIs.
3. Delete thread-ID coordinator tracking.
4. Add `CallScope` and derive nested scopes.
5. Convert dispatch and continuation to use one scope.
6. Remove `ServiceDispatchGuards` and repeated `InvocationContext` construction.
7. Convert active plugin installation to produce one canonical invocation endpoint.
8. Convert first-party plugin authoring to that endpoint contract.
9. Remove per-call `shared_invocation` versus mutable-instance branch.
10. Shrink `PluginHost` to runtime, scope, current plugin, and continuation.
11. Delete superseded fields, guards, and adapters.

## Invariants from earlier PRs

Preserve #493:

- only namespace owners prepare foreign mutations
- handles are opaque and scope local
- commit attempts are one-shot
- generation, authority, owner, store, cancellation, and outstanding status are checked

Preserve #494:

- distinct components in one plugin may call each other
- repeated component endpoint recursion is rejected
- outbound nested calls do not hold a whole-plugin mutable lock

Preserve #495:

- admitted listener work has generation and causality provenance
- listener delivery remains bounded and asynchronous

Preserve #469:

- PluginHost remains the executable plugin boundary
- runtime-provider authority and guest authority remain separate
- live calls remain generation scoped

## Tests

Add focused regressions for:

- nested A -> B -> C calls preserve stack order
- A -> B -> A endpoint recursion rejects before endpoint acquisition
- same plugin with distinct component endpoints still succeeds
- service recursion rejects through InvocationStack
- nested authority can only attenuate
- nested cancellation token is the same root token
- prepared mutation coordinator follows explicit TransactionContext across nested calls
- prepared mutation behavior is independent of OS thread identity
- a prepared mutation created on one worker thread can still carry the correct explicit coordinator if the call scope is intentionally moved
- continuation resumes the same generation/plan and remains one-shot
- ordinary invocation uses one endpoint branch
- plugin stop remains exclusive and waits for live calls
- no whole-plugin mutable lock is held across outbound nested calls

## Deletion target

Delete these duplicate or ambient mechanisms:

- `InvocationContext` field-by-field runtime snapshot
- `ServiceDispatchGuards`
- `PluginHost.call_stack`
- `PluginHost.active_services`
- `PluginHost.active_component_endpoints`
- thread-ID prepared-mutation coordinator stack
- optional per-call `SharedPluginInvocation` branch beside mutable invocation
- continuation-owned cloned `ResolvedServiceChain`

Replace them with:

```text
RuntimeGeneration
CallScope
InvocationStack
TransactionContext
one PluginInvocation endpoint
ContinuationCursor
```

## Non-goals

- no plugin domain-state rewrite beyond what one immutable invocation endpoint requires
- no async Rust runtime migration
- no EventBus scheduling rewrite
- no provider composition change
- no agent policy change
- no persistence backend change

## Acceptance criteria

- [ ] Every nested invocation derives one explicit `CallScope`.
- [ ] One `InvocationStack` owns causal path state.
- [ ] Prepared mutation coordination has no `ThreadId` dependency.
- [ ] Plugin service/component/layer calls use one canonical invocation endpoint path.
- [ ] Lifecycle mutation remains exclusive and separate from invocation.
- [ ] PluginHost no longer repeats generation/config/graph and three causal collections.
- [ ] Continuations resume one preselected plan without cloning/re-resolving it.
- [ ] #493, #494, #495, and #469 regressions stay green.
- [ ] Source, Rust, Clippy, Product, Integration, Docs, and Maintenance checks pass at exact head.

## Completion metric

Record before and after:

- `PluginHost` field count
- invocation-context/guard production LOC
- number of per-call plugin invocation branches
- prepared-mutation coordinator production LOC
- clones of `ResolvedServiceChain` in the runtime hot path

The PR succeeds only if these counts decrease without weakening behavior.
