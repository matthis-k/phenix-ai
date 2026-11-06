# Output policies: selective delivery over existing edges

## Decision

Graph **input policies** decide when a node may activate based on its input slots. Graph **output policies** decide which of an output port's already compiled outgoing edges receive each emitted value. These are dual, independent responsibilities. A node does its single task and emits values; it does not name downstream nodes or manage workflow sequencing.

```text
                          OutputPolicy::Select("route.reply")
Model.out.reply ─────┬──────────────────────► Tool.in.call
                     └──────────────────────► Finish.in.reply
```

The policy can select `[Tool]`, `[Finish]`, both, or neither, independently **for each emission**. No dynamically invented edge, second scheduler, special branching node, or implicit store-trigger is needed.

## Current API and guarantees

A portable Patch attaches a **selector function reference** to an output port:

```rust
patch.output_policy(
    OutputRef { node: "model".into(), port: "reply".into() },
    OutputPolicy::Select { selector: "route.reply".into() },
);
```

The resolved function has one responsibility:

```rust
fn route(
    &mut self,
    selector: &str,
    source: &OutputRef,
    value: &Value,
    existing_edges: &[Edge],
) -> Result<Vec<Edge>, Failure>;
```

It receives the outgoing edges **already declared and compiled** for that port, and returns **any subset**: 0, 1, 2, ... or all of them. There is deliberately **no `SelectOne` enum or hardcoded cardinality policy**. A provider-selection implementation may choose exactly one, but that is *its logic*, not a constraint of the graph engine.

- `OutputPolicy::Broadcast` sends the value to every connected edge by default.
- `OutputPolicy::Select` invokes the registered function. It may inspect the output and authorized store state; it does not grant authority.
- Returning no edges is valid and leaves all downstream branches inactive for this emission.
- Returned edges must be distinct members of `existing_edges`. Nonexistent or duplicate edges are rejected.
- All selected edges and payload schemas are checked **before delivery**, preventing partial fan-out.
- The same compiler, readiness scheduler, Patch edit `SetOutputPolicy`, and deterministic generation identity handle both broadcast and selective delivery.
- A missing selector host fails closed; it never silently broadcasts.

### Example: five compatible providers

```rust
// All five have the same input schema.
fn choose_provider(
    request: &Value,
    existing_edges: &[Edge],
) -> Vec<Edge> {
    let selected = select_provider_name(request); // domain policy
    existing_edges.iter()
        .filter(|edge| edge.to.node == selected)
        .cloned()
        .collect()
}
```

Only that provider receives this value because the function happened to return one edge. Another function could return all five for replication, two for a race, or zero to suppress delivery. The graph remains unchanged, and no extra provider-selection node is required.

The returned values are full canonical `Edge { from, to }` objects, not merely destination slots, so membership is exact.

## Kernel native-policy binding — **not yet implemented**

The new kernel's `NativeGraphRun` can execute ordinary broadcast outputs. It does **not yet** execute a custom native output selector.

Required implementation, without adding another graph engine:

1. Resolve each policy selector to one **verified** selected plugin operation during graph preparation. Store binding and identity with the compiled generation. A selector string is not a capability.
2. Route an emission through that selected operation using the existing native-call dispatcher. Inject only the same attenuated authority, resource and permitted store access enforced for ordinary node operations.
3. Support policy `Ready` and `Pending` outcomes using the existing native pending ticket/physical-drain lifecycle, without inventing a second event loop.
4. Decode the selected destinations using a portable typed result; validate exact membership, uniqueness and schema compatibility before propagating. A policy failure fails its wave; other waves remain independent.
5. Add native tests for authorized store-dependent choice, denied store reads, invalid endpoint, cancellation during pending routing, and out-of-order completion.
6. Refine per-wave coloring/settlement to mark **unselected** outgoing edges closed for that wave. A downstream join must not wait indefinitely for a branch deliberately omitted by an output policy.

Until these gates are implemented, custom policies remain usable in the local graph engine with an explicit `ActionHost::route`, but not through the production native-host integration.

## Still deferred for fast migration

- Recursive module definitions/flattening (when added, only published module ports cross boundaries; output policies still operate on canonical compiled endpoints)
- Rich slot policies beyond `Readiness::{All,Any}`, buffering policies and custom joins
- Native selected-provider routing until the selector binding, pending-policy lifecycle and authorized store access have been implemented and tested
- Priority-arbitrated patch conflicts and advanced splice resolution
- Optional `patch!` authoring macro and graph visualization metadata

These must extend the **same** Patch IR, compiler, GraphSession and host execution bridge. The first migration target is a small, correctly routed graph with trivial leaf operations, *not* full agent functionality.
