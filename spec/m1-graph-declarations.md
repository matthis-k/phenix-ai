# G1/G2 graph declaration contract and implementation scope

Status: partial implementation on `impl/m1-graph-declarations`
Base: accepted S3b at `e78b1aa87614d48a769457c434857af1d4ab036c`
Owners: `phenix-contract/src/graph.rs`, `phenix-kernel/src/graph.rs`
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

## Preparation

`CompiledGraph::compile` normalizes contributions and validates graph
structure without executing plugin code. It reuses `dag::order` for cycle
detection and stable ordering. A missing predecessor is an error. Two nodes
that write the same state destination must have a transitive order between
them. Static topological order is diagnostic data, not an execution schedule.

`Prepared::compile_graph` resolves each node's operation using the existing
M1 selected import bindings, operation table and caller grants. The exported
compiler also accepts a trusted target lookup for isolated tests. Caller
claims inside graph declarations confer no authority.

Graph identity is the SHA-256 digest of a domain prefix plus sorted canonical
declaration bytes. That digest changes with topology, triggers, schemas,
selected operation declarations or failure policies.

## Evidence and remaining work

The current changes add tests for canonical round-trips, reordering,
duplicates, missing triggers and anchors, cycles, denied targets, disconnected
roots and conflicting writers. CI must still execute them on the PR head.

This is **not G1 or G2 acceptance**. The following still require executable
work:

- Embed graph contributions in native/definition-only verified artifact
  metadata, with constant Rust SDK and macro generation.
- Prove external independently compiled plugins and malformed offline artifacts.
- Include resolved provider and grant identities in the generation digest.
- Bind action inspection and event admission to authenticated root authority.
- Exercise conflicts, denied contributions and ambiguous selections against
  actual `Prepared` fixture packages rather than only pure graph tests.

G3 owns activation and dispatch. G4 owns pending calls, cancellation and live
state triggers. No agent-specific entry point or second dispatcher is added.
