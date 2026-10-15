# P2 advanced agent: composition, memory, compaction, code intelligence and reload

Status: PLANNED. No P2 implementation or test acceptance is claimed.
Updated: 2026-10-15
Parent: [M1 roadmap](m1-roadmap.md)
Historical reference: archived oldmain, especially rust/crates/phenix-agent-configurations and rust/crates/phenix-plugin-memory.
P1 evidence and CI gate: [P1 release acceptance](p1-release-acceptance.md) and [post-P1 CI cutover PR #879](https://github.com/matthis-k/phenix-ai/pull/879).

## Goal and release boundary

P2 is the Advanced agent product. Its kernel is the same M1 kernel as P1. Advanced features live in independently replaceable plugins; the kernel retains authenticated dispatch, generation identity, pending tickets, graph scheduling, authority and cancellation. A product composition selects plugins and their declared bindings. It never becomes a second agent executor.

The P2 release target is a usable agent with P1 parity, reference-aware long-term memory, reversible compaction, code intelligence and safe plugin hot reload. Restore any useful oldmain behavior through the new contracts and graph, not by importing the old kernel or its embedded application framework. Planning, jobs, session trees and workspace execution from oldmain require a parity audit but are not automatic release dependencies unless their absence breaks the specified P2 workflows.

**Sequence:** finish the accepted P1 baseline and the single-authority CI cutover before P2 implementation. Specifications and test vectors may be prepared now. Tests are written RED before implementation for every feature slice. Each implementation PR is ready for review when executable code exists. An exact-head full CI pass is required for release acceptance.

## Five immutable package compositions

Build one kernel and compose increasingly capable products. A preset is a packaged, versioned artifact list with selected contract bindings, launch entry and declared grants. It is not another runtime implementation. An installed user-facing preset must start without a separate composition file.

| Flake package / executable | Derives from | Plugins and purpose | Release exposure |
| --- | --- | --- | --- |
| phenix-kernel | M1 kernel | Kernel-only CLI, offline contract/package inspection, no agent graph or model | Public |
| phenix-basic | Kernel + P1 composition | Model, tools, Skills/Context, Session/Journal/Storage, Basic graph, Terminal/JSONL/ACP | Public, already packaged |
| phenix-basic-verification | Basic composition + test provider override | Deterministic history-aware Model and existing acceptance fixtures | CI only, already packaged; retain |
| phenix-advanced | Basic composition + P2 plugins | Advanced memory, context planning/compaction, code intelligence, authorized reload integration | Public after P2 |
| phenix-advanced-verification | Advanced composition + isolated test providers | Typed scenario Model, controllable failures, fake code workspace, clock/revision and reload fixtures | CI only |

Do not silently replace the default Basic Model, weaken Basic grants or add verification-only providers to either user-facing closure. The scripted P1 model may remain the offline reference default; production Model bindings stay replaceable (HTTP or another provider). The verification binary must exercise the real kernel, real session/journal/storage, real graph and real plugin entry points. Only the model, clock and external environment inputs are controlled.

Use the P1 flake's basicCompositionSpec, historyVerificationComposition and basicPreset pattern. Prefer a shared composition function that adds artifacts and replaces explicit bindings, without copying serialized JSON by hand. Produce manifests/digests for all five presets. The verification product must have exactly the same Advanced bindings except for named fixture providers. Check no fixture artifact or test-only capability is reachable from phenix-advanced's Nix runtime closure.

## Scope and implementation order

| Stage | Dependency | Owned change | RED test / completion gate |
| --- | --- | --- | --- |
| P2-0: composition and parity | P1 + CI authority | Freeze oldmain versus P1/P2 behavior inventory, five preset derivations, contracts, separate fixture package | Launch/inspect all presets; prove Basic binary/behavior unchanged; verification plugins excluded from public closure |
| P2-1: evidence model | P2-0 | Typed references, memory scopes, temporal/freshness metadata and durable evidence links | Stable references survive restart; stale/forged/out-of-scope references reject |
| P2-2: memory | P2-1 | Write, extract, reconcile, retrieve, rank and update hybrid memory through plugin contracts | Multi-session, temporal, update, abstention and provenance fixtures; benchmark baseline |
| P2-3: reversible compaction | P2-1; may build alongside P2-2 | Policy, summaries, snapshot lineage, deterministic expansion and source hydration | Tight-window continuation, exact replay/rehydration, crash safety, no duplicated tool effects |
| P2-4: code intelligence | P2-1, P2-2 as needed | Index/symbol graph, diagnostics, revision-aware semantic lookups and memory links | Code rename, file edit, stale symbol, diagnostics and authorization fixtures |
| P2-5: generation-safe hot reload | P2-0; P2-1 for persistent plugin state | Stage, inspect, quiesce, migrate, activate or roll back a new plugin generation | Repeated native reload under active roots, pending tool calls, invalid ABI/grants and restart |
| P2-6: advanced verification and release | P2-2 through P2-5 | Composition fixtures, production transport parity, benchmark/regression report | Full reproducible exact-head CI, P1 regression and final-main check |

Each stage must use existing M1 typed values and native descriptor validation. New plugin contracts are versioned where required. Search for maintained crates and evaluate fit, license, maintenance, dependency weight and Nix/ABI support before writing a parser, vector index, database or protocol. Keep the [R1 library audit](m1-r1-library-audit.md) current when a library is selected.

## Advanced graph topology and contract ownership

Provide an additive phenix-plugin-advanced-agent plugin for the Advanced composition. It contributes typed graph nodes and event handlers around the selected Basic agent; it does not fork the Basic model/tool loop or implement a second scheduler.

- On prompt admission: resolve authorized memory/context anchors, check code/source revisions, retrieve budgeted evidence and choose a checkpoint/current-view policy before the Model request is prepared.
- On committed conversation/tool events: enqueue idempotent extraction, association updates and optional memory maintenance. Effects carry session, source event and generation identity. A failed background maintenance step cannot duplicate or reorder the active Model/tool turn.
- On context threshold: commit compaction checkpoint and publish the selected compacted view through typed Context state before the next Model call. Refresh only impacted projections through generated on-change hooks.
- On code revision or source removal: invalidate dependent memory references, index entries and derived context. The next lookup checks freshness before using the entry.
- On reload: let the kernel's generation transition determine which version handles each root. Advanced handlers receive the selected generation and its authorized providers.

Define any needed Memory, ContextView, Compaction, CodeIndex and ReloadControl contracts in a standalone public contract crate. A production implementation remains a selectable provider. Model-facing answers expose the source reference IDs used by retrieval and expansion, so tests and users can inspect evidence. The Basic topology, plugins and binary remain byte-for-byte independent of this additive composition.

## Reference-aware memory

Retain the capabilities evident in oldmain's phenix-plugin-memory: canonical source references, scoped records, context anchors and associations, ranked retrieval, freshness/dependency revalidation, supersession, consolidation, compaction and expansion. Do not copy its old runtime API. The new memory plugin uses M1 public contracts and the selected Storage provider.

### Evidence and identity

Define typed records with: stable memory ID; kind (fact, event, preference, task or derived observation); authorized scope (session, project, user or explicit shared scope); subject/predicate/object or typed payload; source references; source revision and byte/line/span or event offsets when known; observation and validity times; confidence; evidence-versus-inference flag; supersedes links; status (current, historical, uncertain or needs validation); version; and durable creation/update cause. Every answerable factual memory must link to material that can be fetched and checked. References must resolve through authorized providers, not by exposing raw paths or assuming the reference grants access.

Store an immutable source/event record and mutable versioned projections separately. Make source deletion, redaction and consent limits invalidate derived facts and indexes. Avoid cross-session contamination and inferred facts masquerading as user statements. Preserve history when a fact is corrected: current queries select the latest valid supported version; as-of queries retrieve the historical version. Duplicate event ingestion must be idempotent.

### Hybrid retrieval: default candidate

Start with three complementary methods:

1. Exact/structured lookup for references, IDs, scopes, time bounds and key/value relations. SQLite tables/indexes and FTS5 are candidates using the existing rusqlite provider.
2. Lexical retrieval with source-term expansion for code identifiers, names, paths and exact wording.
3. A bounded relation/temporal graph walk over entities, associations, supersession and evidence edges. Embeddings and semantic rerankers are optional selected providers, not required for offline correctness.

Deduplicate results by source identity and current validity. Rank with explicit relevance, freshness, confidence, scope and source coverage. Apply retrieval token budgets and expose the scored evidence list plus rejected/stale reasons for debugging. A compact entity profile may help query expansion but cannot replace source evidence. The agent should abstain when evidence is missing or contradictory.

Do not turn on every retrieval backend by default. Measure lexical-only, structured+lexical, structured+graph and optional dense/hybrid profiles against the same tests and token budget. Keep the simplest configuration that demonstrably improves recall, answer accuracy or cost. All modes implement one Memory contract and can be replaced independently.

### Research to reproduce, not performance claims

- [LongMemEval](https://arxiv.org/abs/2410.10813) tests extraction, multi-session reasoning, time, updates and abstention. Its official repository also published a [2026 v2 update](https://github.com/xiaowu0162/LongMemEval).
- [Mem0](https://arxiv.org/abs/2504.19413) motivates selective fact extraction and optional relational memory; compare graph benefit against its cost.
- [Zep/Graphiti](https://arxiv.org/abs/2501.13956) motivates temporal entity/relationship revisions and hybrid graph+text retrieval.
- [Hindsight](https://arxiv.org/abs/2512.12818) separates facts, experiences, entity summaries and evolving beliefs; adopt that distinction where traceable.
- [Redis's 2026 hybrid memory evaluation](https://redis.github.io/redis-ai-research-public/longmemeval-agent-memory/) compares raw retrieval, extracted memories and a hybrid on the same question set. Use this as evidence that retaining raw references alongside extracted facts merits testing.

Published model-dependent scores cannot certify Phenix. Record dataset/version, ingestion budget, context tokens, retrieval precision/recall, final answer correctness, abstention, latency and cost. The offline scripted Model is the gating oracle; external model benchmarks are secondary non-gating reports unless a reproducible model/version is pinned.

## Reversible compaction

A compacted prompt is a derived view, never the sole durable record. The Session/EventJournal append-only ordered messages, complete tool requests/results and source references remain authoritative. A compaction checkpoint records its input event range and hashes, parent checkpoint, selection policy/version, model/version if a summarizer was used, generated summary, retained recent messages, excluded ranges and hydration references.

At a configurable context threshold, preserve system/tool/authority context, unresolved tool calls, important recent exchanges and budgeted recalled evidence. Summarize older completed spans; store the summary with cited immutable ranges. Permit nested checkpoints and repeated compaction without losing the oldest evidence. Expansion can restore an original range byte-for-byte by replaying the journal, or selectively hydrate cited source ranges into a new prompt. Do not claim that a summary can recreate exact wording without the original records.

**Required properties:** transactional checkpoint commit, restart recovery, stable message IDs, deduplication of injected evidence, bounded token count, source visibility, optional human inspection, undo/switch-back to an earlier checkpoint view, and explicit retention policy for intentionally deleted data. Compaction never executes a tool, marks a tool as completed, changes the authoritative session timeline or grants access to hidden sources. If original bytes were intentionally erased, report that expansion is unavailable rather than silently inventing text.

Keep compaction, memory retrieval and journal persistence separate contracts. The advanced graph chooses when to compact, what to retrieve and which selected Context view the Model receives. The kernel does not know about token windows or summaries.

## Code intelligence

Restore the useful oldmain phenix-plugin-language concepts: logical code entities, revisions, source provenance, relations, diagnostics, edited-symbol lineage and stale-reference detection. Do this as code intelligence plugins, not as a kernel service.

Use maintained Tree-sitter parsers for syntactic symbols and incremental indexes. Prefer an existing language server and its LSP transport for semantic definition/references/rename/diagnostics when available; use the language server's native capabilities rather than reconstructing semantic analysis with ad-hoc regular expressions. Support at least Rust in P2 acceptance, with language adapters replaceable. Maintain repository/worktree identity, document hash, symbol identity and revision-bound spans; update only affected files. Record the difference between exact LSP identity and best-effort syntactic matching.

Tools expose file/symbol search, outline, definition, references, diagnostics and changed-neighborhood queries with bounded results and explicit evidence. If editing is included, route edits through existing authorized workspace tools and validate pre-edit revision plus post-edit diagnostics. Memory references to code must become needs-validation when a file or symbol changes. A stale reference cannot quietly resolve to a similarly named but different function.

## Safe plugin hot reload

Plugin reload is a **kernel generation lifecycle operation**, requested through a separately authorized control plugin or host command. The manager validates artifact metadata, ABI, contracts, requirements, maximum grants and graph compatibility before loading or executing new plugin code. Prepare a new complete generation, then atomically publish its bindings and graph after all necessary state migration and validation succeed.

- Existing roots retain their original generation and provider leases until they finish or are cancelled. No old shared library is unloaded while any callback, pending ticket, observer or borrowed state depends on it.
- New roots bind the new generation. Stateful plugins must declare versioned snapshot/export/import hooks or an explicit restart-required policy. Migrations use rollback-capable transaction boundaries and cannot widen grants.
- Reject missing imports, schema changes without migration, denied authority, invalid package digests and failed initialization without altering the active generation. Expose stage/apply/rollback/status, exact package digests and diagnostic events through introspection/logging plugins.
- Support replace, add, remove and update for independently packaged native plugins. Audit whether dynamically loaded native libraries can be safely unloaded on each target; use content-addressed copied library paths and deferred retirement where required. Never assume that a Rust shared library or foreign callback is safe to unload merely because a file changed.
- A crash during staging or migration must recover the last committed generation and durable session state. A denied reload must not run candidate constructors or effects.

Use the existing kernel resolver, generation ID, lease retirement, tickets and cancellation logic. A new second scheduler or duplicate contract registry fails review. New code is limited to generic generation transition and narrowly scoped SDK hooks.

## Advanced verification plugins and acceptance

Create a dedicated phenix-plugin-advanced-test-model (or similarly named fixture crate), packaged only with phenix-advanced-verification. It implements the same real typed Model contract. For each scenario it consumes the complete Model request, including actual ordered messages, tool-call IDs/results, contextual evidence and selected revisions. Its deterministic state machine refuses a golden answer when required evidence is absent or stale. It must not read the test answer directly from test harness state, fixture filesystem or private Storage records. Provide a second controllable fixture for source revisions, clock, provider failures and hot-reload lifecycle as needed.

Every test runs the packaged executable, not a function that bypasses the graph. The suite records rendered output, actual model inputs, tool effects, journal offsets, state revisions, artifacts and provenance. Use an isolated SQLite database and real restart between phases. At least one independent ACP client and one Terminal or JSONL frontend drive the same scenarios.

| ID | Independent scenario | Required result / negative control |
| --- | --- | --- |
| AV-01 | P1 Basic multi-turn, tools, streaming, cancellation and replay on Advanced preset | Identical observable P1 semantics; no hidden fixture provider |
| AV-02 | Two sessions; fact introduced in A and asked in B with authorized shared scope | Correct recall with source citation; denied scope cannot retrieve it |
| AV-03 | Later correction to earlier fact; current and as-of questions | New answer for current query, old answer for historical query, both with provenance |
| AV-04 | Multi-hop association, partial evidence, contradictory evidence and no supporting fact | Only supported answer; abstain when evidence insufficient; no fabricated citation |
| AV-05 | Source revision changes or memory explicitly retracted | Invalidate stale fact and related code references; no stale answer |
| AV-06 | Real input exceeds configured context threshold; run compaction | Model receives bounded compacted view; old facts stay retrievable through references |
| AV-07 | Ask verbatim question about old compacted text; expand by journal range | Original text byte-for-byte; no summary hallucination; undo selects old view |
| AV-08 | Crash during summary checkpoint, during tool call and after committed checkpoint | Restart restores consistent state; no duplicated side effects or orphaned tool-call completion |
| AV-09 | Parse Rust repository; find symbol, relations, references and diagnostics | Correct revision-bound spans and typed results; no regex-only false positives |
| AV-10 | Rename/move/edit symbol between requests | Incremental reindex and revalidation; stale ID rejected or resolved through explicit lineage |
| AV-11 | Reload stateless model/provider while a different root is active | New root sees new provider; in-flight root finishes on old provider; old lease drains |
| AV-12 | Reload stateful memory provider with schema migration | State survives across generation and process restart; invalid migration rolls back |
| AV-13 | Denied grants, forged package, incompatible ABI and corrupt graph on reload | Candidate rejected before effects; active generation unchanged |
| AV-14 | Remove and re-add plugin with pending calls, cancellation and repeated reload cycles | No use-after-unload, leaked tickets or doubled observer delivery |
| AV-15 | Build all five presets and inspect closures and contract bindings | Fixture plugins only in verification products; Basic unchanged |
| AV-16 | Compare lexical, hybrid and optional dense memory using frozen corpus | Report answer quality, retrieval completeness, p95 latency, token use and DB size; no ungrounded benchmark claim |

For each test include the counterfactual: remove or corrupt retrieved source, change only the revision, disable the expected graph edge, reorder independent events or deny access. The test must then fail or abstain. Assert a known exact answer only after proving the model actually consumed the cited evidence. Validate that the fixture model can also perform a tool-call/continuation sequence, instead of acting as a static canned-answer lookup.

Add independent contract/ABI vectors, compiled external plugin fixtures, malicious negative cases, native sanitizer or equivalent lifetime tests where feasible, and repeated process restarts. Run Rust unit/integration and Nix package checks first, then the full canonical flake CI release inventory. Require exact-head artifacts and a green result again on the integrated main SHA.

## Compatibility inventory from oldmain

The original archived advanced profile included memory, planning, language, repository workers, jobs, session trees, workspace, hooks, debugging, command toolbelt, options, execution and environment adapters. Before P2-0 closes, classify each as implemented by P1, required P2 replacement, optional composition plugin or deferred work. Record user-observable scenarios rather than simply restoring crate names. Do not claim oldmain parity from the four headline features alone.

Retain oldmain's tested memory-specific semantics where they remain useful: immutable records, scope, freshness, canonical code references, source validation, embeddings/reranking as optional providers, source supersession and confirmation of contextual associations. Port contracts and acceptance, not oldmain's implementation-bound service calls.

## Naming and adjacent post-P1 work

PR #852 used P2a–P2d for multi-client state projection and I/O routing before the Advanced release label was assigned. Keep its work and dependency, but name its track P2-IO1 through P2-IO4 when integrating its roadmap. Advanced P2-0 through P2-6 is the product track. The multi-client I/O track can proceed in parallel after the CI gate; P2 does not depend on synchronized multiple frontends unless a verification scenario explicitly requires them.

## Release decision

P2 is ACCEPTED only when all P2-0 through P2-6 gates pass together, each preset starts as a packaged executable, memory/compaction can produce verifiable evidence, code intelligence handles revision changes, hot reload is generation-safe, and Basic tests still pass. Publish a comparison against the oldmain capability inventory and actual benchmark measurements. Keep the milestone PLANNED until executable checks exist.
