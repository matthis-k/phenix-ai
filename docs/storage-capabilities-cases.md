# Storage design protocol — seven real worked cases
All seven cases below are grounded in the source diff between
`2b6b545dd000b40c2c535cefe0c50b87b9ea5283` and the current PR.
They document actual old behavior and reviewed corrections, not hypothetical
historical evidence. Paths are relative to the repository root.

## 1. Plain scoped reads accidentally needed CAS
**Before:** `packages/phenix-storage/src/scoped.rs::Kv::get`
returned `Option<Revisioned<T>>` using `Session::get_revisioned`.
**Critique:** The simplest storage consumer was forced to depend on version tokens
and an unrelated write capability; read-only KV backends could not serve it.
**After:** `Kv::get` delegates to `Session::get` and returns `Option<T>`;
`Kv::get_revisioned` retains the explicitly advanced operation.
**Evidence:** scoped storage unit tests, plain-only provider test.

## 2. One `atomic_cas` flag stood for incompatible guarantees
**Before:** `scoped.rs::ResourceRequest::requires_atomic_cas` inferred
compare-exchange from every `Read`, even though CAS performs a write.
**Critique:** A read authority and a backend read capability must not imply
conditional-write support.
**After:** `ResourceRequest::require(Capability::CompareExchange)` for a
writer; `require(RevisionedRead)` for a single-key revisioned reader; and
`require(AtomicReadMany)` for coherent groups.
**Evidence:** `phenix-storage-plugin` request preflight and tests.

## 3. Backend preflight forgot atomic batch reads
**Before:** `phenix-storage-plugin::BackendFeatures` offered only
`typed_kv/sql/atomic_cas/invalidation`. A CAS-only backend could pass
planning even though `SnapshotReader` requires atomic read batches.
**Critique:** Fails at runtime after expensive graph composition.
**After:** Independent `revisioned_read`, `atomic_read_many` and
`compare_exchange` declarations checked at prepare; actual probes at
activation. No new backend engine.
**Evidence:** `snapshot_requirement_rejects_cas_only_and_false_batch_offers`.

## 4. A cell could bind before learning its backend was incapable
**Before:** `snapshot.rs::Session::read_source` preflighted
`ReadConsistency::BestEffort`, even when the only caller was
`scoped.rs::CellRead::bind`, whose `ReadCell` reads atomically.
**Critique:** The invariant was checked too late.
**After:** `read_source` uses existing `SnapshotReader` validation with
`AtomicWithinMount`, rejecting before exposing a bound cell.
**Evidence:** `plain_scoped_reads_need_neither_revision_nor_cas` explicitly
checks that the same backend serves plain KV but rejects cell binding.

## 5. Derived examples did not declare their actual guarantee
**Before:** `phenix-storage-graph/examples/derived.rs::offers` used
`ResourceRequest::new(spec.resource_spec(), operations)` for both
readers and writers; the reader's atomic requirement remained implicit.
**Critique:** Host planning cannot validate a requirement omitted by the
declaration.
**After:** Readers call `spec.request(operations)` which requests
`AtomicReadMany`; writers still use the ordinary `ResourceRequest::new`.
**Evidence:** Derived example and tests, storage-plugin read-only batch test.

## 6. Missing revisions misleadingly reported unsupported CAS
**Before:** The default `TypedKv::get_revisioned` error was
`UnsupportedCapability { capability: CompareExchange }`.
**Critique:** A revisioned-read operation must not diagnose a missing
conditional-write capability.
**After:** Reports `RevisionedRead` explicitly, and corresponding revision
regressions assert the corrected error. CAS errors continue to name
`CompareExchange`.
**Evidence:** `phenix-storage/tests/revisions.rs`.

## 7. The observer's declared capability did not match the code it ran
**Before:** `phenix-kernel/examples/scoped_storage.rs` observer used
`kv.get` for revisioned updates while declaring only `Read/Subscribe`.
The writer used `with_atomic_cas` instead of a distinct write requirement.
**Critique:** Actual method requirements could not be independently checked at
the host boundary.
**After:** Observer declares `RevisionedRead` and calls
`kv.get_revisioned`; writer declares `CompareExchange`, maintaining
independent permissions and update tokens.
**Evidence:** `phenix-kernel/tests/scoped_storage.rs`, example runtime.

These examples do **not** assert that generic remote SQL queries, durable
cross-process watches, or cross-mount transactions have been implemented.
Those require new independently reviewed requirements and designs.
