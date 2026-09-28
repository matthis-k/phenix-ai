# Session service plugins

status: implemented

## Purpose

Define sessions as userspace service semantics, not a kernel primitive.

The normal Phenix Harness gets durable conversation/session behavior from the Phenix Plugin Suite. Alternate session services may replace it.

## Ownership

The kernel provides only generic mechanisms used by the session service:

- plugin/service lifecycle;
- generic authority enforcement;
- runtime tasks and cancellation;
- events;
- durable namespaces and transactions;
- generic provider resolution;
- exact kernel-operation provenance.

A session service owns:

- `SessionId` and its allocation rules;
- allocated session creation, so product/application callers never synthesize canonical session IDs;
- session lifecycle;
- accepted user/root input ordering;
- conversation/execution association;
- workspace/configuration associations that are part of session semantics;
- durable session records and recovery;
- open/list/restore/continue behavior;
- frontend-facing session projections.

The kernel stores this state only as opaque plugin-owned durable data.

## Phenix session service

The Phenix Plugin Suite should provide a focused session service implementing the durable-session guarantees required by Phenix.

A separate session-tree service may provide:

- parent/child lineage;
- forks and navigation;
- names/titles;
- summaries;
- tags or collections;
- richer listing/filtering.

Whether flat session storage and tree semantics are one plugin or several is a userspace design choice. It is not a kernel distinction.

## Replacement

An alternate session implementation may define a compatible `session.*` contract and be selected through normal provider binding/priority when the Harness configuration permits replacement.

The kernel must not require Phenix-specific `SessionId` representation or session tables.

Historical state remains interpretable only by a compatible service implementation/schema version. The kernel preserves the durable namespace without pretending to understand it.

## Atomic cross-service operations

A user-visible operation may span several userspace services.

Example:

```text
phenix-sessions: create child session
phenix-session-tree: record lineage edge
phenix-objectives: bind inherited objective
```

If product semantics require atomicity, the services join one kernel-mediated transaction under declared authority. Atomicity does not move any of these concepts into the kernel.

## Invariants

- Kernel defines no Phenix session aggregate or flat-session fallback.
- Session identity and semantics are userspace-owned.
- Normal Phenix durable-session guarantees are implemented by the Phenix Plugin Suite.
- Alternate session implementations can replace the Phenix implementation through the same service contract.
- Session durable state remains isolated in plugin namespaces.
- Cross-service atomicity uses generic kernel transactions.
- Disabling the session service removes session behavior rather than exposing a miniature kernel session implementation.

## Required regressions

- kernel-only profile contains no session service;
- Phenix session plugin creates, persists, restores, and continues a session;
- alternate mock session provider can replace the first-party provider without kernel changes;
- session data round-trips through plugin durable schemas;
- session-tree plugin can be disabled independently when product policy allows it;
- combined child-session/lineage operation commits atomically when configured as one semantic operation;
- kernel persistence/backend code contains no session-specific schema or logic.