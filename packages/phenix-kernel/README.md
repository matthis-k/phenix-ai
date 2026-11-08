# Kernel Graph Contracts

`Kernel::build` composes the existing graph engine. It does not add a scheduler,
provider discovery, authorization or automatic edge insertion.

Three optional contribution kinds integrate graph-owned contracts without adding
fields to the existing kernel `Contribution` struct:

- `ContractDefinition { id, contract }`: plugin A publishes a `GraphContract`.
- `GraphOffer { id, unit, contract }`: plugin B names an actual graph unit path and
  the definition's owner-qualified `Key`. There is no independent actual shape.
- `ContractRequirement { id, contract, offer }`: plugin C requests a definition
  from one explicitly selected, owner-qualified offer. Missing selections fail;
  there is no lexical provider fallback or ambiguous implicit selection.

The trusted host selects the snapshots and offers. A consumer's compose closure
must wire the selected offer's `unit` path, using `Contribution::requires` to
order composition when needed. A requirement is validation of that explicit
target, not proof that arbitrary native compose code used it. The host must keep
the wiring configuration and requirement selection consistent. In particular,
changing only a requirement does not rewire an existing closure.

Definitions and references are resolved before composition. Conflicting same-ID
definitions are rejected, including unused definitions. After every compose
callback, offers and selected requirements are registered using Graph's
`require_contract`. Graph compilation validates the requests and all actual
claims; the kernel then checks the selected immutable `CompiledGraph::find`
results using Graph's contract checker. Extra endpoints follow Graph's
matching rules, not a kernel matcher. A matching shape without an explicit claim,
a missing path, a conflicting definition or an invalid final boundary fails
before `Deployment::build` and before producer activation in `Kernel::start`.

Each compose callback runs under its leased plugin revision's opaque
`GraphOrigin`. Offers must resolve to actual compiled declarations with that
same origin, so a provider cannot advertise another plugin's path or retain
ownership after a foreign replacement merely because its contract still matches.
Origins survive packaging and clones; node layout retargeting retains handler
origin, while successful replacement records the replacing origin. Module origin
identifies the instance boundary's instantiator, not ownership of every private
handler inside the imported package. This is trusted-host provenance, not an
authorization grant or a behavioral proof. Composite root graphs have no single
provider origin; offer named nodes or module instances instead.

The offer's path may address a node or a module's public boundary. The provider
must attach its explicit claim through `GraphBuilder::implements` or
`implements_public`. Consumers can use named endpoint lookup and `connect_wire`
with independently defined request/response types that have equal complete
portable schema descriptors. Native local endpoints still require Rust type
identity. A contract does not prove handler behavior or codec honesty.

## Revisions And Staging

Kernel retains the exact input snapshots and composition handles until after
engine teardown. Reload/unload does not redirect an old graph to new code. A
fresh build independently validates the new compiled units; contract IDs are not
generation leases.

A host can call `prepare_reload`, substitute `stage.candidate()` into the input
snapshots, and call `Kernel::build` before committing the plugin stage. A build
failure allows dropping the stage while the old plugin set and kernel remain
usable. A successful build does not commit the stage or activate producers.
`PluginSet::commit` still rejects stale stages.

There is **no integrated atomic plugin/kernel activation coordinator**. The host
owns commit/publication/start sequencing, must handle commit or start failure,
and must not publish a candidate kernel after a failed commit.

## Evidence

`tests/graph_contracts.rs` runs real three-plugin A-definition/B-provider/C-consumer
execution with independently declared portable codecs and both request/response
edges using `connect_wire`. It also covers failed candidate builds, old/new
behavior after reload/unload, drop counters, missing paths and definitions, and
validation before producer activation. Existing lifecycle/storage/derived tests
remain applicable; these tests do not establish an atomic coordinator.

`tests/contract_offers.rs` independently exercises valid node/module offers,
missing claims, missing paths, explicit selections and late composition edits.
It also rejects foreign-owned node/module paths and foreign replacements with
otherwise compatible claims.

Run `cargo test -p phenix-kernel -p phenix-plugin` from the workspace root.
