---
status: partial
parent: pull/514
---

# Shared graph mechanics

## Goal

Move repeated generic directed-graph mechanics to `petgraph` where doing so deletes local mechanism, while keeping Phenix graph semantics, ordering policy, authority, provider selection, lifecycle rules, and diagnostics repo-owned.

#514 intentionally kept `ComponentGraph` representation and deterministic domain policy Phenix-owned. #519 does not reverse that decision. It applies `petgraph` only to generic traversal, cycle, reachability, or validation mechanics that are separable from subsystem policy.

## Theoretical problem

The reusable mechanical operations are:

- cycle detection and concrete cycle witnesses;
- reachability and reverse reachability;
- acyclicity validation;
- structural dependency membership.

Topological or ready-node ordering is reusable only when the subsystem does not assign semantic meaning to that order. Phenix-specific meaning stays outside the graph library.

## Decision

Use `petgraph` internally when it replaces a real generic graph implementation. The workspace dependency uses `petgraph 0.8.3` with default features disabled and only `std`; the migration does not need `GraphMap`, `StableGraph`, `MatrixGraph`, serde, or Rayon support.

Do not expose petgraph node indices or graph types in public Phenix contracts. No new cross-crate graph utility API is introduced in this PR. `phenix-core::graph_util` remains crate-private for repeated Core mapping/traversal glue. Plugin crates may depend directly on the workspace `petgraph` dependency and keep only the minimal private mapping needed by their domain types. #519 must not create a new shared utility crate or make Core graph helpers public merely to reuse them from plugins.

Petgraph traversal order is never a semantic ordering contract. Reachability is membership. Existing subsystem ordering remains explicit at the owning call site when order affects lifecycle, concurrency, readiness, priority, or public behavior. Concrete cycle diagnostics remain Phenix-owned and preserve current externally tested paths and messages.

Do not build a second generic graph framework around petgraph. Do not add petgraph merely to prove a property that the domain model already makes unrepresentable.

## Scope

Audit the graph-like sites identified by the #514 follow-up and migrate only their generic mechanics.

### Core

- required component-import cycle validation inside `ComponentGraph`;
- plugin activation cycle validation, including structural runtime-provider dependencies;
- event-subscription structural cycle validation;
- persistence-bootstrap dependency reachability and cycle validation;
- reconciliation structural reverse reachability.

### Plugins

- hooks dependency ordering: audit only; keep the local loop when it is the lexical-ready ordering policy itself;
- planning DAG acyclicity validation;
- execution worker-task graph checks: delete redundant cycle traversal when legal task insertion already makes cycles unrepresentable;
- repository-worker dependencies: audit only; keep direct predecessor/evidence/priority logic when no generic traversal exists.

## Keep Phenix-owned

- `ComponentGraph` domain representation and resolved component/import/provider semantics;
- `ComponentId`, `PluginId`, service and interface identities;
- provider selection and fallback policy;
- dependency-first plugin activation order;
- authority attenuation and gates;
- listener/event semantics and event concurrency levels;
- execution task state transitions, immutable dependency sets, and runnable eligibility;
- lexical-ready hook ordering and hook failure policy;
- planning semantics;
- repository-worker PR evidence, merged-predecessor semantics, eligibility, and work-priority policy;
- reconciliation actions and reload or migration policy;
- exact public error taxonomy and semantic diagnostics.

## Implementation findings

- Component required-import cycle detection is generic and migrates to the Core graph helper. Required primary/fallback edges and concrete cycle diagnostics remain `ComponentGraph` semantics.
- Plugin activation acyclicity is generic. The valid dependency-first activation sequence is lifecycle policy and remains explicit rather than adopting petgraph topological iteration order.
- Event cycle detection is generic. Level construction remains explicit because listeners in one level may execute concurrently, making level boundaries semantic behavior.
- Reconciliation dependency closure is generic structural reachability. Restart/reload/migration decisions remain `GraphReconciler` policy.
- Planning cycle validation is pure acyclicity and uses petgraph directly inside the plugin.
- Execution's old `creates_cycle` traversal is redundant. A new task may reference only already-existing tasks, and task identities/dependency sets are immutable after insertion, so legal insertion cannot create a cycle. The traversal is deleted rather than replaced.
- Hook ordering intentionally remains local. Its `BTreeSet` ready selection defines required lexical-ready order; petgraph could not replace that loop without changing semantics, and adding a second cycle pass would grow the implementation.
- Repository-worker dependency handling intentionally remains local. It checks direct predecessor state and combines it with verified-green evidence and work-priority policy; there is no generic graph traversal to replace.

## Implementation constraints

- No petgraph types cross public API, serde, persistence, plugin ABI, or SDK boundaries.
- Preserve deterministic behavior for equivalent inputs independent of registration or snapshot order.
- Preserve concrete cycle-path diagnostics where the current contract exposes them.
- Do not change provider tie-breaking, graph generation identity, event concurrency levels, task readiness, hook ready-order policy, or worker selection semantics.
- Remove superseded generic DFS/reachability implementations. Do not retain fallback or parallel generic graph mechanisms.
- Prefer `DiGraph` unless a call site proves it needs another representation. Stable node identity is not assumed.
- Keep edge direction explicit at each call site. Dependency meaning belongs to that subsystem, not to a generic helper.
- Keep readiness and priority filters outside petgraph. Graph code answers structural questions only.

## Acceptance criteria

- [ ] `petgraph` is a workspace dependency with only the required `std` feature enabled.
- [ ] Core-only shared construction and traversal glue remains crate-private.
- [ ] Each audited site either delegates a real generic graph mechanism to petgraph, deletes a redundant mechanism, or records a concrete semantic reason to keep its existing domain-policy loop.
- [ ] No replaceable local generic DFS or reachability implementation remains at migrated sites.
- [ ] Existing semantic ordering tests remain green.
- [ ] Registration/input-order permutation coverage proves component cycles, activation behavior, event levels/cycles, planning cycles, hook lexical-ready order, persistence bootstrap, and repository-worker selection are stable.
- [ ] Execution regression coverage proves task insertion rules make dependency cycles unrepresentable and that runnable-task semantics remain unchanged.
- [ ] Cycle diagnostics retain the concrete offending path where currently promised.
- [ ] Graph generation identity is unchanged for semantically equivalent inputs.
- [ ] Measure deleted versus added hand-maintained Rust LOC and call out any site where a graph adapter is larger than the removed mechanics.
- [ ] Delete this migration spec when #519 is complete; canonical subsystem specs and behavior tests remain the lasting sources of truth.

## Non-goals

- Replacing `ComponentGraph` with a petgraph public/domain type.
- Replacing reconciliation policy with an incremental-computation framework such as Salsa.
- Replacing Phenix state machines with graph-library state.
- Changing public graph or domain DTOs.
- Creating a generic graph crate or public graph utility API.
- Introducing async execution.
