# G1/G2 graph declaration contract and implementation scope

Status: partial implementation on `impl/m1-graph-declarations`
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
C depends on both. The shared event derives its schema from a Rust struct.

`phenix-m1-graph` verifies each native artifact, compiles their contributed
multi-root DAG with `Prepared::prepare`, compares graph and generation
identities across selection permutations, discovers root-authorized actions,
and rejects a missing graph-node grant. A Linux constructor witness checks
that preparation executes no native constructor. It also checks a normal
direct call after loading, which does **not** prove G3 event dispatch.

Nix owns the conformance gate as `checks.x86_64-linux.m1-graph`; the
release evidence includes the graph report.

## Evidence and remaining work

The source includes tests for canonical round-trips, reordering,
duplicates, missing triggers and anchors, cycles, denied targets, disconnected
roots and conflicting writers. Exact-head CI has not established acceptance.

**G1/G2 remain unaccepted** until the following gaps have executable evidence:

- Malformed graph sections and mismatched graph/package identities need an
  independent tampering fixture, beyond owned decoder unit tests.
- Definition-only graph contributors have no packaged metadata mechanism yet.
- An authenticated event-admission API needs to validate payload schemas,
  resource scopes and root authority, rather than trusting event ID strings.
- An action's advertised authority requirements must be enforced when the
  frontend submits it. Discovery currently filters root-granted actions.
- Node schemas and write declarations need comparison against their selected
  operation and property definitions. The current compiler verifies existence
  and authority but not all cross-schema constraints.

G3 owns activation and dispatch. G4 owns pending calls, cancellation and live
state triggers. No agent-specific entry point or second dispatcher is added.
