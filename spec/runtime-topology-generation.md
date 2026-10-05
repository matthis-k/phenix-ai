# Runtime topology generations

status: implemented
coverage:
  - rust/crates/phenix-core/src/runtime_topology_generation_regression.rs
  - rust/crates/phenix-sdk/tests/plugin_attribute_only_gate.rs

## Purpose

One resolved Graph Generation is the source of truth for executable runtime topology.

Plugins declare behavior and own Plugin state. They do not own routing. Components expose executable functions. The kernel resolver turns those declarations into callable nodes, service interposition, Event entry points, Plugin Resources, and lifecycle ownership. Loading or unloading a Plugin produces a candidate Graph Generation and activates it atomically.

This extends the Plugin authoring, Layering, Event, reconciliation, and `PluginHost` contracts.

## Model

A Plugin contributes:

- executable Component functions;
- service Exports and Terminal Provider implementations;
- Layers around service calls;
- Listeners for completed Events;
- Plugin Resources and metadata;
- lifecycle and Plugin state ownership.

The resolved Graph Generation owns how those contributions connect.

```text
service call -> Layer -> Layer -> Terminal Provider function

Event -> Listener function
      -> Listener function
```

Hooks are authoring syntax. An intercepting Hook lowers to a Layer. An observational Hook lowers to a Listener. There is no Hook runtime.

Listeners are Event-triggered entry points. They are not service-chain edges and cannot affect the operation that emitted the Event.

## Resolved topology

A resolved Graph Generation contains enough information to activate the executable topology without asking Plugin instances to discover or register routing semantics.

The resolved form owns:

- Graph Generation identity;
- Component and function ownership;
- resolved service chains and Effective Layer Order;
- Terminal Provider selection;
- Listener identities, Event contracts, dependency ordering, failure policies, and owning Plugin identity;
- Plugin Resource projections;
- lifecycle ownership and restart requirements.

Runtime handler objects may remain erased executable bindings, but their topology and ownership come from the resolved Graph Generation.

## Plugin instances

`PluginInstance` is an execution, lifecycle, and Plugin state container. It is not a routing abstraction.

A Plugin instance may provide callable implementations and Listener handlers for the nodes owned by its Plugin. It must not mutate global routing, choose service chains, or create hidden subscriptions that are absent from the resolved Graph Generation.

Factories answer only how to construct a Plugin implementation for a Graph Generation. Factory registration is not topology.

## Activation

Candidate Graph resolution finishes before live mutation.

Activation follows this order:

1. Resolve and validate the complete candidate Graph Generation.
2. Compute the transition from the active Graph Generation.
3. Stage new or restarted Plugin instances and Plugin Runtime bindings.
4. Keep the active Graph Generation unchanged if staging fails.
5. Install resolved configuration, Component Graph, service topology, Listener topology, Plugin Resource projections, and Graph Generation identity as one transition.
6. Retire removed or replaced executable bindings and Plugin instances.
7. Preserve unchanged Plugin instances when the transition permits it.

No caller may observe a mixture of old service topology and new Listener topology, or the reverse.

## Load and unload

Loading a Plugin means resolving a candidate Graph Generation that contains its contributions, then activating that Graph Generation.

Unloading a Plugin means resolving a candidate Graph Generation without that Plugin, then activating that Graph Generation.

Unload removes every executable binding owned only by the retired Plugin, including Terminal Provider implementations, Layers, Listeners, and generation-local Plugin Resource projections. No stale handler remains reachable after the new Graph Generation becomes active.

Dependency closure and restart policy determine which other Plugins restart. Unchanged compatible Plugin instances remain active.

## Listener topology

Listener declarations are declarative metadata before activation.

A resolved Listener entry contains:

```text
subscription id
owning Plugin
owning Component/function
source Event type + version
payload schema + projection
dependency edges
failure policy
required and maximum authority
Graph Generation attribution
Runtime Provider or embedded handler binding
```

The Event bus executes the resolved Listener entries. Plugin `start` and `stop` may manage Plugin-local resources, but they are not the canonical source of Listener registration or removal.

Generated and manually constructed Core fixtures use the same Event transport, Effective Authority attenuation, Listener dependency ordering, recursion policy, and Event Delivery failure policy.

## Service topology

Service chains are resolved before activation. Layers wrap the next resolved node through the canonical Continuation mechanism. A Plugin instance cannot insert an extra Layer by mutating live routing during `start`.

Adding, removing, or reordering a Layer therefore requires a new Graph Generation.

## Reconciliation

Live reconciliation compares complete Graph Generations, not independent mutable registries.

The transition plan accounts for changes to:

- Plugin manifests;
- Component and function ownership;
- service chains and Effective Layer Order;
- Listener entry points and Listener dependency order;
- Plugin Resources and resolved configuration;
- Plugin Runtime bindings and lifecycle policy.

A topology change that changes executable bindings changes Graph Generation identity.

## Inspection

Inspection reports the active Graph Generation and its executable topology. It is possible to determine which Plugin owns a callable node, which Layers precede it, which Listeners consume an Event, and which Graph Generation installed each executable binding without inspecting Plugin-local mutable state.

## Invariants

- One resolved Graph Generation is the source of truth for executable topology.
- Plugins provide behavior, state, lifecycle, and declarations. The kernel owns routing.
- Components and functions are executable nodes.
- Layers are service interposition in resolved invocation chains.
- Listeners are Event-triggered entry points.
- Hooks lower to Layers or Listeners and have no separate runtime.
- `PluginInstance` does not own global topology.
- Factories construct Plugin implementations and do not register routing.
- Load and unload activate complete candidate Graph Generations.
- Graph Generation replacement is atomic across service and Listener topology.
- Failed staging leaves the active Graph Generation unchanged.
- Retired Graph Generations leave no reachable Listener or service binding behind.
- Unchanged compatible Plugin instances may survive a Graph Generation transition.

## Validation

Regression coverage proves Plugin addition and removal, service and Listener replacement, failed candidate rollback, retained Plugin instances, retired handlers, resource-only Plugins, Listener DAG rejection, and Graph Generation attribution. Static SDK coverage proves generated Listener declarations and handlers use the same resolved activation path.

## Selectable-generation extension

The implemented baseline activates one live Graph Generation and retires replaced executable state. `spec/selectable-generations.md` defines the proposed extension for keeping multiple compatible generations resident under one long-lived kernel.

That extension does not weaken this document's topology rule. Each root still observes exactly one complete Graph Generation. Selection happens at root entry; dispatch never assembles a mixed topology from several generations.
