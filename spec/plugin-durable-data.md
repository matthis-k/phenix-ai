# Plugin durable data

status: implemented

## Purpose

Let userspace plugins persist canonical and derived service state without owning raw database handles or requiring domain-specific kernel tables.

## Ownership

The kernel owns persistence mechanisms:

- durable namespace identity;
- schema registration and validation;
- schema versioning and migration ordering;
- transaction boundaries and atomic commit semantics;
- recovery/startup gating;
- namespace permission checks;
- provider-neutral query/mutation APIs;
- storage backend selection;
- provenance for durable mutations.

A plugin owns:

- its namespaced schema definitions;
- domain identifiers stored in those schemas;
- field meaning and validation;
- indexes/query declarations it requires;
- migrations between its schema versions;
- canonical or derived service semantics reconstructed from those records.

The storage backend owns physical representation.

## Registration

A plugin may register one or more `DurableSchema` values:

```text
DurableSchema
  owner: PluginId
  namespace: DurableNamespaceId
  version
  records/tables
  fields
  keys
  relations
  indexes
  constraints
  migrations
```

The API must remain provider-neutral. SQLite SQL is not the canonical contract.

## Domain state

Agent-domain state belongs here when owned by a userspace service.

Examples include:

```text
phenix-sessions        sessions and conversation events
phenix-artifacts       artifact identities/content refs
phenix-context         context resources/revisions
phenix-workers         task/result/verification state
phenix-planning        objectives/plans/decisions
```

The kernel does not mirror these into kernel-private aggregates.

## Transactions

Plugin durable mutations use kernel transactions.

A transaction may span multiple plugin namespaces when one declared user-visible operation requires atomicity.

Example:

```text
phenix-sessions: create child session
phenix-session-tree: add lineage edge
phenix-objectives: inherit assignment
commit atomically
```

The kernel validates namespace ownership, declared transaction participation, schema/version compatibility, permissions, and transaction scope before dispatching to the backend.

A backend commits the complete mutation set or none of it.

## Queries

The generic contract supports the smallest useful record operations:

- exact key lookup;
- bounded ordered scan/list;
- atomic put/delete mutations;
- value assertions for optimistic compare-and-swap coordination.

`phenix-sdk` builds typed collections on that contract. `DurableMap` provides deterministic encoded-key lookup and scans. `DurableLog` uses fixed-width sequence keys plus a tail assertion in one transaction. Collection names and map keys are encoded so collection prefixes cannot overlap through separator characters.

FTS, vector search, graph traversal, semantic ranking, and other specialized query systems remain separate service capabilities.

## Migrations

Plugin schema migrations are versioned declarations owned by the plugin and executed through the kernel/backend migration lifecycle.

Startup must validate the complete configured schema set before exposing dependent services.

A missing plugin never authorizes automatic deletion of its durable namespace.

## Removal and replacement

Disabling or removing a plugin preserves its durable data unless an explicit user/migration operation deletes it.

A replacement provider may consume an existing namespace only when the service contract/schema declares compatibility or an explicit migration performs conversion.

The kernel never assumes two implementations assign the same domain meaning to bytes merely because they implement a similarly named capability.

## Backend portability

A conforming persistence backend stores kernel-private infrastructure schemas plus arbitrary valid plugin schemas without understanding plugin domain semantics.

Schema contracts may require optional backend operations. `Migrations` is the only optional backend feature today; transactions, unique record keys, and bounded ordered scans are baseline behavior.

An incompatible backend is rejected before store activation.

## Permissions

A plugin receives durable-data access only to authorized namespaces and operations.

Durable-data permission does not grant filesystem, repository, network, secret, IPC, or raw backend access.

Kernel-private trust/state namespaces are not writable through the plugin API.

## Invariants

- Plugins define product schemas; kernel defines persistence mechanisms.
- Kernel contains no session/artifact/context/product tables merely to support the normal Harness.
- Plugins never receive raw backend handles.
- Durable namespaces are isolated by owner/contract.
- Multiple userspace services may join one atomic transaction.
- Disabling a plugin never silently deletes its state.
- Backend switching does not reinterpret plugin schema meaning.
- Specialized search/query systems remain separate capabilities.

## Required regressions

- mock plugin registers a schema and round-trips records through the baseline backend;
- same schema fixture works through an alternate mock backend;
- plugin cannot read or mutate another namespace;
- plugin cannot mutate kernel-private schema;
- multi-plugin transaction commits atomically or not at all;
- schema migration is transactional and version-gated;
- disable/re-enable preserves compatible data;
- missing plugin does not drop its namespace;
- unsupported backend features make a backend ineligible before startup;
- plugin cannot obtain raw SQLite/connection access.