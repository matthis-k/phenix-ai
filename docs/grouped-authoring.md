# Typed Operation Interfaces

## Current Model

This guide describes the uncommitted operation redesign on
`impl/operation-interface-20261109`, based on `5469c55b`. Final-commit tests and
independent review remain pending; see the [implementation record](operation-interface-redesign.md)
for the removal ledger and verification gates.

An `Operation<I, O, InputMode, OutputMode>` is authoring metadata over a Node
declaration or a public Module boundary. It does not execute and does not survive
compilation. Both implementations expose `inlets()` and one `output()` through
the same checked endpoint types. Modules flatten through the existing compiler.

An input layout describes the slot frontier of one port. An interface declares
which ports are visible. A Module projection maps a public leaf to one or more
existing physical input targets. Graph edges retain dataflow; physical input
ports retain readiness and buffering policy. Public grouping adds no runtime
port, join, buffer, barrier, or scheduler.

For a portless source feeding a one-port atomic node, the inferred API is:

```rust,ignore
let source = graph.node("source", || 42_i64)?;
let sink = graph.node("sink", |x: i64| x + 1)?;
graph.connect(source.output(), sink.inlet())?;
graph.trigger("start", [source.operation().start()])?;
// After deployment: engine.fire("start", vec![])?;
```

A zero-argument closure has the `NoPorts` input interface: no physical input
port, no unit slot, and no `inlet()` shorthand. It runs only when explicitly
started, once per firing of its trigger, through the ordinary scheduler queue.
Source starts consume no trigger payloads. Existing `PublicInput` triggers keep
their payload arity and behavior. Cancellation and activation budgets apply to
source invocations exactly as to input-driven invocations.

The same `.start()` selector works on an instantiated `Module<NoPorts>` operation.
Compilation selects the zero-input child producers reachable upstream from its
exposed output, including nested children. Disconnected sources remain dormant;
there is no module runtime wrapper or hidden activation node. Reachability is
structural, not conditional: normal output selection controls subsequent delivery.
Modules whose output has no reachable zero-input producer reject activation.

Raw actions now observe `InvocationCtx::source_port_id() -> Option<u16>`: `None`
for a source start and `Some(port)` for input-driven work. Port-specific dispatch
must guard `Some(port)` rather than assuming every invocation has an input port.
Source arguments are an empty raw argument map, not a prepared unit value.

Structured ports return addressable views. A view is not itself a delivery
endpoint: `target()` succeeds only for an exact leaf in the selected layout.
Directly connecting a structural view is a compile error; resolving its leaf is
an explicit runtime check, not compile-time frontier proof.

```rust,ignore
#[derive(Clone, LocalPortLayout, InputFields)]
struct AddArgs { left: i64, right: i64 }

let add = graph
    .node("add", |x: AddArgs| x.left + x.right)?
    .input(Layout::fields())?;
graph.connect(
    left.output(),
    add.inlet().field(AddArgs::SLOT_LEFT).target()?,
)?;
```

Nested fields are selected by composing typed descriptors. Manual descriptors
remain available for custom layouts; Graph validates them against the actual
declaration and chosen layout. Atomic, fields, bounded-depth, deep, and custom
layouts all use the same view and endpoint representation.

The optional `InputFields` derive owns the typed `SLOT_*` descriptors for named
structs, including generic structs. Use it alongside `LocalPortLayout` for local
authoring or `PortLayout` for encoded authoring; a type deriving both layouts
derives `InputFields` only once. Neither layout derive generates descriptors.
Compose nested fields with `Request::SLOT_ADDRESS.then(Address::SLOT_CITY)`.
Descriptors use raw Rust field names (without `r#`), independent of atomic/flatten
directives and codec or `PortArgs` slot renames. Resolving a descriptor still
validates the selected layout's actual frontier. Without the `derive` feature,
manual `SlotField` descriptors remain available. Layout selections are
frozen in admitted address books. Node reconfiguration creates a new revision;
old endpoints are rejected on use rather than statically revoked by Rust.

## Modules

Declare the public layout independently of child operations. Project each public
leaf explicitly and select exactly one output source when finishing the Module:

```rust,ignore
let interface = InputInterface::single(Layout::<FArgs>::fields())?;
let mut f = Module::new("f", interface)?;
let a = f.node("a", |x: AArgs| x.first + x.second)?
    .input(Layout::fields())?;
let b = f.node("b", |x: i64| x * 3)?;

f.connect(
    b.output(),
    a.inlet().field(AArgs::SLOT_SECOND).target()?,
)?;
f.expose(
    f.inlet().field(FArgs::SLOT_X),
    a.inlet().field(AArgs::SLOT_FIRST).target()?,
)?;
f.expose(f.inlet().field(FArgs::SLOT_Y), b.inlet())?;
let package = f.finish(a.output())?;

let f = graph.instantiate("f", &package)?;
graph.connect(external.output(), f.inlet().field(FArgs::SLOT_X).target()?)?;
```

The public layout need not equal any child layout. `B -> A.second` remains an
internal edge; only `A.first` is visible through `x`. A public leaf may project
to several compatible destinations, but repeated identical destinations are
rejected. Exposing multiple children through a port does not combine their
readiness policies.

For a statically declared multi-port operation, the owner type of each port
descriptor prevents using it with a different interface:

```rust,ignore
struct ServicePorts;
impl PortInterface for ServicePorts {
    const PORT_COUNT: Option<usize> = Some(2);
}
const REQUEST: InletPort<ServicePorts, i64> = InletPort::new("request");
const CANCEL: InletPort<ServicePorts, bool> = InletPort::new("cancel");
let request = operation.inlets().port(REQUEST);
let cancel = operation.inlets().port(CANCEL);
graph.connect(request_source.output(), request.target()?)?;
graph.connect(cancel_source.output(), cancel.target()?)?;
```

Only `SinglePort<...>` exposes `inlet()`. `NoPorts` and custom multi-port
interfaces do not implement that shorthand. Port cardinality and slot layout are
checked against declarations; the compiler remains responsible for full public
projection resolution and physical graph validation.

Rust descriptor ownership does not prove reconstruction or behavior of a custom
erased physical multi-port definition. Custom `PortInterface` validation and
actual leaf checks remain necessary. The root Graph is a container with an
optional exposed result, not an operation. Its `expose_input`,
`expose_input_field` and `expose_output` coexist with typed Module projection.
That dual path, reachable through Module's builder Deref, is a residual API risk;
`finish` rejects inputs outside the declared interface.

The public `ModuleInput`, `ModuleOutput`, `ModuleInputTarget`,
`ModuleOutputSource` and `ModuleDefBuilder` remain raw physical compiler API, not
alternate typed operation wrappers. Use explicit typed Modules for public
layout/projection authoring. The raw API remains available for compiler/manual
declarations and differs from a typed-only canonical surface.

## Contracts And Encodings

Contract-bound authoring uses the same addressing surface. It retains a borrow of
Graph authority and revalidates the complete effective contract when resolving a
leaf, so a pending layout change cannot leave a stale unrestricted view:

```rust,ignore
let bound = graph.bind_contract("model", &contract)?;
let model = bound.operation()?;
let request = model.inlets::<(), EncodedValue>()
    .port(InletPort::<(), Request>::new("run"))
    .field(SlotField::<Request, Request>::new("value"))
    .target()?;
graph.connect(caller_request.output(), request)?;
```

`caller_request` must be encoded for the shown encoded destination.
Locally represented endpoints use Rust type identity. Encoded endpoints
use complete canonical schema/codec identity and may connect independently
compiled Rust types. Local-to-encoded wiring is rejected unless an explicit
conversion operation performs that transformation. Graph owns compatibility;
Kernel delegates contract imports to Graph.

An operation has one output; its action may emit zero or one correctly typed
payload at slot 0. Fanout shares that payload, conditional routing selects edges,
and distinct tuple members require explicit projection operations. Those are
separate invocations, not one atomic multi-invocation emission. Scheduler batch
atomicity does not roll back action side effects.

Existing `graph.replace` accepts compatible Nodes or Modules using a
`NodeTemplate` or owned/borrowed `TypedModulePackage`. Cross-kind replacement
compiles a candidate, requires complete shape and explicit matching claims,
preserves origin/request obligations and revokes old handles on success. Failed
replacement preserves the original graph. Equal shape is not behavioral proof.

## Historical Migration

This section names superseded APIs only to explain the migration. They are not
current examples or supported alternatives. Previous grouped guides and the
PR #1251 inventory used string/slot and representation-specific endpoint
accessors; those authoring methods have been removed from current Graph code.
The low-level compiler still has physical `InputSlot` and declaration metadata,
but those are not Node/Module operation endpoints.

## Evidence

- `module_projection_semantics`: typed `F(x,y) = A(x,B(y))`, A/B/C-to-D with bias,
  nested/repeated Modules, a bare Node with the same public shape, physical node
  counts, and independent projected child scheduling.
- `typed_operations`: atomic and structured views, non-leaf rejection, depth and
  custom frontiers, zero and multi-port definitions, per-port projections,
  contract restriction/revalidation, stale declaration handles, local/encoded
  mode independence, and compile-fail cases.
- `grouped_wire_authoring`: independent portable Rust definitions, typed nested
  selectors, contract-restricted projection, and encoded/local mismatch checks.
- `module_slots`: literal dotted boundary names and independent grouped routes.
- `operation_substitution`: existing replacement verb across kinds, candidate
  rollback, explicit claims, origins, requests, revocation and patch provenance.

These are source-derived coverage references, not a final-head pass assertion.
