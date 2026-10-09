# Conflict-aware execution graph patches

status: partial implementation (pure slot-order preflight; graph edits not wired)
stage: C of spec/microkernel-composition-roadmap.md
depends-on: #726, #727, Stage B typed-contribution PR
blocks: Stage E application-consumer migration

## Implementation slice: deterministic slot-order preflight

`phenix-core::graph_patch_order::resolve_slot_order` is now executable and
unit-tested. It consumes selected, stable contribution IDs and owner-published
slot identities, validates duplicate IDs and duplicate proposed node IDs,
rejects missing/cross-slot ordering dependencies, detects ordering cycles,
rejects unordered pairs of effectful insertions, and uses deterministic
topological ordering independent of discovery order. Qualified inclusion
slots have separate identities. This is a *pure precursor*; it never mutates
the live generation or chooses providers.

**Not yet implemented:** actual `InsertNode`/`RemoveNode`/`ReplaceNode`,
typed edge edits, schema-extension initialization proofs, slot cardinality,
authority and selected-artifact validation, atomic IR application, resolver
and inspection integration. These are mandatory before merging #729.

## Goal

Plugins declare graph topology and immutable structural patches as data. Core composes every selected patch against an identified base topology before activating an immutable generation. No plugin load-order mutation, direct runtime graph reference or second provider resolver.

## Canonical IR identities

A patch targets a versioned `ExecutionPlanDefinition`, not the provisional `WorkflowTopology` representation used while #726 migrates the agent loop. The plan owner publishes stable `PlanId`, IR semantic revision, `NodeId`, `EdgeId`, `SlotId`, frame-schema revision and typed public entry/output contracts. Node identity is separate from service/provider identity.

A slot is a published IR insertion boundary with allowed operations, cardinality, typed outcomes, authority bounds and ordering semantics. Subplans are expanded with inclusion-site-qualified identities before patch selection. No patch may search for neighboring nodes by name or modify the plan owner's entry, global input/output contracts, scope policy or slot definitions. Such owner-level changes require a new plan revision.

## Closed IR patch algebra

The canonical shape is a versioned patch contribution with stable author identity, target `PlanId`, expected base revision and a finite sequence of typed IR edits. The **only** edits are:

| Edit | Meaning |
| --- | --- |
| `InsertNode`, `RemoveNode`, `ReplaceNode` | Structural node edits with stable identities and typed ports |
| `AddEdge`, `RemoveEdge` | Typed outcome transition edits |
| `BindSlot`, `OrderSlot` | Target owner-published slots and explicit relative ordering |
| `ExtendFrameSchema` | Namespaced typed field addition with an initialization proof |

An authoring helper may express a convenient splice, but it lowers to these edits **before** Core composition. There is no `Splice` or `BindTrigger` step in the canonical algebra. The trigger-to-plan mapping is a separate versioned `EntryBinding` contribution. The plan owner alone can change entry, public I/O contracts and global scope requirements.

Provider binding is not a patch. Around-call wrapping is a Service Layer; event observation is a Listener; policy is canonical configuration. No arbitrary `apply(&mut Graph)` callback is allowed in the Core resolver. `#[phenix(contribute)]` remains an authoring mechanism for the same frozen contribution metadata.

## Ordered slots and typed branches

Two plugins may insert IR nodes at an owner-published slot if its cardinality and allowed operations permit it. Effectful insertions require explicit `OrderSlot` constraints or a slot guarantee that the order is semantically irrelevant. Input enumeration or alphabetical plugin order never defines effect order.

A conditional denial follows a declared typed outcome edge that bypasses protected successors. A successful continue outcome follows an ordinary typed edge. These transitions are part of the compiled IR, not a secondary continuation engine. Composition validates every resulting branch for schema compatibility, authority, frame initialization and permitted exits, including cyclic and Fork/Join paths.

## Conflict semantics

| Contributions | Default |
| --- | --- |
| Different stable nodes/edges | Merge |
| Repeated contribution identity, even with byte-equivalent owner and payload | Invalid, reject during contribution preparation |
| Two explicitly ordered slot insertions | Compose in deterministic topological order |
| Two unordered effectful slot insertions | Ambiguous, fail |
| Two exclusive replacements of one target | Conflict, fail |
| Replace target removed or revision incompatible | Conflict, fail |
| Missing edge contract / unresolved service / authority escalation | Invalid, fail |
| Ordering constraints cycle | Conflict, fail |

A product policy may supply explicit ordering constraints or select a winner among exclusive candidates. Distinguish conflict resolution from validation: priority or selection cannot make an unauthorized, incompatible or structurally invalid graph valid. Ignore disabled candidates only under explicit policy; report their provenance.

## Frame schema, authority, and generation constraints

`ExtendFrameSchema` fields use contributor-owned namespaces, typed revisions and explicit initialization semantics. Core must prove that no reachable path reads an uninitialized field, including loops, branches and joins. Patches cannot remove or reinterpret existing fields or insert capability-bearing values into frame data. An immutable `DataRef` remains an authority-free locator, resolved only through an independently authorized import.

All patches are lowered to frozen IR contributions before the canonical resolver runs. Composition preserves the selected base, author, target and ordering provenance. A candidate is compiled with the same provider bindings that direct Core invocation uses. It never mutates an in-flight root or its generation.

## Resolver algorithm

1. Collect frozen canonical contributions, resolve versioned `ExecutionPlanDefinition` owners, and canonicalize IDs independently of discovery order. Expand namespaced subplans before patching.
2. Resolve the selected plan, base revision and typed IR patch targets; preserve owner-only metadata.
3. Classify edit read/write sets and additive versus exclusive operations.
4. Build conflicts and ordering constraints before applying any patch. Resolve only those conflicts covered by explicit policy.
5. Apply patches atomically to a candidate graph, retaining provenance for every resulting node, edge and selected override.
6. Validate typed outcomes, reachability, cycles, permitted exits, interface imports, selected providers and authority through canonical resolver bindings.
7. Hash normalized graph plus policies into generation identity and compile dispatch handles; executions pin that generation.

The resolver should emit a conflict report with both contributors, target, operation, and possible user resolutions. `phenix inspect graph` must expose effective graph and `graph explain` provenance without requiring a model or agent plugin.

## Implementation ownership and gates

This slice owns `phenix-core` workflow graph, structural patch compiler, graph inspection and an additive resolver integration. It starts after #726 merges and Stage B fixes the contribution envelope. Stage D, which defines plugin kind templates, should not edit Core workflow internals.

Do not reimplement Service Layers, provider planning, event subscriptions or authority mechanisms. No tool-, skill-, prompt- or model-specific branch in Core.

## Required tests

- Two ordered independent insertions compile and run through the original continuation.
- A policy branch redirects to denial without invoking subsequent insertions or the protected tool.
- Reversing input enumeration yields identical normalized graph and generation hash.
- Repeated contribution IDs are rejected by the canonical preparation boundary even when owner, payload and bytes match. Identical content in two distinct IDs is a separate policy decision, not silent identity deduplication.
- Two exclusive replacements, unordered effectful insertions, ordering cycles, missing slots and stale targets reject activation with actionable conflict provenance.
- Explicit policy resolves one compatible conflict, but cannot override schema or authority rejection.
- Provider/Layers dispatch and in-flight generation pinning remain correct after patching.
- A non-agent workflow uses these same operations, with no agent crate loaded.

- A published slot referenced through an inlined subplan's stable inclusion-site-qualified ID resolves; an unqualified or stale reference fails.
- A typed frame extension is accepted only when initialized on all reachable paths and rejects collisions, reinterpretation and capability values.
- A patch cannot alter plan entry, owner-owned public I/O or global scope requirements.
- Fork/Join and cyclic control-flow behavior remain valid after an IR patch, and no alternate Layer, Listener or provider resolver is used.

These tests and implementation are required before this design-stage PR is considered semantically complete.
