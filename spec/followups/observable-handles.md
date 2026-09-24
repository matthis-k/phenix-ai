---
status: implemented
parent: pull/607
---

# Observable handles and residual generic mechanics

## Goal

Close the remaining crate-replacement questions from the ownership audit without adding generic dependencies that make Phenix semantics harder to express.

This PR focuses on observable handle allocation and records explicit non-adoption decisions for the nearby structures that were evaluated as potential generic-crate targets.

## Theoretical problem

`ObservationId` + `ObservationGeneration` form a generational-handle identity. Generic arena crates such as `slotmap` solve stale-handle rejection when slot reuse is an implementation detail.

Phenix's observable IDs/generations are different in one important way: they are public exported/serialized contract types, not merely private arena keys.

## Decision: public handles stay Phenix-owned

Do not expose `slotmap` keys or another arena representation through the public observable API.

Adopt `slotmap` internally only if it can replace allocation/stale-handle bookkeeping without requiring a second public-id -> slot-key mapping. If an extra mapping is required, keep the simpler Phenix representation.

The expected result today is to retain the public `ObservationId` / `ObservationGeneration` pair and strengthen allocation semantics rather than force an arena abstraction.

The current store needs no private arena identity: `subscriptions` is already keyed by the public
`ObservationId`, while `SubscriptionNode` stores those same IDs directly. A `slotmap`
implementation would therefore retain the public-ID lookup and add slot keys plus a public-ID ->
slot-key mapping. It would increase identity bookkeeping rather than replace it, so no dependency is
added.

## Counter exhaustion

Current observable allocation advances `next_observation` and `next_generation` with saturating arithmetic. Once either reaches `u64::MAX`, saturation would stop advancing and can violate uniqueness.

Replace saturation with checked allocation:

- allocate the current non-reserved value,
- `checked_add(1)`,
- return an explicit observable exhaustion error if no next value exists,
- never wrap or reuse an exhausted public ID/generation.

Add deterministic tests that initialize/drive a test store near the maximum and prove exhaustion is reported before duplicate identity can be created.

## Subscription path structure

Keep the specialized observable subscription tree/trie.

The theoretical structure is a prefix trie, but generic Rust trie crates are usually byte/string-key oriented. Phenix paths contain typed structural segments and exact-vs-recursive observation semantics. A generic trie would need an adapter retaining most of the current logic.

Acceptance requirement: document this boundary beside the structure and ensure the implementation contains only path-index mechanics plus Phenix observation semantics, not unrelated policy.

## Transaction/patch state machine

Keep the observable transaction algebra and rollback semantics Phenix-owned.

Generic JSON-patch/persistent-data-structure crates do not express the typed `PhenixValue` schema, observation delivery, version/commit metadata, or rollback/delivery ordering contracts. Do not translate through JSON merely to use an upstream patch crate.

## Reconciliation

Do not replace reconciliation with Salsa or another incremental-computation framework.

Use the graph substrate from #519 for structural reachability/topology only. Phenix continues to own candidate-vs-active diff meaning, reload/drain/migration policy, invalidation actions, generation transitions, and inspectable transition plans.

Current usage confirms that split: `phenix-core/src/reconciliation.rs` represents Phenix-specific `GraphDiff`, `ReconciliationAction`, `ReconciliationPreview`, and generation transitions, while resolved component topology supplies the structural graph operations. No incremental-computation framework owns or obscures reconciliation policy, so adding Salsa would duplicate the existing semantic transition model rather than replace generic graph mechanics.

## Session-tree and priority structures

- Keep session-tree durable semantic IDs rather than introducing arena-local node identities. Current `phenix-plugin-session-tree` state is keyed and serialized by durable `SessionId`/parent `SessionId` values and updates lineage plus ordered child-ID collections transactionally. An arena key would therefore add a second identity layer without replacing the durable contract. Use graph helpers only for generic traversal where useful.
- Keep repository-worker one-shot priority selection as deterministic ordering/`min_by_key`; `phenix-plugin-repository-workers` reconstructs a bounded snapshot into a `BTreeMap` and selects the next eligible PR with `.min_by_key(|pr| (pr.priority, pr.queue_order, pr.number))`. There is no long-lived incrementally updated heap whose decrease-key/remove behavior would justify a priority-queue dependency. Introduce one only if that usage shape changes.

## Authority and configuration

Keep both Phenix-owned:

- `Authority` is an open capability-set lattice with attenuation semantics; generic bitset/permission crates do not own the namespace or security meaning.
- configuration resolution is a Phenix precedence/conflict/authority merge policy; generic config crates may be frontend parsers but must not replace canonical resolution semantics.

## Acceptance criteria

- [x] Observable public IDs/generations remain stable Phenix contract types.
- [x] Allocation cannot saturate/wrap into duplicate observation identity; exhaustion is explicit and tested.
- [x] `slotmap` is adopted only if it deletes real internal bookkeeping without a second identity mapping; otherwise rejection is recorded with code-size evidence.
- [x] Observable subscription trie ownership is documented and remains typed-path-specific.
- [x] Observable transaction/rollback semantics remain on `PhenixValue`, not JSON translation.
- [x] Reconciliation uses shared graph mechanics where applicable but no generic incremental framework owns transition policy.
- [x] Session-tree/worker-priority non-adoption decisions are recorded and backed by current usage shape.
- [x] No new crate is added solely to wrap a few lines of domain code.

## Non-goals

- Changing observable wire IDs.
- Reusing observation slots after deletion.
- Replacing `PhenixValue` with JSON.
- Changing reconciliation/domain behavior.
