---
status: planned
parent: pull/520
---

# Plugin durable storage migrations

## Goal

Migrate first-party plugins from serialized collection/index blobs to the durable collection primitives introduced by the previous PR.

This PR should delete plugin-local storage machinery. It must not introduce new per-plugin generic indexing helpers.

## Theoretical problem

These plugins repeatedly implement the same data structures on top of point KV storage:

- entity table + `@all` index
- ordered append-only sequence encoded as one JSON `Vec`
- secondary index + record table
- large materialized state projection rewritten on every mutation

Once core exposes ordered scans and typed durable collections, these become ordinary maps/logs/indexes.

## Migration inventory

At minimum audit and migrate the following current patterns:

### Obvious `@all` indexes

- `phenix-plugin-basic-tools`: `tools/@all`
- `phenix-plugin-basic-skills`: `skills/@all`
- `phenix-plugin-basic-context`: `context/@all`
- `phenix-plugin-sessions`: `sessions/@all`
- `phenix-plugin-context`: `resources/@all`
- `phenix-plugin-jobs`: plugin-local index vector
- `phenix-plugin-planning`: plugin-local history/index vectors
- `phenix-plugin-memory`: plugin-local indexes in `persistence.rs`

### Whole-sequence rewrites

- sessions inputs
- sessions history
- any planning/job/memory history stored as a full serialized vector

Use `DurableLog<T>` where ordering/append semantics are the actual domain model.

### Whole-state projections

- execution currently persists executions, callables, and worker tasks together as one `ExecutionProjection` blob

Split this into independently keyed durable records/collections where mutations do not require atomic replacement of the whole projection. Preserve the few invariants that genuinely require a multi-record transaction by using one `transact_many`/namespace transaction.

Audit options/execution-configuration and other small snapshot-like state separately. A bounded configuration snapshot may remain one value when whole-object atomic replacement is the intended semantic model. Do not split blobs merely for style.

### Secondary indexes

Audit artifacts, memory, planning, jobs, session-tree, and repository-worker state for manually synchronized index records. Use the generic collection/index primitives when the index is structural. Keep domain-specific lookup/ranking semantics in the plugin.

## Migration policy

This repository is prerelease and does not keep parallel legacy APIs. Durable state still needs deterministic migration semantics, however.

For every changed persisted namespace:

1. bump the durable schema version,
2. provide one explicit migration from the immediately previous layout when the existing test/product workflow expects restart persistence,
3. migrate once into the new canonical representation,
4. remove old read/write paths after migration,
5. do not retain runtime fallback reads of the old layout.

If a plugin's current durable state is explicitly disposable prerelease fixture state and the product contract allows reset, document that and perform a deliberate schema reset rather than implicit fallback compatibility.

## Correctness constraints

- Public service/API responses and ordering stay unchanged unless a separate semantic change is intentional.
- List operations are deterministic and no longer require reading an `@all` blob then N point records when one ordered scan suffices.
- Append operations write O(1) records rather than rewriting O(n) history.
- Independent entity mutations do not rewrite unrelated entities.
- Multi-record invariants remain atomic.
- Corrupt/missing indexed records cannot silently produce partial state; typed collection decoding returns explicit errors.
- Namespace authority is unchanged.

## Concurrency

Use the prior PR's transaction/CAS semantics. Do not implement unbounded retry loops in every plugin.

Where a log or index update conflicts, expose a deterministic conflict or use one shared bounded helper if the operation is provably idempotent. Tests must cover two writers racing for the same append/index update.

## Acceptance criteria

- [ ] No first-party plugin retains an `@all` JSON index where an ordered collection scan represents the same semantics.
- [ ] Session input/history append is O(1) durable writes and range-readable.
- [ ] Execution no longer rewrites the complete execution/callable/task projection for one record mutation.
- [ ] Jobs/planning/memory/artifacts are audited for structural secondary indexes and migrated where appropriate.
- [ ] Intentionally atomic small snapshot values are documented rather than mechanically split.
- [ ] Every persisted layout change has an explicit schema migration/reset decision.
- [ ] No legacy runtime fallback/read path remains.
- [ ] Restart/conformance tests prove semantic equivalence before/after migration.
- [ ] Concurrency tests cover append/index conflicts.
- [ ] Measure storage write amplification before/after for representative session history and execution-task updates.
- [ ] Hand-maintained Rust LOC for generic storage/index mechanics decreases materially.

## Non-goals

- Changing memory ranking policy.
- Changing execution lifecycle semantics.
- Replacing plugin-domain records with database-specific schemas.
- Exposing SQL or backend-native indexes to plugins.
