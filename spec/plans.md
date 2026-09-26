# Plans

status: implemented

## Ownership

Plans are durable workspace-owned strategy records. The runtime owns plan identity, revisions, lifecycle, step state, objective links, and execution links.

A plan describes intended work. Orchestrations and executions own scheduling and runtime mechanics.

## Shape

A plan has:

- stable identity;
- workspace identity;
- lifecycle state;
- optional predecessor or superseded plan;
- ordered steps;
- objective references.

A plan step has:

- stable identity within the plan;
- description;
- lifecycle state;
- revisability;
- explicit step dependencies;
- objective references.

Plans do not contain model targets, callable bindings, execution authority, retry policy, timeout policy, or scheduling policy.

## Drafts and enactment

A plan is mutable while prospective. Draft edits replace prospective strategy before any execution enacts a step.

The first execution linked to a plan step enacts the plan revision. That revision then freezes. Future strategy changes create a successor plan instead of editing the enacted plan.

Enactment is durable and replay-validated. A restored runtime cannot reinterpret an enacted plan from mutable frontend or backend state.

## Lifecycle

Plan states are:

```text
draft
active
completed
failed
invalidated
abandoned
superseded
```

`failed` means the strategy was attempted and did not succeed. `invalidated` means new evidence disproved an assumption or made the route inapplicable. `abandoned` records an explicit choice to stop using the plan. `superseded` identifies a successor strategy.

Step states are:

```text
proposed
committed
active
completed
failed
invalidated
abandoned
```

A completed plan has no incomplete committed work. Failure and invalidation remain distinct durable outcomes.

## Execution links

An execution may enact one plan step. The link records exact plan and step identities.

Linking the first execution to a plan step atomically freezes the plan revision. Later executions may link only to steps in that same frozen revision unless a successor plan has been created.

Execution links do not copy model, callable, authority, retry, timeout, or orchestration semantics into the plan.

## Dependencies

Step dependencies form a DAG. A step may become active only after its required dependencies complete.

The runtime rejects unknown dependencies, self-dependencies, and cycles. Dependency order is semantic plan data, not a scheduler. An orchestration may choose how to execute ready work.

## Backtracking

Backtracking is semantic history:

```text
old plan -> failed or invalidated
cause or later decision reference
successor plan
```

Changing strategy never rewrites an enacted plan.

Plan backtracking does not restore workspace files. Workspace restoration is a separate explicit operation against an existing recovery checkpoint and remains subject to normal execution authority.

## Objectives

Plans reference objectives; they do not own objective meaning. Plan and step objective references must resolve to objectives in the same workspace.

A successor plan may preserve the same objective references while changing strategy. Changing objective meaning uses objective supersession instead.

## Concurrency

Draft plan updates use revision-based optimistic concurrency. An update supplies the expected draft revision. If another frontend or agent already changed that draft, the runtime rejects the stale update with a typed conflict instead of overwriting it.

Enacted plan revisions are immutable, so they do not participate in draft conflict resolution.

## Persistence

Plans, steps, dependencies, draft revisions, lifecycle transitions, successor links, and execution-step links live in the canonical workspace SQLite database.

The journal records ordered semantic transitions. SQLite stores the same facts relationally and reconstructs typed events without JSON event replay.

Deletion preserves referential integrity. A plan or step referenced by an execution cannot silently disappear.

## Context contract

This slice records exact active plan and step identities needed by later context projection.

The later context slices include only the active relevant plan state as mandatory model context. Full plan history remains durable and addressable rather than permanently injected.

## Scope

This slice owns plan identity, draft revision, first-enactment freezing, lifecycle transitions, step dependencies, objective links, execution-step links, optimistic draft conflicts, successor plans, relational persistence, replay, and focused regressions.

General durable references, decision records, context projection, and history retrieval belong to later slices. Plan editing remains a runtime semantic API in this slice; model-visible discovery and loading are introduced by the context-catalog slice.
