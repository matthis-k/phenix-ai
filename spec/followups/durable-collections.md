---
status: planned
parent: pull/519
---

# Durable collection primitives

## Goal

Replace plugin-local serialized indexes and whole-value sequence rewrites with one backend-neutral ordered record substrate plus typed author-facing collection helpers.

Core owns durable byte-record mechanics, namespace authority, transactions, and backend behavior. The SDK owns reusable typed collection layout and codecs. Plugins keep domain meaning, lifecycle, ordering policy, and schema meaning.

## Current mismatch

`PersistenceBackend` currently exposes point `read` plus `transact_many`, while first-party plugins emulate indexed collections with records such as `tools/@all`, `skills/@all`, `context/@all`, and `sessions/@all`.

`BackendFeature` also advertises `Transactions`, `UniqueKeys`, `ForeignKeys`, `OrderedAppend`, and `IndexedRange` even though several are already baseline trait behavior or have no callable operation.

The next plugin migration PR must be able to delete those local indexes without introducing a different plugin-local storage abstraction.

## Fixed boundaries

- SQLite is an implementation, not the semantic model.
- No SQLite type, SQL fragment, connection handle, or query language crosses Core or SDK persistence APIs.
- Namespace ownership and persistence authority remain enforced by `PluginHost` before backend access and by the backend itself.
- `transact_many` remains the one transaction model. `DurableLog` must use its existing assertion/CAS operation rather than adding an append transaction API.
- Cross-namespace prepared transaction atomicity is unchanged.
- Public plugin/service wire formats do not change because durable storage changes.
- Existing persisted layouts are not silently reinterpreted. Layout migration belongs to #521 unless #520 itself changes a Core-owned layout.
- Scan order is storage-key order only. It must never become plugin-domain semantic ordering by accident.
- Collection helpers do not own retry policy for non-idempotent domain operations.

## Core record API

Add the smallest ordered read primitive needed by reusable collections.

The semantic shape is:

```rust
pub struct DurableRecord {
    pub key: String,
    pub value: Vec<u8>,
}

pub enum ScanDirection {
    Forward,
    Reverse,
}

pub struct DurableKeyRange {
    // explicit lower/upper bounds; exact representation may use std::ops::Bound
}

pub trait PersistenceBackend {
    fn read(...);
    fn scan(
        &self,
        caller: &PluginId,
        namespace: &ResourceNamespace,
        range: &DurableKeyRange,
        direction: ScanDirection,
        limit: Option<usize>,
    ) -> Result<Vec<DurableRecord>, PersistenceError>;
    fn transact_many(...);
}
```

`scan` is baseline `PersistenceBackend` behavior, not an optional descriptive feature. All in-tree backends and test doubles must implement it.

### Scan semantics

- Ordering is deterministic ascending UTF-8/BINARY record-key order for `Forward` and the exact reverse for `Reverse`.
- Bounds have explicit inclusive/exclusive/unbounded semantics.
- `limit = Some(0)` returns no records. Other limits are applied after bounds and direction.
- Every returned key is inside the requested namespace and range.
- The same namespace ownership checks used by point reads apply to scans.
- Prefix scans are a tested convenience built from exact key bounds. They must not depend on backend-specific `LIKE` behavior or whole-namespace post-filtering.
- SQLite uses the existing `(namespace, record_key)` primary-key index and bounded `ORDER BY record_key` queries. It must not load the namespace and sort/filter in Rust.

Expose the operation through the same host path as point reads:

`PersistenceBackend::scan` -> `PluginHost::scan_durable` -> `KernelAccess::scan_durable`.

The host method requires `kernel.persistence.read` and caller ownership of the namespace exactly like `read_durable`.

## Capability model cleanup

After this PR, `BackendFeature` only represents behavior that is genuinely optional at backend selection time.

Current decisions:

- `Transactions`: remove. `transact_many` atomicity is baseline trait behavior.
- `UniqueKeys`: remove. One value per `(namespace, record_key)` is baseline record semantics.
- `ForeignKeys`: remove/defer. No current public schema operation can request or observe backend foreign-key semantics.
- `OrderedAppend`: remove. Ordered append is an SDK helper built from scan plus transaction assertions.
- `IndexedRange`: remove. Ordered range scan becomes baseline trait behavior.
- `Migrations`: keep. `migrate_schema` is an optional operation with an explicit unsupported path.

Do not keep a capability enum variant merely because SQLite happens to provide the property internally.

Update schema/resource tests and any explicit feature declarations to match the reduced capability model.

## Persistence error identity

Today `PluginHost` converts backend persistence errors into `KernelError::Persistence { message }`. That loses the structured `AssertionFailed { namespace, key }` identity required for safe CAS-based collection helpers.

#520 must preserve assertion conflict identity across the host boundary. The exact variant name may differ, but plugin/SDK code must be able to distinguish an assertion conflict without parsing text.

A narrow Core error such as this is sufficient:

```rust
KernelError::PersistenceConflict {
    plugin: PluginId,
    namespace: ResourceNamespace,
    key: String,
}
```

Other backend failures may remain opaque persistence errors if no caller decision depends on their subtype.

## SDK collection layer

Put author-facing reusable collection helpers in `phenix-sdk`, above Core's byte-record substrate. Do not move plugin-domain record types into Core.

### DurableMap

`DurableMap<K, V>` provides:

- `get`
- `put`
- `delete`
- bounded/prefix `scan`
- deterministic enumeration in encoded-key order

Key encoding is explicit through one deterministic key codec contract. Do not use an unconstrained blanket `Display` or arbitrary serde representation as the storage key contract.

The codec must:

- round-trip valid keys
- escape/reserve collection metadata separators
- produce stable strings across runs
- document that storage order is encoded-key order, not automatically domain order

Values use the existing serde/Phenix serialization stack. Do not introduce a second general serialization system.

### DurableLog

`DurableLog<T>` uses one collection prefix containing:

- a reserved tail record
- fixed-width sequence entry keys whose lexical order matches numeric sequence order

Append is one optimistic operation:

1. read the current tail
2. derive the next sequence, rejecting overflow
3. transact `AssertValue(tail, expected)` + `Put(entry)` + `Put(tail)` atomically
4. return a typed conflict when the assertion loses a race

The helper must not hide an unbounded retry loop. A caller may retry explicitly when its domain operation is safe to retry.

Reads use reverse/forward range scans rather than a serialized vector.

### DurableSet and secondary indexes

Do not add `DurableSet` unless it removes material code beyond a trivial map wrapper.

Do not add a generic secondary-index framework in this PR. #521 may add a small helper only if multiple migrations demonstrate the same index maintenance operation and it can remain domain-neutral.

## Implementation batches

### 1. Core scan types and backend contract

- Add `DurableRecord`, `DurableKeyRange`, and `ScanDirection`.
- Add mandatory `PersistenceBackend::scan`.
- Extend in-tree test backends/mocks at compile time, with no fallback implementation that silently full-scans point reads.
- Add backend-independent range semantics tests.

### 2. Local SQLite scan

- Implement bounded SQL scans against `kernel_plugin_records`.
- Use the `(namespace, record_key)` primary-key index.
- Cover inclusive/exclusive bounds, unbounded sides, forward/reverse, zero/nonzero limits, empty result, and namespace isolation.
- Prove malformed/foreign namespace access is rejected before rows are returned.

### 3. Host/API forwarding

- Add `PluginHost::scan_durable` and `KernelAccess::scan_durable`.
- Reuse `kernel.persistence.read` authority and resource-owner checks.
- Preserve structured assertion conflicts instead of stringifying them.
- Add authority/ownership regressions for scan and conflict propagation.

### 4. Capability cleanup

- Remove baseline/descriptive `BackendFeature` variants listed above.
- Keep only actual optional backend operations, currently migrations.
- Update resource declarations, provider descriptors, bootstrap tests, and feature-negotiation tests.

### 5. SDK DurableMap

- Add the deterministic key codec boundary.
- Implement typed get/put/delete/scan over `KernelAccess`.
- Test key escaping, round-trip, stable layout, malformed key/value decoding, prefix isolation, and deterministic enumeration.

### 6. SDK DurableLog

- Define reserved tail and fixed-width entry layout.
- Implement read/append using scan plus existing `AssertValue` transaction semantics.
- Add conflict, empty log, ordering, reverse tail read, overflow, malformed tail, malformed entry, and restart-stable layout tests.

### 7. Conformance and cleanup

- Add one reusable persistence backend conformance suite where practical.
- Run it against `LocalPersistence` and in-tree alternate/test backends.
- Search for capability flags with no operation and collection helpers that bypass namespace authority.
- Keep first-party plugin migrations out of this PR except minimal compile/test adjustments caused by the capability API cleanup.

## Acceptance criteria

- [ ] `PersistenceBackend` has a deterministic ordered range scan with no SQLite-specific API leakage.
- [ ] SQLite executes bounded indexed queries instead of full in-memory namespace scans.
- [ ] Scan authority and namespace ownership exactly match point-read policy.
- [ ] Host/SDK callers can distinguish transaction assertion conflicts without parsing strings.
- [ ] `BackendFeature` contains only real optional backend operations.
- [ ] `DurableMap` exists with explicit stable key encoding and typed encode/decode tests.
- [ ] `DurableLog` exists with conflict-safe monotonic sequencing using the existing transaction/CAS model.
- [ ] No hidden unbounded retry policy exists in collection helpers.
- [ ] Existing point reads, transactions, prepared multi-plugin atomicity, schema ownership, and migrations remain behaviorally compatible.
- [ ] Backend conformance covers bounds, direction, limits, namespace isolation, malformed values, and CAS conflict behavior.
- [ ] #521 can remove `@all` vectors and whole-vector logs without adding replacement plugin-local generic storage helpers.

## Non-goals

- Migrating first-party plugin persisted layouts. That is #521.
- Replacing `PersistenceBackend` with a concrete database crate.
- Making SQLite the semantic storage model.
- Adding arbitrary predicates, joins, SQL exposure, or a query language.
- Adding plugin-domain secondary-index policy to Core.
- Changing public plugin/service wire contracts.
