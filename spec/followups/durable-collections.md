---
status: planned
parent: pull/519
---

# Durable collection primitives

## Goal

Make the persistence contract expose the generic storage operations that plugins currently reimplement with serialized `@all` vectors and whole-object rewrites.

The target is a backend-neutral ordered key/value substrate with typed durable collection helpers. Backends own storage mechanics. Plugins own record meaning and domain transitions.

## Theoretical problem

The repeated problem is transactional indexed storage:

- point lookup by stable key
- ordered/range/prefix scans
- uniqueness/CAS
- append-only ordered sequences
- secondary indexes over durable records

This is the same family of problem as an ordered relation/indexed key-value store. It is not specific to sessions, tools, skills, context, jobs, planning, or memory.

## Current mismatch

`BackendFeature` advertises capabilities including `IndexedRange` and `OrderedAppend`, but `PersistenceBackend` only exposes point `read` plus transactional `Put`/`Delete`/`AssertValue`. Plugins therefore cannot use the advertised capabilities and maintain JSON index vectors such as `tools/@all`, `skills/@all`, `context/@all`, and `sessions/@all` themselves.

A feature flag with no callable operation is not a usable backend contract.

## Decision

Add the smallest backend-neutral read/query primitive needed to represent indexed collections and ordered logs. Build typed SDK helpers above it rather than leaking SQLite or backend-specific queries into plugins.

### Core persistence API

Introduce an ordered range/prefix scan with deterministic byte/string key ordering, explicit limits, and direction. The exact public names may change during implementation, but the semantic shape must be equivalent to:

```rust
struct DurableRecord {
    key: String,
    value: Vec<u8>,
}

enum ScanDirection { Forward, Reverse }

trait PersistenceBackend {
    fn read(...);
    fn scan(
        &self,
        caller: &PluginId,
        namespace: &ResourceNamespace,
        range: DurableKeyRange,
        direction: ScanDirection,
        limit: Option<usize>,
    ) -> Result<Vec<DurableRecord>, PersistenceError>;
    fn transact_many(...);
}
```

Use an explicit range type rather than backend query strings. Prefix scans may lower to a key range internally.

### Typed collection layer

Add lightweight typed helpers in the SDK/core authoring layer:

- `DurableMap<K, V>`: stable encoded key -> value, get/put/delete/scan
- `DurableSet<K>` only if it removes real plugin code rather than wrapping a map trivially
- `DurableLog<T>`: monotonically sequenced records backed by range scans and one transactional tail/CAS record
- secondary-index helpers only where they can be expressed generically without embedding plugin query semantics

`DurableLog` should allocate the next sequence with the existing transaction/CAS mechanism so ordered append does not require a second transaction model. Sequence keys must sort lexicographically in numeric order, e.g. fixed-width big-endian/zero-padded encoding owned by the helper.

If `BackendFeature::OrderedAppend` no longer corresponds to an independent operation after this design, remove or redefine it. Do not retain capability flags that cannot be exercised through the backend contract. Apply the same rule to `IndexedRange`, `UniqueKeys`, and `ForeignKeys`: each advertised feature must either affect an exposed operation/schema guarantee or be removed until it does.

## SQLite backend

Implement scan natively with the existing `(namespace, record_key)` primary key and ordered SQL range queries. Do not add plugin-specific tables or SQL to core.

Keep Phenix's per-namespace schema/migration model. `rusqlite_migration` remains inappropriate unless the storage model changes from per-plugin schema versions to one global database migration chain.

## Encoding and compatibility

- Collection key encoding must be deterministic and documented.
- Public plugin/service wire formats must not change merely because durable storage changes.
- Existing persisted layouts either receive explicit migrations or remain readable until migrated in the next PR; do not silently reinterpret bytes.
- Typed helpers use existing Phenix value/serde contracts rather than inventing a second serialization system.

## Authority and transactions

- Namespace ownership checks remain in core.
- Scans must enforce the same read authority/ownership rules as point reads.
- Cross-namespace `transact_many` semantics remain atomic.
- A typed helper must not allow a plugin to address another plugin's namespace.
- CAS conflict behavior remains explicit; do not hide retry loops in a helper unless the operation is idempotent and bounded by contract.

## Acceptance criteria

- [ ] Persistence has an ordered range/prefix read primitive with deterministic semantics and backend conformance tests.
- [ ] Local SQLite implements it using indexed queries, not full in-memory scans.
- [ ] At least `DurableMap` and `DurableLog` exist with typed encode/decode tests.
- [ ] Append sequencing is safe under concurrent/conflicting writers using the existing transaction assertion mechanism.
- [ ] Every `BackendFeature` corresponds to an observable backend guarantee/operation or is removed/deferred explicitly.
- [ ] No SQLite type/query leaks into plugin APIs.
- [ ] Existing point read and transaction behavior stays compatible.
- [ ] Persistence backend conformance covers ordering, bounds, reverse scans, limits, namespace isolation, malformed values, and CAS conflicts.
- [ ] Follow-up plugin migration can remove `@all` vectors without adding plugin-local storage helpers.

## Non-goals

- Replacing the persistence backend interface with a concrete database crate.
- Making SQLite the semantic model.
- Adding arbitrary query languages.
- Encoding plugin-domain secondary-index policy in core.
