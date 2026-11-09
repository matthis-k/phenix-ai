# Storage capability correction — Adversarial critique (Phase 2)
Date 2026-11-09. Separate pass by the **same assistant**; independence criterion not met.

## First pass rejected and revised
A universal `StoreQuery<T>`/native SQL facade would discard SpacetimeDB's generated subscription/query semantics or invent a SQL engine. Reject. A `Kv::read` special case with an `unversioned` bit leaves advanced guarantees conflated. Reject. Revised design uses existing `Capability` and keeps backend-native operations inside existing provider Graph contracts.

## Reject list evaluation (revised design)
1. New abstraction without invariant: avoided; no new subsystem; `CellSpec::request` is a method.
2. Mere conceptual mirror: avoided; independent revisioned read, atomic batch and CAS are distinct operational promises.
3. Compensating wrappers: avoided; correct existing `Kv` semantics.
4. Duplicate primitives: avoided; existing `Session::get`, `SnapshotReader`, `Producer` reused.
5. Complicated common use: avoided; plain `get` becomes `Option<T>`; advanced token use explicitly named.
6. Runtime computation that belongs earlier: avoided; collect/prepare/activate independent feature checks, runtime recheck for safety.
7. Ad hoc exceptions: avoided; capability set handles orthogonal combinations.
8. Migration disproportionate to payoff: **must measure in Phase 4**; general backend support justifies targeted affected uses; return to Phase 1 if broadly invasive.
9. Second dialect: avoided by deleting legacy revisioned `get` meaning and `atomic_cas` request bit.
10. No subtraction: avoided; exact removed field, inference and API semantics documented.
11. Foreseeable required failure: handled for read-only atomic batches and preflight rejection of CAS-only cells.

## Adversarial consumer
Backend advertises `RevisionedRead=true, AtomicReadMany=false, CompareExchange=true`. Single-key revisioned readers may prepare; `CellSpec` readers must fail before graph activation. Reverse case: `AtomicReadMany=true, CompareExchange=false` may serve read-only cells. False advertised capability at activation must abort candidate publication. Two plugins using one resource have independent grants; never union their authorizations.

## Accepted limitations (protocol default)
- Cross-mount atomicity: not promised by current Storage; `BestEffort` or co-locate resources. Accepted for this slice, no distributed transaction demanded.
- SQLite external-process changes: current `StorageBinding` watches only registry writes. Not claimed as supported. A future external feed requires a new Phase 0–2 with concrete adapter and reconnect/replay falsifier.
- Native remote SQL/query syntax: no fake generic SQL. Consumers import a provider's Graph contract; preserves native capabilities and schema matching.
- Notifications: bounded, non-durable, overflow-rescan, no exactly-once guarantee; existing semantics unchanged.
- Calling a method whose stronger capability the plugin did not declare: structured runtime error, not silent fallback; approved declared requirements fail earlier.

These limitations do not weaken existing invariants or the bounded capability-model use case. **Not accepted:** claiming external event coverage, treating CAS as proof of atomic batching, or bypassing grants. Those are defects.

## Decision
Revised note provisionally survives this separate critique. It does not pass independent-review preference, and no code/diff/test evidence is asserted. Phase 3 must keep trivial use trivial, verify all actual call sites and return to Phase 1 if migration proves disproportionate.
