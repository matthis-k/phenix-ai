# Stable code entity identity and lineage

status: implementation-in-progress

Tracks #516 slice 7 and `token-efficiency.md`.

## Gap

Current language intelligence exposes definitions, references, implementations, symbols, diagnostics, and call hierarchy with exact source revisions. Stable logical entity identity and cross-revision lineage exist only in the spec.

## Implementation progress

- [x] Added logical entity, revision, facet-revision, and lineage value contracts to the language plugin.
- [x] Added a durable provider-independent entity revision map with immutable revision records and current-revision lookup.
- [x] Restart/provider-change coverage proves logical identity remains stable while provider, name, and path observations change.
- [x] Tests prove name/location changes do not redefine logical identity and ambiguous split lineage can remain tentative.
- [x] Added durable repository continuity state so rebuild/lost-map flows can explicitly publish available, rebuilding, or unavailable continuity.
- [x] Added deterministic current-vs-revision facet deltas so downstream consumers can revalidate only changed entity/relation neighborhoods.

## Required implementation

- [x] Define normalized `LogicalCodeEntity`, revision, relation, and lineage contracts.
- [x] Keep path/line/name as revision observations, not logical identity.
- [ ] Reuse rust-analyzer or SCIP facts where practical.
- [ ] Add deterministic file/revision fallback when semantic providers are unavailable.
- [ ] Preserve identity across confident move/rename cases.
- [x] Represent ambiguous extract/split/merge lineage as tentative evidence, not forced identity.
- [ ] Incrementally invalidate only changed entity/relation neighborhoods. (Typed facet-delta queries are implemented; automatic change-stream delivery remains.)
- [x] Persist provider-independent logical entity revisions so changing indexers does not rewrite the durable Phenix entity identity.

## Acceptance

- [ ] Rename and file move preserve logical identity when evidence is unambiguous.
- [ ] Semantic replacement creates a new identity or explicit replacement lineage.
- [ ] Dirty/stale analyzer state keeps exact source revision provenance.
- [ ] No analyzer still permits file-based work without false semantic guarantees.
- [ ] Index rebuilds preserve stable identities using the durable identity map; lost mappings report unavailable continuity. (Durable map/restart preservation and explicit persisted continuity state are implemented; rebuild orchestration remains.)

## Ownership

A first-party code-intelligence plugin owns normalized identity and lineage. External analyzers provide facts, not canonical Phenix identity.

## Baseline and dependencies

Checked against `main` at `46aa246361a7`. `phenix-plugin-language` records consumed observations and provider epochs; its presence does not prove a live Rust analyzer is installed. Use a configured managed/frontend provider when available and exact workspace reads otherwise. #595 depends on this PR's facet revisions and change stream; #596 depends on its entity resolution. The plugin owns logical identity and its persistent map; source files remain authoritative.

## Implementation draft

1. Define typed IDs and revisions in the shared code-intelligence contract and implement them in a replaceable userspace plugin. Separate repository identity from worktree/branch/dirty-buffer view identity. An entity has one logical ID and immutable observations containing source revision, view, location, name, signature, body evidence, provider generation, and analyzer version. Distinct entities with identical text still have distinct IDs.
2. Import typed language observations. SCIP is optional interchange for indexable symbols/occurrences/relations. Resolve positions using the declared source encoding. Only workspace-backed observations with verified content revision establish current disk facts. Dirty buffers carry frontend connection/provider epoch, document generation, and content identity; mixed/unknown observations remain non-authoritative for disk writes.
3. Persist an identity map and lineage evidence. Unchanged exact observations reuse their mapping. Ordinary edits of an established entity keep its ID and create a revision. Confirm rename/move continuity from an authoritative refactoring/edit receipt or a documented deterministic one-to-one continuity rule in a known source transition. Fingerprint similarity alone proposes tentative lineage. Ambiguous matches, extract/split/merge, and explicit replacement keep distinct IDs and typed edges. Absence from a partial/stale index is not proof of deletion.
4. Publish facet revisions for existence, name/location, signature, body, and named relation sets. A full revision changes whenever observations change; a facet stays equal only when the provider can establish that facet's equality. Unknown semantic equality remains unknown even when normalized text matches. This lets #595 invalidate a path claim on move while preserving a separately verified body claim.
5. Query requests pin view/source/index revision and return evidence plus coverage `Complete` or `Partial(reason)`. Bounded neighborhoods include a continuation cursor bound to that revision. Stale cursor/provider generation is a typed error. File/revision fallback reads current authorized workspace bytes and does not reuse stale semantic results as current.
6. Persist each identity/revision/relation change and its sequence in one transaction. Index rebuilds replay this map and event cursor. Analyzer replacement reconciles observations against existing IDs; it cannot guarantee old IDs if the durable map is lost. Mark the affected region unavailable while rebuilding. Emit catch-up/gap information for memory consumers.
7. Recheck scope on each query and reference resolution. Optional analyzers neither download nor start automatically merely because this plugin is installed.

## Fixture sequence

- Verified rename/move preserves ID, changes location/name facets, and retains immutable earlier observations.
- Body edit retains established ID with a new body revision; explicit replacement records a separate ID/edge.
- Identical candidates and partial-index absence stay unresolved; no forced merge or deletion.
- A dirty-buffer view cannot satisfy a disk revision write precondition.
- Rebuild with the durable map preserves IDs; lost-map recovery reports unavailable continuity.
- Analyzer replacement and paginated reads preserve scope, revision, coverage, and ordering.
- A committed revision change and its event survive restart together.

## References

- [Code identity](../token-efficiency.md#code-intelligence), [document provenance](../language-intelligence.md#documents), [consumed observations](../language-intelligence.md#consumed-observations).
- [Current observation/epoch owner](https://github.com/matthis-k/phenix-ai/blob/46aa246361a7bfc1ea477d08ab40c204acc89a54/rust/crates/phenix-plugin-language/src/implementation.rs).
- [SCIP schema](https://github.com/scip-code/scip/blob/main/scip.proto) defines symbols, occurrences, ranges, and relationships, not cross-revision semantic identity. [LSP 3.17](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/) defines document versions and position encodings.
