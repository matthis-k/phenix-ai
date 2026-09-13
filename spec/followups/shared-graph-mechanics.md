---
status: planned
parent: pull/514
---

# Shared graph mechanics

## Goal

Move generic directed-graph mechanics to one maintained substrate while keeping Phenix graph semantics, ordering policy, authority, provider selection, lifecycle rules, and diagnostics repo-owned.

This supersedes PR #514's narrow `petgraph` rejection. That decision evaluated only `ComponentGraph`; the same DFS/topological/reachability mechanics are currently reimplemented across core and plugins.

## Theoretical problem

The shared problem is a directed graph with some graphs constrained to be acyclic:

- cycle detection and concrete cycle witnesses
- topological ordering
- ready-frontier calculation
- reachability and reverse reachability
- dependency completion/readiness
- topological levelization

Phenix-specific meaning stays outside the graph library.

## Decision

Use `petgraph` internally for graph representation/algorithms. Do not expose petgraph node indices or graph types in public Phenix contracts.

Determinism remains a Phenix invariant. Where a petgraph algorithm does not define the required lexical tie-break, compute the eligible frontier through petgraph and apply the existing stable Phenix ordering at that boundary. Concrete cycle diagnostics remain Phenix-owned and must preserve current externally tested paths/messages.

Prefer a small crate-private adapter such as `graph_util` only if it removes repeated construction/index mapping. Do not create a second generic graph framework around petgraph.

## Scope

Migrate all generic graph traversal found in this audit:

### Core

- component import/provider dependency cycle validation
- plugin activation dependency ordering
- event subscription dependency ordering/levelization
- persistence bootstrap dependency cycle validation
- reconciliation dependency/reverse-reachability traversals where they are purely structural

### Plugins

- hooks dependency ordering
- planning DAG validation
- execution worker-task cycle/readiness traversal
- repository-worker dependency traversal/eligibility graph mechanics

## Keep Phenix-owned

- `ComponentId`, `PluginId`, service/interface identities
- provider selection and fallback policy
- authority attenuation/gates
- listener/event semantics and receipt causality
- execution task state transitions
- hook failure policy
- planning semantics
- repository work-priority policy
- reconciliation actions and reload/migration policy
- exact public error taxonomy and semantic diagnostics

## Implementation constraints

- No petgraph types cross public API, serde, persistence, plugin ABI, or SDK boundaries.
- Preserve lexical deterministic ordering for equivalent inputs independent of insertion/registration order.
- Preserve or improve concrete cycle-path diagnostics.
- Do not change provider tie-breaking, graph generation identity, event ordering semantics, task readiness, or worker selection semantics.
- Remove old DFS/Kahn implementations after each call site migrates; no fallback/parallel graph implementation remains.
- Prefer `StableDiGraph` only if stable node identity is actually needed; otherwise use the simplest directed graph representation.

## Acceptance criteria

- [ ] `petgraph` is a workspace dependency and only internal Phenix adapters expose it.
- [ ] Every graph site listed above uses the shared substrate or documents a concrete semantic reason it cannot.
- [ ] No local generic DFS/toposort/reachability implementation remains at migrated sites.
- [ ] Existing deterministic ordering tests remain green.
- [ ] Add registration-order permutation tests for component, hook, event, planning, execution-task, and persistence-bootstrap graphs.
- [ ] Cycle diagnostics retain the concrete offending path where currently promised.
- [ ] Graph generation identity is unchanged for semantically equivalent inputs.
- [ ] Measure deleted vs added hand-maintained Rust LOC and call out any site where the adapter is larger than the removed mechanics.

## Non-goals

- Replacing reconciliation policy with an incremental-computation framework such as Salsa.
- Replacing Phenix state machines with graph-library state.
- Changing public graph/domain DTOs.
- Introducing async execution.
