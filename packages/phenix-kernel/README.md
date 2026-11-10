# Kernel Graph Contracts

Kernel composes the existing Graph engine. It discovers eligible providers,
resolves consumer-local imports and orders composition, but adds no scheduler,
authorization grant or automatic Graph edge insertion. Native composers remain
trusted code.

## Definitions And Imports

- `ContractDefinition { id, contract }` publishes a Graph-owned semantic definition.
- `GraphOffer { id, unit, contract, producer }` names an actual public unit, its
  definition Key and exact producing contribution Key, not an independent shape.
- `ContractRequirement { id, contract, consumer }` declares a consumer-local import.
  `ImportPolicy` supplies optional selections and allow/deny restrictions.

The resolver requires a unique eligible provider or explicit host selection;
missing, ambiguous and ineligible imports reject. Denial wins, and all policy
keys are validated, including unused entries. Selected providers become
composition dependencies. Changing selection changes the resolved import used
by the consumer, without capturing a provider implementation path.

Consumers explicitly bind and wire the selected operation:

```rust,ignore
let selected = context.import("service")?;
let bound = selected.bind(context.graph)?.operation()?;
let request = bound.inlets::<Request, EncodedValue>()
    .port(InletPort::<Request, Request>::new("run"))
    .field(SlotField::<Request, Request>::new("left"))
    .target()?;
let response = bound.output::<Response, EncodedValue>()?;
context.graph.connect(caller.output(), request)?;
context.graph.connect(response, sink.inlet())?;
```

These are the same checked typed views used by Graph operation authoring and the
independently compiled host consumer fixture. Structural views are not delivery
endpoints; `target()` validates the actual leaf/type/mode. Owner typing does not
prove a custom erased physical multi-port definition's reconstruction or behavior.
Local connections require Rust type identity; encoded connections allow different
Rust types with equal complete schema/codec descriptors. Graph remains the only
compatibility checker, and descriptor equality does not prove codec honesty.

Conflicting definitions, including unused definitions, reject. Final Graph
compilation validates actual explicit claims and requests; Kernel checks the
compiled offers and exact producer origins. Current contracts require the complete
single-output interface, not an output subset with diagnostic extensions. Equal
shape without an explicit matching claim is insufficient. Native composition code
can ignore a resolved import; final validation does not prove arbitrary wiring.

Compose callbacks run under distinct producer child origins within the leased
plugin revision. Foreign or sibling producers cannot launder a compatible claim.
Origins survive packaging/cloning; layout retargeting preserves handler origin,
while replacement attributes the new boundary to the replacer. Module boundary
origin does not assert ownership of every private implementation node. Root Graph
containers have no single provider origin and are not operations.

Graph offers may name a Node or Module public boundary. Typed Modules declare an
independent `InputInterface`, use checked `Module::new(...)?`, explicitly project
leaves and finish with one output source. Existing `graph.replace` supports
compatible cross-kind edits using typed packages or NodeTemplate, with candidate
compilation, explicit claims, request/origin obligations and handle revocation.
The root exposure APIs and raw physical compiler declarations remain available;
they are not alternate runtime operation wrappers.

## Candidate Lifecycle

`ComposeContext::activate` and `export` register candidate-owned factories and
typed artifacts. `KernelHost::prepare` uses the exact staged inventory, validates
the deployment and passes a nonadmitting `Binding` to the binding hook before
starting producers without polling them. Data-only `BuildOptions` carries import
policy and Graph-owned execution limits, not a deployment callback.

`publish` rejects stale/foreign candidates, commits the validated plugin batch,
retires old producers and switches the active generation. Publication serializes
against producer drains, not old explicit waves/read callbacks. Old engine/query
handles remain pinned and usable; retired producer drains cannot admit notices.
Artifacts/actions/producers retain plugin code leases through teardown.

This optional coordinator does not roll back arbitrary effects of native compose,
binding or activation callbacks. Dropping failed candidates releases their owned
resources and preserves the active generation. Raw PluginSet/Kernel callers still
own their sequencing; successful build alone neither commits nor publishes.

## Evidence And Status

`tests/declarative_imports.rs` covers discovery, policy, dependency ordering and
provider switching. `host_portable_contracts.rs` and its independently compiled
consumer exercise typed bound discovery through host publication. Contract offer,
origin, generation, retirement and artifact tests cover actual claims and leases.

The current operation redesign is an uncommitted tree based on `5469c55b`, branch
`impl/operation-interface-20261109`, preserving earlier uncommitted work. Handoff
reports Graph tests/doctests passed; workspace testing hit a portable-host schema
error wording assertion, corrected by main with a subsequent focused host pass.
Final workspace verification, exact implementation commit and independent review
remain pending. These test references are not a final-head pass claim.

See [Typed Operation Interfaces](../../docs/grouped-authoring.md), the
[implementation record and removal ledger](../../docs/operation-interface-redesign.md)
and [Kernel lifecycle](../../docs/kernel.md). Run
`cargo test -p phenix-kernel -p phenix-plugin --locked` to exercise package tests.
