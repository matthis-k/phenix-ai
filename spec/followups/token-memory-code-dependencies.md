# Memory dependencies on code identity

status: implementation-in-progress

Tracks #516 slice 8. Depends on the stable code entity/lineage tracker.

## Gap

`phenix.memory` already has exact provenance, supersession, canonical-resource references, and freshness/revalidation. Memory records cannot yet depend on logical code entities or relation revisions, so repository knowledge cannot survive refactors selectively.

## Implementation progress

- [x] `MemoryRecord` and extraction observations carry exact supporting dependency revisions separately from durable source references.
- [x] Freshness initialization and revision observation preserve original support while moving affected claims to validation.
- [x] Record normalization rejects derived-memory services as authoritative supporting dependencies.
- [x] Revision observation can page deterministically through the reverse dependency index instead of repeating the first 100 memories.

## Required implementation

- [ ] Add code-entity and code-relation dependency references to derived memory.
- [x] Record the exact supporting entity/relation revisions used by a claim.
- [x] Route changed dependencies into the existing `current -> needs_validation -> historical` lifecycle.
- [ ] Preserve current memory across move/rename only when every declared supporting facet remains verified unchanged.
- [x] Revalidate/transition only dependency-indexed memories, with revision-bound pagination for large reverse indexes.
- [x] Model-backed revalidation cannot restore a claim to current after exact supporting revisions changed; a superseding claim must record new exact support.
- [ ] Keep code dependencies separate from fallback workspace-association identity.
- [x] Use deterministic checks before `memory.validate` or `memory.resolve`; changed exact support now blocks model revalidation from restoring current state.

## Acceptance

- [ ] Moving a function preserves location-independent claims with unchanged support and invalidates claims that depend on its old location/name.
- [x] Changing a relied-on dependency revision marks only reverse-indexed dependent memories for validation; facet-specific code wiring remains in #594.
- [ ] Deleted/replaced entities cannot remain silently current.
- [ ] Rebuild/restart preserves dependency provenance and freshness state.
- [x] No code-memory record becomes canonical source state.

## Ownership

Memory owns derived claims and freshness. Code intelligence owns entity/relation identity and revisions. Exact code/source evidence remains authoritative.

## Baseline and dependencies

Checked against `main` at `46aa246361a7`. Reuse `MemorySourceReference`, `MemoryDependencyRevision`, `MemoryFreshnessRecord`, and the existing freshness lifecycle. #594 must first expose exact entity/relation facets and revision queries. Avoid a second code-dependency registry in memory: generic service/resource dependencies can address code-owned facet resources. Historical source evidence and the latest observed dependency revision have different roles.

## Implementation draft

1. Have the code owner expose typed facet references that project to the existing service/resource/revision dependency shape. Example roles are entity existence, name/location, signature, body, and a declared relation set. Extend `MemoryRecord`/record admission with a separate `supporting_dependencies: Vec<MemoryDependencyRevision>` (or equivalent existing generic field) alongside its exact `source_refs`. Today `initial_state` derives dependencies only from `source_refs`, which have no revision field; merely publishing code facet resources cannot attach them to a claim. Verify each supporting revision at the code snapshot before first `Current` admission. Derive the reverse index from these dependencies in the same record transaction. Unknown facets conservatively cover the full entity/relations.
2. Keep the supporting revision immutable in the claim record and preserve it separately from the last observed revision in `MemoryFreshnessRecord` and any revalidation receipt. The current freshness code overwrites the observed dependency revision on changes; that update cannot rewrite historical support or mark the claim valid. A successful revalidation adds new support and evidence using the existing record version/supersession mechanism.
3. Build the reverse index from claim/facet dependencies. Changes mark affected claims `NeedsValidation` in the same transaction as their derived index/cursor updates. A pure move preserves only claims whose declared supporting facets are proved unchanged. Claims about path, name, module membership, callers, or visibility must be invalidated when those dependencies change. Stable logical ID alone never establishes freshness.
4. Reuse deterministic availability/revision/temporal/supersession checks before model-backed validation. Deletion or confirmed replacement invalidates existence-dependent claims. Tentative lineage and incomplete/stale analyzer coverage keep claims unresolved. A model verdict cannot make unavailable exact evidence current.
5. Event processing is an accelerator for recall checks. Before returning `Current`, verify dependency revisions at a declared code snapshot or prove the event cursor caught up to that snapshot. Missing code provider, event gaps, and rebuilds yield `NeedsValidation`/omission or labeled historical results. This closes the race between code mutation and asynchronous memory processing.
6. Extend `ObserveRevision` with a revision-bound cursor or an equivalent durable batch job. It currently loads at most 100 indexed memories and repeats the same first page on retry. Process every indexed dependent, commit each page's transitions and cursor atomically, and keep claims outside completed pages non-current until the event cursor reaches the requested code snapshot. Replay after a gap reconciles all affected dependencies against the code owner. Use one durable schema migration for the new claim field and cursor; older records retain their exact source dependencies without invented semantic support. Propagate declared dependencies through extract/consolidate/promote, or leave the derived result non-current until support is recomputed.
7. Keep workspace-recovery association identity separate. Memory remains a derived claim store. Validation takes ordinary bounded helper budgets and records its work for #599.

## Fixture sequence

- A body-dependent claim survives a verified move; a claim naming the old path becomes non-current.
- A signature/relation change invalidates exactly its dependents; unknown facet requirements invalidate conservatively.
- Stable ID with a changed body never preserves an unsupported behavioral claim.
- Original supporting revision remains retrievable after an observed revision changes.
- Recall between source change and event delivery checks current revisions and cannot return stale `Current`.
- More than 100 dependents finish across revision-bound pages; retries resume after the returned memory-ID cursor instead of replaying page one. Restart/event-gap replay gives the same freshness; missing analyzer and tentative lineage remain unresolved.
- Record, extract, consolidate, and promote preserve declared support or require revalidation; old records migrate with exact raw provenance.
- Migration preserves generic provenance and introduces no fabricated code dependencies.

## References

- [Memory freshness and recall](../plugin-memory-freshness.md#recall), [source provenance](../plugin-memory.md#source-references), [repository memory](../token-efficiency.md#repository-and-task-memory), [#594](https://github.com/matthis-k/phenix-ai/pull/594).
- [Existing generic dependency contract](https://github.com/matthis-k/phenix-ai/blob/46aa246361a7bfc1ea477d08ab40c204acc89a54/rust/crates/phenix-sdk/src/contracts/memory_freshness.rs), [Current observed-revision handling](https://github.com/matthis-k/phenix-ai/blob/46aa246361a7bfc1ea477d08ab40c204acc89a54/rust/crates/phenix-plugin-memory/src/freshness.rs), [record and limited revision command](https://github.com/matthis-k/phenix-ai/blob/46aa246361a7bfc1ea477d08ab40c204acc89a54/rust/crates/phenix-sdk/src/contracts/memory.rs), [current index iteration](https://github.com/matthis-k/phenix-ai/blob/46aa246361a7bfc1ea477d08ab40c204acc89a54/rust/crates/phenix-plugin-memory/src/implementation.rs).
