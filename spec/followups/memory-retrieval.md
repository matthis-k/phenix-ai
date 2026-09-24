---
status: partial
---

# Memory retrieval ownership

## Goal

Separate generic information-retrieval mechanics from Phenix memory semantics without outsourcing freshness, supersession, scope, fallback policy, or source-of-truth ownership.

The memory plugin already externalizes embedding and reranking through `MemoryEmbeddingInterface` and `MemoryRankInterface`. This follow-up closes the remaining generic candidate-retrieval boundary and makes the implementation choice evidence-driven.

## Theoretical problem

Memory recall is a staged information-retrieval problem:

1. semantic eligibility: scope, lifecycle/freshness, supersession, source validity
2. candidate generation: lexical/inverted-index retrieval and optionally semantic candidates
3. ranking/reranking
4. domain revalidation and deterministic final ordering

Only stages 2 and the generic mechanics inside stage 3 belong to an IR implementation. Stage 1 and the final acceptance policy are Phenix semantics.

## Decision

Keep the durable memory store authoritative. Search indexes are derived and rebuildable.

Preserve the current embedding/rank provider contracts. Add one explicit candidate-search abstraction only if it allows a maintained IR implementation to replace the local lexical scan without pushing memory policy into that implementation.

Preferred shape:

```rust
trait MemoryCandidateSearch {
    fn upsert(&mut self, document: SearchDocument) -> Result<(), SearchError>;
    fn remove(&mut self, id: &MemoryId) -> Result<(), SearchError>;
    fn search(&self, query: &SearchQuery) -> Result<Vec<SearchHit>, SearchError>;
    fn rebuild(&mut self, documents: impl Iterator<Item = SearchDocument>) -> Result<(), SearchError>;
}
```

This may be an internal strategy trait or a Phenix component interface. Use a public component interface only if independent plugins/backends need to provide it; do not create a contract solely to wrap one private crate.

## Implemented boundary

`retrieval.rs` now separates Phenix eligibility/revalidation from `CandidateSearch`. The default `LexicalCandidateSearch` is deliberately private and scan-backed: it can be replaced by a maintained index implementation without moving scope, kind, temporal visibility, or supersession policy into the search backend. Search hits are positional/score-only implementation data and are revalidated against authoritative records before final ordering.

## Tantivy decision gate

Evaluate Tantivy as the maintained lexical/inverted-index implementation against the current deterministic lexical scan.

Adopt Tantivy when all of the following are true:

- production retrieval code becomes smaller or materially more capable,
- index state can remain derived/rebuildable rather than authoritative,
- deterministic hit ordering can be normalized at the Phenix boundary,
- compile/startup/storage overhead is acceptable for the default harness,
- recall benchmarks show a meaningful crossover for realistic memory sizes.

If those conditions fail, keep the small scan as the default implementation but retain the candidate-search boundary only if another backend is expected soon. Do not add Tantivy merely to replace a few lines.

Record benchmark data for at least 100, 1k, 10k, and 100k memory records with representative query lengths. Measure warm query latency, rebuild/index cost, and retained index size.

## No ANN yet

Do not add HNSW/vector-index infrastructure in this PR. Current semantic candidate work is bounded to a small candidate set and embedding/ranking are already provider-backed. Add ANN only when profiling shows brute-force semantic candidate work, not lexical candidate generation, is the bottleneck.

## Domain revalidation

Regardless of retrieval backend, the memory plugin must re-check returned IDs against authoritative durable records before use:

- requested scope
- freshness/lifecycle state
- supersession/tombstone state
- source validity/provenance requirements
- any execution/session visibility rule

A stale/rebuilt/buggy index cannot make an otherwise ineligible memory visible.

## Ordering

Search-engine score is advisory input. Final output remains deterministic after applying Phenix policy. Define explicit tie-breaking using stable memory identity and any existing recency/policy dimensions.

## Persistence interaction

Build on the durable collection migration PR:

- scan authoritative memory records efficiently for index rebuild,
- update/remove derived index entries when durable mutations commit,
- do not persist a second canonical copy of memory records inside the IR engine,
- if the index is persisted for startup speed, mark it derived/cache state and make corruption/missing index recoverable by rebuild.

## Acceptance criteria

- [x] Retrieval pipeline is explicitly split into eligibility, candidate generation, ranking, and domain revalidation.
- [x] Existing `MemoryEmbeddingInterface` and `MemoryRankInterface` remain the canonical embedding/rerank extension points.
- [x] Generic lexical candidate retrieval is behind one implementation boundary rather than entangled with memory policy.
- [ ] Tantivy is either adopted with benchmark evidence or rejected with measured evidence and no speculative dependency.
- [ ] No ANN/vector index is added without evidence.
- [x] Candidate-search results are revalidated against scope/kind/visibility/supersession policy before ranking; any future derived index must preserve this boundary.
- [ ] Index rebuild from durable state is deterministic and tested.
- [x] Final recall ordering is deterministic for equal scores.
- [ ] Failure of the derived search index has an explicit policy: rebuild/fallback only as an availability mechanism, never as legacy compatibility.
- [ ] Measure production LOC and dependency/compile impact of the chosen implementation.

## Non-goals

- Changing what memories are allowed to persist.
- Changing fall-through memory activation policy.
- Moving freshness/supersession policy into an external search engine.
- Adding a vector database by default.
