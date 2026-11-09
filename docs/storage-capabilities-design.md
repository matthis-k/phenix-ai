# Storage capability correction — Design Note (revised)
Date: 2026-11-09. Baseline: `impl/resolved-contract-imports-20261108`, commit `2b6b545d`. Scope: Storage, Storage Plugin, scoped typed cells; no new query or event runtime.

## Phase 0: constraints and what if nothing changes?
In `scoped.rs`, `Kv::get` calls `Session::get_revisioned`, and `ResourceRequest::requires_atomic_cas` makes all scoped reads require conditional writes. In `phenix-storage-plugin`, `BackendFeatures` exposes `atomic_cas` but omits `AtomicReadMany`, required by `SnapshotReader` and derived cells. The built-in SQLite and memory backends work, but read-only/remote snapshot backends are rejected unnecessarily, and CAS-only backends can pass planning only to fail when bound to cells. **No change** does not satisfy a general backend capability contract.
Reuse `Capability`, `TypedKv`, `ResourceRequest`, `Session`, `Kv`, `CellSpec`, `SnapshotReader`. Preserve authorization-before-lookup, immutable grants, schema identity, same-mount atomicity, existing SQL authorizer and Graph/Kernel independence. Naming, errors, host-policy approval and opt-in backend probes follow existing conventions.

## Phase 1: target consumer examples (not yet implemented)
Trivial: `let current: Option<Settings> = storage.project()?.config::<Settings>("phenix.agent/settings")?.get("default")?;`
Advanced: `let kv = storage.project()?.config::<Settings>("phenix.agent/settings")?; let current = kv.get_revisioned("default")?; kv.compare_exchange("default", current.as_ref().map(|x| &x.revision), &replacement)?;`
Falsifying case: `registrar.add(ResourceOffer::new("a", A.request([Operation::Read, Operation::Subscribe])))?; let a = storage.bind(A.read())?; graph.sources("amounts", (&a,))?;` where `A: CellSpec<Amount>` is backed by a read-only, atomic-batch capable provider with **no** CAS. It must succeed. A CAS-capable provider without batch atomicity must fail at prepare.

## Concepts / mapping / ownership / timing
- Logical resource: existing `ResourceId`, host-owned; independent of backend and type.
- Grants: existing `Session`/`Operation`; host-approved, per-plugin, validated on use.
- Plain read: existing `TypedKv::get` exposed as scoped `Kv::get -> Option<T>`.
- Revisioned read: existing `TypedKv::get_revisioned`, exposed as `Kv::get_revisioned -> Option<Revisioned<T>>`.
- Atomic grouped read: existing `TypedKv::supports_atomic_read_many`; explicit `Capability::AtomicReadMany` in request and backend offer, with same-mount validation at query time.
- CAS: existing `TypedKv::supports_compare_exchange`; explicit `Capability::CompareExchange` with `Write`.
- Requirements: reuse `Capability` as `ResourceRequest.required: BTreeSet<Capability>`; replace `atomic_cas: bool`, do not add another guarantee type.
- `CellSpec::request(operations)`: convenience method on existing descriptor, producing normal `ResourceRequest` with `AtomicReadMany` requirement.
- Backend: `TypedKv::supports_revisioned_reads` (default false); independent probes for revisioned, batch and CAS. `BackendFeatures` is the pre-open declaration; `Capabilities` is runtime report.
- At authoring time declare exact capabilities; collect rejects invalid kind/operation combinations; prepare compares advertised support with needs; activation probes actual backend support and atomically publishes views only after success; runtime repeats authorization and integrity checks.
- Native SpacetimeDB generated queries/subscriptions stay in provider-owned Graph contracts/`Producer` (already available); do not manufacture an SQL translator or pretend `SqlBackend::connection` is anything but SQLite-native. Storage notices remain registry-local/non-durable in this slice.

## Three alternatives
A. **No change**: zero migration, fails read-only atomic provider and preflight distinction.
B. Add `Kv::read` and `unversioned` flag: small diff, preserves implicit CAS coupling and duplicate modes.
C. **Chosen**: correct existing capability representation, `get` = plain, `get_revisioned` = advanced, `CellSpec::request` = exact atomic requirement. Moderate API migration, no new runtime concepts.

## Subtraction, budget and migration
Delete `ResourceRequest.atomic_cas`, `requires_atomic_cas`, `with_atomic_cas`, implied Read=>CAS, and revisioned-returning `Kv::get` compatibility meaning. No alias/shim. Only add a capability enum variant, a requirement set in existing type, a backend predicate, one renamed method, and one cell declaration convenience. Grep all affected Rust/docs/call sites; migrate fully. No second producer or native query API.

## Acceptance
Full plain-only, revision-only, batch-only, CAS-only, all-features, dishonest-offer tests; per-plugin grant isolation, unknown schema denial, exact batch snapshot behavior and existing derived example. `cargo fmt --all --check`, `cargo test --workspace --locked`, no-default tests, Clippy strict, focused storage/plugin/derived/kernel tests, Nix flake check, exact-head CI. Record call-site counts, obsolete grep counts and baseline-vs-new behavior. Stop and return to Phase 1 on a new conceptual problem. All historical examples are in actual linked source files, not invented.
