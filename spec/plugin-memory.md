# Memory service plugin

status: implemented
coverage:
  - rust/crates/phenix-harness/src/basic_suite.rs
  - rust/crates/phenix-harness/src/lib.rs
  - rust/crates/phenix-plugin-memory/src/component.rs
  - rust/crates/phenix-plugin-memory/src/embedding_integration.rs
  - rust/crates/phenix-plugin-memory/src/maintenance_integration.rs
  - rust/crates/phenix-plugin-memory/src/provenance_integration.rs
  - rust/crates/phenix-plugin-memory/src/reranking_integration.rs
  - rust/crates/phenix-plugin-memory/src/restart_integration.rs
  - rust/crates/phenix-plugin-memory/src/supersession_integration.rs
  - rust/crates/phenix-plugin-memory/src/tests.rs
  - rust/crates/phenix-plugin-sessions/src/history_integration.rs

## Purpose

Define the normal Phenix memory mechanism as a replaceable userspace service.

Memory manages derived recall, hierarchical summaries, durable learned state, and reversible context compaction. Core supplies only generic plugin, authority, event, persistence, transaction, and dispatch mechanisms.

The default implementation is `phenix.memory`. An alternate implementation may replace it through the same declared contracts. Kernel-only and basic fixture profiles do not require memory.

New requirement: `spec/plugin-memory-freshness.md` extends this contract with explicit freshness, incremental revalidation, `memory.validate`, and canonical-decision reference semantics. PR #459 is not complete until both specifications are satisfied.

## Core rule

Raw durable sources are authoritative. Memory is a derived index over those sources. Model context is a temporary projection over memory and exact sources.

```text
raw durable sources
        |
        v
  derived memory
        |
        v
 context projection
```

Compaction may remove detail from the active model context. It must not remove the ability to resolve that detail from durable sources.

## Ownership

`phenix.memory` owns:

- memory identity and typed memory semantics;
- hierarchical summary nodes and their child/source references;
- retrieval, ranking, expansion, and recall policy;
- context checkpoints produced by the default compactor;
- memory extraction, consolidation, promotion, supersession, and expiry policy;
- memory-owned durable schemas and rebuildable indexes;
- memory synthesis callable identities;
- memory-specific options and defaults.

It does not own:

- raw session/model/tool history;
- context resource identity or context injection;
- model/provider selection infrastructure;
- generic persistence;
- execution authority;
- planning/objective/decision canonical state owned by another service.

A memory record may describe or index another service's state. It never silently becomes the canonical copy of that state.

## Service shape

The normal suite should provide one plugin with a focused set of interfaces:

```text
phenix.memory@1       recall and durable memory operations
context.compact@1     bounded reversible context compaction
context.expand@1      progressive disclosure into compacted detail
```

Optional focused providers may remain separate:

```text
memory.embed@1        semantic embedding
memory.rank@1         reranking
```

The first implementation must work without either optional provider by using deterministic metadata, exact/lexical search, recency, scope, and explicit links. Semantic providers improve recall but are not required for correctness.

## Component graph

The default component is conceptually:

```text
phenix.memory

exports
  phenix.memory@1
  context.compact@1
  context.expand@1

imports
  phenix.models.routing@1      required for model-backed synthesis
  phenix.options@1             optional configuration source
  memory.embed@1               optional
  memory.rank@1                optional

uses generic host capabilities
  durable data
  transactions
  event subscriptions
  service dispatch
```

The memory plugin may call session, execution, context, artifact, planning, or other services only through ordinary declared service contracts and authority. It receives no raw database handle and no privileged access to another plugin's durable namespace.

`phenix.context` remains responsible for assembling the final model context and enforcing the execution context budget. In the normal suite, its `context.compact@1` and `context.expand@1` bindings resolve to `phenix.memory`.

## Memory model

Memory has two related structures.

### Hierarchical context memory

Hierarchical nodes provide progressive disclosure over detailed history.

```text
project/topic summary
  -> session/subtopic summary
     -> task/event summary
        -> exact durable source references
```

A node contains at minimum:

```text
MemoryNode
  id
  kind
  scope
  summary
  children[]
  source_refs[]
  covered_time/range
  created_at
  generation
```

The hierarchy is not required to have a fixed number of levels. The default presentation may expose useful levels such as pointer, synopsis, detailed summary, event, and raw source, but stored nodes should represent semantic grouping rather than an arbitrary fixed depth.

A parent summary is an index into its children and sources, not a replacement for them.

### Typed durable memory

Long-term learned state is typed so different facts have different lifecycle rules.

The first-party implementation should support at least:

```text
episode     what happened
fact        a proposition believed to hold
procedure   how to perform or approach something
decision    a remembered decision plus rationale/source
```

Resource references may be attached to any memory without copying the resource itself.

Typed memory records contain provenance and temporal validity where meaningful:

```text
MemoryRecord
  id
  kind
  scope
  content
  valid_from?
  valid_until?
  supersedes[]
  source_refs[]
  confidence?
  created_at
  generation
```

`confidence` describes the derived memory record. It does not weaken exact provenance or turn uncertain content into canonical state.

## Source references

Every derived memory or compacted summary must retain exact durable provenance.

A source reference identifies enough information for the owning service to resolve the original durable record or range. The memory plugin may cache derived text for search, but a cache is not authoritative evidence.

The normal Phenix suite therefore needs addressable retained conversation/model/tool history as described by `spec/model-turn-protocol.md`. Memory must not depend on raw provider payloads.

If a source cannot be resolved durably, the memory service must either retain an explicitly owned immutable evidence record under a declared contract or refuse to treat that source as durable memory provenance. It must not silently store an unverifiable paraphrase as exact evidence.

## Recall

Memory storage and retrieval are separate concerns. Durable memory records are canonical memory state. Lexical, semantic, graph, and reranking data are rebuildable indexes over those records.

The service exposes two read modes:

| Mode | Purpose | Ranking |
| --- | --- | --- |
| structured query | Exact access by scope, kind, id, source, time, and ordering | None |
| search/recall | Discovery from text plus scope and time constraints | Exact and lexical first; optional semantic candidates and reranking |

Structured query must remain useful when every optional search index is absent. It must not depend on embedding similarity to answer an exact id, source, time, or chronological query.

Search is budgeted progressive disclosure. A search supplies or derives:

- requester and scope;
- query text;
- memory kinds where constrained;
- temporal bounds where constrained;
- target context budget;
- required exact references where applicable.

Default search should combine:

1. scope and authority filtering;
2. exact identifiers and explicit links;
3. lexical/full-text candidates;
4. recency and temporal validity;
5. optional semantic candidates;
6. optional reranking;
7. hierarchy traversal and expansion until the budget or evidence need is satisfied.

The result should contain compact nodes first and expose child/source references for further expansion. Retrieval must not eagerly inject every matching raw record.

Semantic similarity never decides authority, scope, freshness, or validity.

### Time ownership

The runtime owns the current clock. Agent-facing memory tools do not require the model to manufacture a Unix timestamp for ordinary reads or writes.

`memory.record` receives semantic memory data and the runtime assigns `created_at`. Semantic validity such as `valid_from` may still be supplied as a date because it describes the remembered fact rather than the storage operation.

Current reads omit a time filter. Agent-facing reads may express relative or historical intent with forms such as:

```text
max_age: "30d"
as_of: "2026-10-01"
created_from: "2026-09-01"
created_until: "2026-10-01"
```

The application boundary parses these forms and resolves them against the runtime clock. Internal memory timestamps use Unix seconds. Callers with millisecond clocks convert once at the memory boundary.

### Reference-backed freshness

`source_refs` and `supporting_dependencies` apply before both structured query and ranked search. A supplied reference remains part of freshness tracking whether it names a code location, file range, retained quote, tool result, session history range, artifact, or another durable resource.

A source revision change marks dependent derived memory for validation according to the freshness contract. Search indexes must not make an invalidated record current again. Historical reads may still return the record for times before the observed change.

## Compaction

The default memory plugin implements Phenix context compaction as reversible progressive disclosure.

Given an execution context projection and target budget:

1. deterministic pruning remains first;
2. already-derived compact nodes are reused when valid;
3. a bounded set of detailed entries is summarized when needed;
4. the active projection replaces covered detail with a checkpoint reference;
5. exact covered source ranges and retained exact references remain durable;
6. later `context.expand@1` calls can recover progressively deeper detail.

Compaction produces a durable derived checkpoint:

```text
ContextCheckpoint
  id
  summary_node
  covered_refs[]
  retained_exact_refs[]
  source_ranges[]
  model_generation?
  configuration_revision
```

Compaction does not promote facts, modify plans, mutate decisions, or rewrite raw history. Promotion and consolidation are separate operations.

A later compaction may use an earlier checkpoint as input for efficiency, but all durable provenance continues to resolve to original sources. Summary-only ancestry is insufficient.

## Consolidation and promotion

Memory maintenance is incremental. It should not repeatedly resummarize the complete history.

```text
new durable observations
  -> candidate extraction
  -> local grouping/linking
  -> bounded summary update
  -> optional durable promotion
  -> optional higher-level summary update
```

### Consolidation

Consolidation merges or supersedes derived memories when new evidence changes the best current representation.

Examples:

- several episodes support one durable procedure;
- a newer fact supersedes an older fact;
- multiple task summaries roll into one session/topic summary.

Old evidence remains addressable even when a derived record is superseded.

### Promotion

Promotion decides that session-local derived information should become longer-lived memory.

Promotion must be explicit in the memory service state transition. Context compaction alone never implies promotion.

The default policy should favor durable decisions, stable facts, recurring procedures, and important episodes. Transient execution state should normally remain session-scoped.

## Model routing

Memory synthesis reuses the existing model routing service. It does not introduce a memory-specific router.

The memory plugin invokes stable callable identities through the execution's pinned routing profile:

```text
memory.summarize
memory.extract
memory.consolidate
memory.validate
memory.resolve
```

`RoutingProfile.callable_targets` may map all five to one cheap memory model or route them independently. A Phenix product preset may map these callables to a cost-efficient model such as GPT Luna. A local/offline preset may map them to a local model. The memory plugin contains no provider-specific model name.

Suggested quality split:

```text
memory.summarize     cheap/local model is normally sufficient
memory.extract       cheap/local model is normally sufficient
memory.consolidate   cheap strong model by default
memory.validate      deterministic first; cheap/local model for semantic checks
memory.resolve       stronger fallback for ambiguous conflicts
```

When no special callable target is configured, ordinary routing-profile fallback applies.

Embedding and reranking are separate optional provider contracts and are not implemented by invoking the generative memory route unless explicitly configured that way.

## Pinned configuration

Any memory work that affects an execution's current context must use the immutable configuration revision pinned to that execution.

This includes:

- model routing profile;
- context budget policy;
- compaction policy;
- retrieval limits;
- enabled semantic providers.

Background consolidation may use the current memory-maintenance configuration, but every derived record stores enough generation/configuration provenance to explain how it was produced.

## Local-first behavior

The default mechanism must remain useful without an external API.

The baseline path should support locally:

- durable storage through plugin schemas;
- exact and lexical search;
- hierarchy traversal;
- recency/time filtering;
- deterministic ranking features;
- incremental checkpoint reuse;
- optional local embeddings/reranking when providers are installed.

Generative summarization and consolidation use routed model calls. A fully local routing profile therefore yields a fully local memory system.

Loss of an optional embedding/reranking provider degrades search quality, not memory correctness or provenance.

## Events and maintenance

Automatic memory maintenance should consume ordinary typed userspace events after their source state is durable. Event delivery itself is not the canonical memory history.

Maintenance may run incrementally through normal runtime task/job mechanisms. User-visible operations should not block on long-term consolidation unless their requested result depends on it.

Synchronous compaction is allowed when a model call cannot fit without it. Overflow recovery remains explicit and inspectable.

## Durable data

`phenix.memory` owns only derived memory state and indexes in its durable namespace.

Canonical records include:

- memory records;
- hierarchy nodes and edges;
- compaction checkpoints;
- supersession/validity links;
- generation/provenance metadata required to interpret those records.

Rebuildable state may include:

- FTS indexes;
- embedding vectors;
- reranking caches;
- derived candidate indexes.

Removing or disabling the plugin preserves its durable namespace. Re-enabling a compatible implementation may restore it according to normal plugin durable-data rules.

## Options

The memory plugin may define options through `phenix.options@1`. Option storage remains owned by the options service; behavior remains owned by memory.

Initial option surface should stay small. Prefer semantic policy controls over implementation tuning. Likely controls include:

```text
memory.auto_recall
memory.auto_promote
memory.recall.max_tokens
memory.maintenance.enabled
```

Model selection should normally remain in routing profiles rather than duplicating provider/model strings as memory options.

Exact compaction thresholds should follow model capacity and context budgeting rather than a fixed global token number.

## Suite composition

The normal Phenix Plugin Suite enables `phenix.memory` by default and binds the normal context service's compaction/expansion imports to it.

The basic fixture suite remains minimal and does not gain a mandatory memory dependency.

An alternate memory provider may replace `phenix.memory` without Core changes. A context implementation may also choose another compatible compaction provider when Harness policy permits it.

## Failure semantics

- Failure to write canonical memory state fails that memory operation atomically.
- Failure of optional semantic indexing leaves exact/lexical recall available.
- Background extraction/consolidation failure leaves raw source history valid and may be retried.
- Compaction failure must not discard active detailed context. The caller may prune further, select another route, or fail with explicit context exhaustion.
- Unresolvable provenance marks the derived record invalid for evidence-backed recall until repaired; it is not silently accepted.
- Model-generated memory never expands execution authority.

## Invariants

1. Raw durable sources remain authoritative.
2. Memory is userspace and replaceable; Core contains no Phenix memory semantics.
3. Compaction is reversible through exact durable provenance.
4. Context compaction and long-term promotion are separate state transitions.
5. Typed memory preserves semantic and temporal distinctions instead of flattening all history into text chunks.
6. Retrieval filters scope and authority before semantic ranking.
7. Semantic search is optional; exact/lexical recall remains functional without it.
8. Memory model work uses ordinary model routing with stable callable identities.
9. Memory durable state stays in the memory plugin namespace.
10. Memory never mutates another service's canonical state through summarization or consolidation.
11. Current-execution memory behavior uses the execution's pinned configuration revision.
12. Disabling memory removes memory behavior, not session/source history.

## Required regressions

- kernel-only profile contains no memory service or memory schema;
- normal Phenix suite activates `phenix.memory` by default;
- alternate memory provider replaces the first-party provider without Core changes;
- basic fixture suite still runs without memory;
- compacted context resolves through checkpoint -> child/source references to exact retained history;
- repeated compaction does not lose raw provenance through summary-only ancestry;
- compaction cannot mutate plans, decisions, objectives, execution state, artifacts, or source history;
- compaction failure leaves detailed active context intact;
- session-local memory can be promoted explicitly and is not promoted merely by compaction;
- newer fact may supersede an older derived fact while both source histories remain addressable;
- retrieval excludes out-of-scope memories before semantic ranking;
- exact/lexical recall works with embedding and reranking providers absent;
- memory synthesis resolves `memory.*` callable targets through the pinned routing profile;
- background maintenance failure does not corrupt authoritative session/source history;
- memory plugin cannot access another plugin's durable namespace directly;
- disabling/re-enabling memory preserves compatible durable memory state.

## Implementation structure

The implementation should follow the existing plugin split rather than putting memory behavior into Core or `phenix-sdk`.

```text
rust/crates/phenix-sdk/src/contracts/memory.rs
  neutral ids, commands, responses, values, interfaces

rust/crates/phenix-plugin-memory/
  src/lib.rs
  src/component.rs
  src/implementation.rs
  src/hierarchy.rs
  src/retrieval.rs
  src/compaction.rs
  src/consolidation.rs
  src/persistence.rs
```

`phenix-sdk` exposes only neutral authoring/contracts. `phenix-plugin-memory` owns runtime behavior and durable semantics.

Expected follow-up integration:

1. add neutral memory and compaction interfaces to `phenix-sdk`;
2. make retained portable model/tool history durably addressable by exact source reference;
3. implement `phenix-plugin-memory` with exact/lexical retrieval first;
4. bind `phenix.context` compaction/expansion to memory in the normal suite;
5. route `memory.*` synthesis callables through `phenix.models.routing@1`;
6. add incremental extraction/consolidation and promotion;
7. add freshness/revalidation from `spec/plugin-memory-freshness.md`;
8. add optional local embedding/reranking providers;
9. add product-level recall/compaction/revalidation observability and conformance tests.
