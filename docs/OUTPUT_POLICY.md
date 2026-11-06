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

A portable Patch configures a policy on an existing output port:

```rust
patch.output_policy(
    OutputRef { node: "model".into(), port: "reply".into() },
    OutputPolicy::Select { selector: "route.reply".into() },
);
```

- Default `OutputPolicy::Broadcast`: all outgoing edges receive the emission.
- `OutputPolicy::Select`: a host-supplied function receives output port, value and candidate destinations, and returns a subset of those destinations.
- An empty selection is valid. No downstream node is activated through omitted edges.
- Unknown or duplicate destinations are rejected. The complete set is validated **before** any delivery; a failed policy cannot cause partial fan-out.
- Only compiled edges may be selected. The policy cannot add graph topology or authorize capabilities.
- Policy changes are ordinary Patch edits (`SetOutputPolicy`), resolved at compilation with the same conflict and deterministic identity rules as other patch edits.
- Local `ActionHost::route` implements the policy. Missing policy implementation **fails closed** rather than broadcasting.

The policy's result selects `InputRef` endpoints. These are the *destination portions* of canonical edges; identity remains the source-output / destination-input pair. There are no user-managed edge IDs.

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
- Priority-arbitrated patch conflicts and advanced splice resolution
- Optional `patch!` authoring macro and graph visualization metadata

These must extend the **same** Patch IR, compiler, GraphSession and host execution bridge. The first migration target is a small, correctly routed graph with trivial leaf operations, *not* full agent functionality.
