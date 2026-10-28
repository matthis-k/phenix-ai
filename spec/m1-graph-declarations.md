# G1/G2 graph declaration contract and implementation scope

Status: historical G1/G2 graph-v1 development contract. Verify present `phenix.graph.v1` behavior in current source and conformance tests; do not infer branch status. For future graph dataflow and patches, use [Graph Architecture v2.4](graph-architecture-v2.4.md) (proposed, not implemented).
Base: accepted S3b at `e78b1aa87614d48a769457c434857af1d4ab036c`
Owners: `phenix-contract/src/graph.rs`, `phenix-contract/src/descriptor/static_metadata.rs`, `phenix-macros/src/`, `phenix-artifact/src/lib.rs`, `phenix-kernel/src/graph.rs`, `resolve.rs`, `generation.rs`
Roadmap: [m1-roadmap.md](m1-roadmap.md)

## Declaration rules

`GraphContribution` is the owned, offline Rust representation of one plugin's
event, action and node declarations. `encode` produces a canonical
`phenix.graph.v1` value payload. The decoder rejects unknown fields, unknown
policies, invalid schemas and noncanonical ordering. Sorting declarations has
no effect on their identity.

| Item | Meaning |
| --- | --- |
| `EventDefinition` | Unique event ID and canonical payload schema |
| `ActionDefinition` | Discoverable action ID, description, event and required operation keys |
| `GraphNode` | Plugin-local ID, selected operation, triggering events, predecessor edges, declared writes, join and failure policy |
| `selector` | Empty for a local operation, or the selected import slot or indexed slot |
| `after` | Qualified `plugin/node` dependency; constrains order but never activates a node |
| `writes` | Qualified `plugin/property` state destination |
| `JoinPolicy::AllActive` | Wait for all predecessors that participate in the same epoch |
| `FailurePolicy` | `Skip` or `FailRoot`; execution implementation belongs to G3 |

The kernel identifies graph nodes by plugin and local node ID. Contributions
may have disconnected components. Repeated global event IDs and action IDs
reject, including identical duplicates.

## Rust authoring

The public SDK now exposes const-friendly `StaticGraphContribution`, `StaticGraphNode`, `StaticEventDefinition` and `StaticActionDefinition`. `#[event(id = "...")]` uses the type's `Portable` schema. `#[action(...)]` declares discovery metadata and required operation keys. Both generate associated constants without handwritten metadata bytes. The plugin assembles a static declaration from those constants. `#[plugin(graph(GRAPH))]` emits its canonical bytes into an ELF `.phenix.graph` section. The verifier inspects the section before `dlopen`, rejects duplicates, truncated or noncanonical payloads, and checks that the declared graph owner equals the package owner. The artifact SHA-256 binds the graph section to the selected executable.

## Preparation

`CompiledGraph::compile` normalizes contributions and validates graph
structure without executing plugin code. It reuses `dag::order` for cycle
detection and stable ordering. A missing predecessor is an error. Two nodes
that write the same state destination must have a transitive order between
them. Static topological order is diagnostic data, not an execution schedule.

`activation_order` projects the graph onto nodes whose trigger set intersects a
supplied admitted event set. It removes dormant predecessors before ordering.
An `after` edge alone never activates its successor. This is a pure planner,
not the G3 event dispatcher.

`Prepared::compile_graph` resolves each node's operation using the existing
M1 selected import bindings, operation table and caller grants. The exported
compiler also accepts a trusted target lookup for isolated tests. Caller
claims inside graph declarations confer no authority.

Graph identity is the SHA-256 digest of a domain prefix plus sorted canonical
declaration bytes. That digest changes with topology, triggers, schemas,
selected operation declarations or failure policies. The selected generation
digest includes the graph identity, selected bindings and operation grants.

## Native conformance fixtures

The external examples workspace now includes one shared task-contract crate and
three independent `cdylib` plugins: `example-graph-a`, `example-graph-b`,
and `example-graph-c`. A and B trigger from `example.graph.start`.
C depends on both; D depends on C in the same external plugin. The shared event derives its schema from a Rust struct.

`phenix-m1-graph` verifies each native artifact, compiles their contributed
multi-root DAG with `Prepared::prepare`, compares graph and generation
identities across selection permutations, discovers root-authorized actions,
and rejects a missing graph-node grant. A Linux constructor witness checks
that preparation executes no native constructor. G3 now submits a scoped action through the kernel, observes serial output
updates, and checks committed-state fan-in, failure propagation and resource
attenuation. The ordinary direct-call check remains separate evidence.

Nix owns the conformance gate as `checks.x86_64-linux.m1-graph`; the
release evidence includes the graph report.

## Evidence and remaining work

The source includes tests for canonical round-trips, reordering,
duplicates, missing triggers and anchors, cycles, denied targets, disconnected
roots and conflicting writers. Exact-head CI has not established acceptance.

The definition-only follow-up implements the remaining package mechanism.
`#[plugin(profile = "definitions", graph(GRAPH))]` exposes canonical graph
bytes. The package record binds an optional graph file and digest; package
identity includes its contents. Definitions contribute events/actions and
retain their existing restriction against executable nodes. The existing
resolver combines their declarations with independently authorized native nodes.

The external gate rejects graph digest tampering, re-signed native/definition
owner mismatches, executable definition nodes, handler schema mismatch, missing
properties and ungranted writes before constructors. It executes a separate
partial action with only A/C active. B's committed value remains available to C;
D stays dormant despite its ordering edge. Follow-up shipping CI and uploaded
exact-head evidence remain before acceptance.

G3 owns activation and dispatch. G4 owns pending calls, cancellation and live
state triggers. No agent-specific entry point or second dispatcher is added.
