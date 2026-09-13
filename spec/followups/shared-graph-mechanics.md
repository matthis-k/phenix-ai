---
status: partial
parent: pull/514
---

# Shared graph mechanics

## Goal

Move generic directed-graph mechanics to `petgraph` while keeping Phenix graph semantics, ordering policy, authority, provider selection, lifecycle rules, and diagnostics repo-owned.

#514 intentionally kept `ComponentGraph` representation and deterministic domain policy Phenix-owned. #519 does not reverse that decision. It applies `petgraph` only to repeated generic traversal, cycle, reachability, and topological mechanics inside Core and first-party plugins.

## Theoretical problem

The shared problem is a directed graph with some graphs constrained to be acyclic:

- cycle detection and concrete cycle witnesses
- topological ordering and ready-frontier calculation
- reachability and reverse reachability
- dependency completion as a structural predicate
- topological levelization

Phenix-specific meaning stays outside the graph library.

## Decision

Use `petgraph` internally for graph representation and algorithms. The workspace dependency uses `petgraph 0.8.3` with default features disabled and only `std`; the migration does not need `GraphMap`, `StableGraph`, `MatrixGraph`, serde, or Rayon support.

Do not expose petgraph node indices or graph types in public Phenix contracts. No new cross-crate graph utility API is introduced in this PR. `phenix-core::graph_util` remains crate-private for repeated Core mapping/traversal glue. Plugin crates may depend directly on the workspace `petgraph` dependency and keep only the minimal private mapping needed by their domain types. #519 must not create a new shared utility crate or make Core graph helpers public merely to reuse them from plugins.

Determinism remains a Phenix invariant. Petgraph traversal order is never a semantic ordering contract. Reachability is treated as membership. Where an algorithm does not define the required lexical tie-break, derive the eligible frontier through graph mechanics and apply the existing stable Phenix ordering at the call site. Concrete cycle diagnostics remain Phenix-owned and must preserve current externally tested paths and messages.

Do not build a second generic graph framework around petgraph.

## Scope

Migrate the repeated generic graph mechanics identified by the #514 audit follow-up.

### Core

- required component-import dependency cycle validation inside `ComponentGraph`
- plugin activation dependency ordering, including structural runtime-provider dependencies
- event subscription dependency ordering and levelization
- persistence bootstrap dependency cycle validation
- reconciliation dependency and reverse-reachability traversals where they are purely structural

### Plugins

- hooks dependency ordering
- planning DAG validation
- execution worker-task cycle detection and dependency-completion queries used by `runnable_tasks`
- repository-worker dependency blocker and closure traversal used by selection

## Keep Phenix-owned

- `ComponentGraph` domain representation and resolved component/import/provider semantics
- `ComponentId`, `PluginId`, service and interface identities
- provider selection and fallback policy
- authority attenuation and gates
- listener and event semantics and receipt causality
- execution task state transitions and runnable eligibility beyond dependency completion
- hook failure policy
- planning semantics
- repository-worker PR evidence, merged-predecessor semantics, eligibility, and work-priority policy
- reconciliation actions and reload or migration policy
- exact public error taxonomy and semantic diagnostics

## Implementation constraints

- No petgraph types cross public API, serde, persistence, plugin ABI, or SDK boundaries.
- Preserve lexical deterministic ordering for equivalent inputs independent of insertion or registration order.
- Preserve or improve concrete cycle-path diagnostics.
- Do not change provider tie-breaking, graph generation identity, event ordering semantics, task readiness, or worker selection semantics.
- Remove old DFS and Kahn implementations after each call site migrates; no fallback or parallel generic graph implementation remains.
- Prefer `DiGraph` unless a call site proves it needs another representation. Stable node identity is not assumed.
- Keep edge direction explicit at each call site. Dependency meaning belongs to that subsystem, not to a generic helper.
- Keep readiness and priority filters outside petgraph. The library answers structural graph questions; the subsystem owns whether a node is eligible now.

## Migration order

1. Harden the Core helper and persistence-bootstrap pilot.
2. Migrate Core required-import validation and plugin activation ordering.
3. Migrate event ordering and levelization.
4. Migrate reconciliation reachability.
5. Migrate hooks and planning.
6. Migrate execution worker-task cycle validation and structural dependency completion.
7. Migrate repository-worker dependency blocker/closure mechanics last because dependency state is combined with PR evidence and work-priority policy.
8. Remove leftover generic traversal code, run permutation coverage, verify graph-generation identity, and measure the Rust LOC delta.

## Acceptance criteria

- [ ] `petgraph` is a workspace dependency with only the required `std` feature enabled.
- [ ] Core-only shared construction and traversal glue remains crate-private.
- [ ] Plugin migrations use `petgraph` directly or document a concrete semantic reason not to; no new cross-crate graph utility API is added.
- [ ] Every graph site listed above delegates generic graph mechanics to petgraph.
- [ ] No local generic DFS, topological-sort, or reachability implementation remains at migrated sites.
- [ ] Existing deterministic ordering tests remain green.
- [ ] Add registration-order permutation tests for component, hook, event, planning, execution-task, and persistence-bootstrap graphs, plus repository-worker input-order coverage where selection depends on graph structure.
- [ ] Cycle diagnostics retain the concrete offending path where currently promised.
- [ ] Graph generation identity is unchanged for semantically equivalent inputs.
- [ ] Measure deleted versus added hand-maintained Rust LOC and call out any site where the adapter is larger than the removed mechanics.

## Non-goals

- Replacing `ComponentGraph` with a petgraph public/domain type.
- Replacing reconciliation policy with an incremental-computation framework such as Salsa.
- Replacing Phenix state machines with graph-library state.
- Changing public graph or domain DTOs.
- Creating a generic graph crate or public graph utility API.
- Introducing async execution.
