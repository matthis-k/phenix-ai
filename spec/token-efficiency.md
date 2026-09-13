# Token-efficient default harness

status: specification-only

## Goal

Define the default Phenix Harness strategy for reducing model work without trading away correctness, provenance, or recoverability.

Optimize successful work per model cost and compute. Raw token count is diagnostic input, not the objective. Cached input, fresh input, cache writes, output, reasoning, latency, retries, delegated work, state reacquisition, and task outcome remain separate measurements.

This profile builds on:

- `model-turn-protocol.md` for typed turns, tool results, retained history, and usage;
- `context-catalog.md` for exact resources, revisions, and progressive loading;
- `context-artifacts-pruning.md` for durable large outputs and reversible pruning;
- `context-compaction.md` for per-execution budgeting and checkpoints;
- `plugin-memory.md` and `plugin-memory-freshness.md` for derived memory, provenance, and revalidation;
- `skill-discovery.md` for progressive skill disclosure;
- `language-intelligence.md` for current semantic code observations;
- #509 for model selection and routing;
- #510 for bounded delegation contracts.

The profile adds composition rules where those contracts do not already decide behavior.

## Default pipeline

1. For a new root, apply the structural sufficiency gate and optional fallback
   recovery from `fallback-memory-recall.md`, then bind the workspace.
2. Build model-independent demand from canonical task state and selected exact
   sources. Memory and code intelligence contribute only when requested by context
   policy; they are not unconditional startup queries.
3. Resolve a deterministic per-step usage plan from task requirements, execution
   state, effective capabilities, and remaining root budget.
4. Route a concrete target that satisfies the plan's hard requirements, then
   materialize and admit the model-specific projection using `model-routing.md`.
5. Provision the selected tools and skills. Run any bounded child allowed by the
   plan through the ordinary worker runtime. Its result reenters the same admission
   path before the parent model call.
6. Execute the model turn and authorized tools. Apply retention and transactional
   compaction for later turns. Attribute helper, child, retry, and reacquisition cost
   to the task where causal evidence exists.

The system should prevent unnecessary bytes from entering model context before it tries to summarize bytes already admitted.

## Out-of-box baseline

The normal Harness composes the first-party services below. Kernel-only composition
remains explicit. Providers are bound by typed interfaces; replacing one does not
require loading its first-party implementation as a dependency.

| Concern | Default | Optional improvement / unavailable behavior |
| --- | --- | --- |
| Usage planning | deterministic per-step resource plan from task/state/budget | learned cost/success estimators are optional; no estimator is required for correct planning |
| Model selection | existing selected authenticated deployment and deterministic routing | helper targets and learned estimates are optional; missing authentication yields normal setup guidance |
| Context | deterministic assembly, exact dedupe, typed collapse, artifact references | model compaction/reducers require declared capabilities; mandatory context still must fit |
| Memory | existing per-workspace store, exact/lexical recall, provenance and freshness | embeddings, reranking and external databases are optional; no match means ordinary discovery |
| Recovery | cold gate plus at most one compatible isolated classifier call | no history, missing provider or invalid output means ordinary discovery; no guessed workspace |
| Tools | bounded common tools and authorized catalog discovery/load operations | native deferred schemas optional; host loads selected schemas on the next turn |
| Code | authorized file/search/patch operations and exact file revisions | installed language providers add semantic actions; unsupported languages stay usable |
| Delegation | available through ordinary worker contracts | automatic exploration and smaller-model routing are off until measured and configured |
| Usage/cache | record available counters and estimate provenance | missing cache/usage reporting remains unknown and disables evidence-based cache decisions |

Ordinary discovery means inspecting authorized live state and available owner
services, then asking a focused question if the missing target remains unresolved.
It does not mean scanning every repository or silently expanding memory scope.

The baseline needs no embedding service, GPU, vector database, remote analyzer,
second model credential, or network access beyond the chosen model and requested
task tools. Optional services must not perform downloads or block startup merely
because their plugin is installed. Report configured, effective, degraded, or
disabled status through ordinary inspection, with stable reason codes.

Package the default services and their artifact/reference resolver together. Product
tests must exercise fresh state, one model fixture, no optional providers, a second
session with reusable evidence, and an explicitly stripped composition.

## Ownership

| Concern | Owner |
| --- | --- |
| per-step resource intent across model requirements, reasoning, context, tools/skills, delegation, retries | usage policy |
| final model projection, context budget enforcement, retention policy, cache epochs | `phenix.context` |
| derived recall, checkpoints, cross-session learned state, revalidation | `phenix.memory` |
| exact large outputs and recoverable payloads | artifact service |
| delegated exploration and deterministic execution plans | execution/orchestration plugins |
| tool catalog, invocation, typed result projection | tool plugins |
| skill metadata and exact activation | skill/context plugins |
| current code facts | language/code-intelligence providers |
| logical code identity and cross-revision lineage | first-party code-intelligence plugin |
| target eligibility and concrete model selection | model routing |
| token, cache, latency, retry, delegation, reacquisition, and outcome accounting | model-turn history plus observability |

Core gains only generic contracts required by more than one userspace implementation. Code graphs, repository memory, compression policy, and learned pruning stay outside Core and ship through the default Harness when useful.

## Per-step usage planning

Routing answers which compatible target executes a step. Usage planning answers how much resource the step receives. Keep these decisions separate so routing does not accumulate context, reasoning, tool, delegation, retry, and recovery policy.

Semantic shape:

```rust
struct StepPlan {
    selection: ModelSelection,
    reasoning: ReasoningBudget,
    context: ContextBudget,
    tools: ToolProvision,
    skills: SkillProvision,
    delegation: DelegationPolicy,
    retry: RetryBudget,
}

trait UsagePolicy {
    fn plan(
        &self,
        task: &TaskRequirements,
        state: &ExecutionState,
        capabilities: &DeploymentCatalog,
        budget: &RemainingBudget,
    ) -> StepPlan;
}
```

Reuse canonical types where they already exist. These names define semantic roles; they do not require duplicate public enums or registries.

Rules:

- The default policy is deterministic and works with one configured model, no historical telemetry, and no cheaper helper model.
- Hard capability, authority, deadline, and root-budget constraints precede optimization.
- `StepPlan` coordinates resource intent. It does not implement routing, context admission, tool provisioning, skill resolution, worker lifecycle, retries, or authority.
- A plan cannot reserve more than the remaining root budget. Helper, child, retry, and verifier reservations compose under that same budget.
- Per-step reasoning effort is optional backend capability. If unsupported, record the target's fixed/default behavior rather than claiming the requested effort was enforced.
- Tool and skill provision starts with the smallest sufficient set when policy permits. Typed insufficiency may expand provision within the plan's bounds and a distinct attempt identity.
- Context admission may return typed exhaustion or a request for bounded replanning. It never silently drops mandatory content to satisfy the plan.
- The current unresolved turn cannot train or rerank its own plan. Historical estimators are derived inputs for later steps and turns.
- Policy state is derived and replaceable. Canonical task, context, memory, routing, and execution state remain authoritative.

Fallback recovery occurs before root execution. It receives one explicit bounded bootstrap utility plan rather than recursively invoking ordinary root `UsagePolicy`. After root admission, normal per-step planning applies.

## Context admission

Use this order before model-backed compression:

1. omit content that is not required;
2. expose descriptors before bodies;
3. reuse an exact unchanged observation instead of inserting duplicate bytes;
4. parse structured outputs into typed results;
5. move large exact payloads to durable artifacts and insert a compact view plus reference;
6. admit results from explicitly planned bounded exploration;
7. apply task-aware code pruning only to material that remains too large;
8. use model-backed compaction only after deterministic reduction.

Every lossy projection keeps an exact recovery path while the source is required by retention policy.

## Cache policy

Prompt caching changes the cost model. The smallest prompt is not always the cheapest prompt.

Context assembly uses stable cache epochs:

```text
stable prefix
  invariant instructions
  stable model-facing contracts
  stable selected tool/skill metadata

append-only epoch
  task turns
  compact observations
  exact references

next epoch
  stable prefix
  one durable checkpoint
  later turns
```

Rules:

- Keep cacheable prefix bytes and ordering deterministic.
- Put volatile material after stable material where provider semantics allow it.
- Append environment changes when semantics permit. Authority/instruction changes
  rebuild affected context immediately, even when that invalidates the cache.
- Do not regenerate a rolling summary every turn.
- Compact in batches when context pressure, reasoning quality, or expected future cost justifies a new cache epoch.
- A compaction decision accounts for provider cache-read cost, cache-write or fresh-prefill cost, expected remaining turns, context pressure, quality risk, and expected reacquisition cost.
- Provider-native prompt caching and compaction may implement the operation. Durable Phenix history and provenance remain authoritative.
- Model changes may lose provider cache reuse. Routing may include that cost as an estimate after hard eligibility filtering; cache cost never overrides capability requirements.

## Context lifecycle

Each model-visible context item has one retention state:

```text
full -> compact -> reference -> drop
```

`pinned` is an orthogonal policy marker for material that cannot be evicted while its condition holds.

Examples of pinned material include the active goal, fixed delegation constraints, unresolved blockers, and current safety or authority instructions.

A transition changes only the model projection. It does not mutate the authoritative source.

## Tools

### Discovery

The default Harness should not place every tool schema in every model request.

Expose a small stable set of common tools plus a searchable catalog. Load full schemas only for selected tools when the provider and model support deferred discovery. Providers without deferred tool loading receive the smallest task-relevant deterministic set.

Tool catalog ordering and schema serialization must be stable within a cache epoch.

The portable catalog exposes authorized discovery and schema loading as ordinary
typed tools. A discovery result is a descriptor, not invocation authority. Loading
rebuilds the next request's admitted schema set. If the backend cannot change tools
mid-session, reset/replay only when supported, or keep a bounded fixed set and report
unsupported dynamic loading. Never hide a required tool with no retrieval path.

### Results

A tool result should be structural when the producer has a reliable parser.

Preferred examples include Git status/diff metadata, compiler diagnostics, test summaries, CI checks, language-service results, package operations, and process outcomes. Raw text remains available by exact reference when needed.

Large results use:

```text
ToolObservation
  semantic result
  compact model view
  exact source or artifact reference
  content/revision identity
  invalidation metadata when known
```

Do not use a learned compressor where a deterministic parser can preserve the required semantics.

### Local pipelines

Deterministic filtering, joining, aggregation, validation, and repeated tool calls should run outside the frontier-model context when an ordinary local program can do the work. The model receives the final typed result and references to intermediate evidence when useful.

A tool may advertise a reusable observation key only when it can also define the facts that invalidate reuse. Phenix must not assume arbitrary shell commands are pure.

## Skills

`skill-discovery.md` remains authoritative. The efficiency profile requires its progressive-disclosure path in the default Harness:

1. metadata for discovery;
2. exact instruction body on activation;
3. references, assets, and scripts on demand.

Do not add a second skill summarization format.

## Delegated exploration

Use #510 delegation contracts for bounded repository exploration.

The explorer receives the task query, required authority, selected context, and relevant exact references. It does not inherit the parent transcript by default. It returns compact findings plus exact code/source references.

Exploration results may enter repository memory only through the normal memory provenance and freshness rules. A child transcript is not repository knowledge.

The default implementation should reuse ordinary Phenix workers. Do not depend on a separate explorer runtime.

Attach the typed contract to the ordinary worker task with a selected target,
attenuated authority, scoped references, deadline, attempt limit, and reserved share
of the root budget. The applicable `StepPlan` controls whether delegation is allowed
and bounds its reserved share, depth, attempts, deadline, and result size. Children,
helper calls, retries, and verification share the root budget; parallel children
cannot each spend the full remainder. Pin fixed design constraints in the child
projection. Check result size and evidence access before parent admission. Oversized
results use artifacts; invalid results escalate through ordinary worker failure
handling. Parent cancellation stops children using existing lifecycle rules. Unknown
child usage is accounted as unknown, not zero savings.

Do not infer that delegation was efficient because parent-context bytes fell. Compare child cost, parent cost, retries, and any later reacquisition against the same task outcome.

## Code intelligence

Current language facts remain owned by the existing language-intelligence contract. Add a first-party code-intelligence provider for repository-wide structure and history rather than moving code semantics into Core.

The normalized model should support:

```text
LogicalCodeEntity
  stable logical id
  kind

CodeEntityRevision
  logical id
  source revision
  current name/location/signature
  exact language/source references

CodeRelation
  source entity
  relation kind
  target entity
  source revision interval

CodeLineage
  same_entity | renamed | moved | extracted_from | inlined_into |
  split_from | merged_from | replaced_by
  evidence
  confidence where inferred
```

File path and line range are observations, not logical identity.

The first Rust implementation should consume existing language facts from rust-analyzer or SCIP where practical and use Tree-sitter only where it fills a concrete gap. Other languages may use SCIP or focused adapters. Joern may be an optional graph provider. RefactoringMiner may be an optional Java lineage provider.

The normalized Phenix identity remains stable across providers so memory and task state do not depend on one external graph engine.

Provider symbol IDs map to workspace/repository-scoped logical IDs; they are not
portable identities themselves. Record analyzer/index version and exact source
revision. Unsaved buffers and dirty worktrees need distinct revision identities.
Ambiguous rename/split/merge evidence creates tentative lineage and revalidation,
not a forced identity merge. When analyzers are absent or stale, exact file/revision
references remain usable and semantic identity guarantees are reported unavailable.

## Structured code actions

Prefer semantic reads and edits when language support is available:

```text
read entity
read callers/references/implementations
read changed neighborhood since revision
replace entity body
insert relative to entity
remove entity
```

The runtime applies syntax-preserving mechanical edits. Raw file reads and textual patches remain fallback operations.

Structured edits resolve the current entity under write authority, compare the
expected source revision, and apply through the existing transactional workspace
path. A revision mismatch returns a typed conflict before mutation. The adapter
must declare its supported language/actions; unsupported actions use explicit
textual operations rather than pretending to preserve syntax or semantics.

## Repository and task memory

Keep Phenix memory as the default memory implementation. It already owns exact provenance, typed records, hierarchical recall, supersession, and freshness. External memory databases must not become a second canonical store in the default Harness.

Extend memory references so derived records can depend on stable code entities and code relations in addition to current canonical resource references.

A repository memory claim records:

```text
claim
scope
subject refs
supporting source refs
supporting code entity/relation revisions
freshness state
```

A code move or rename does not invalidate a claim when logical identity and supporting relations survive. A changed supporting entity or relation moves affected memory through the existing freshness path. Revalidation stays incremental and deterministic first.

Task continuation uses the same memory system as a derived index over canonical execution, delegation, objective, plan, decision, CI, and artifact records. It should retain decisions, failed approaches, relevant entities, open questions, and a continuation checkpoint without copying the full transcript.

On resume, context recall prefers the delta since the last valid checkpoint plus the smallest relevant repository memory set.

## Primitive-agent export

A constrained remote agent may not have Phenix code intelligence or durable local state. The default Harness may export a budgeted continuation packet containing:

- task goal and fixed constraints;
- current decisions and unresolved blockers;
- relevant logical code entities with current locations;
- changed facts since the last checkpoint;
- selected repository memories with freshness state;
- exact references or links the remote environment can resolve.

The export is a projection. Phenix state remains authoritative.

Negotiate reference resolution with the recipient. Inline bounded required evidence
when its references are not resolvable there. If required material cannot fit, return
typed budget exhaustion. A delta identifies the exact base checkpoint/content hash;
when the recipient lacks that base, send a budgeted full packet. Imported summaries
are derived context and cannot restore authority or establish exact source facts.

## Learned reducers

Learned pruning is optional and replaceable.

- SWE-Pruner is a viable optional code-evidence pruning provider because its public implementation is permissively licensed.
- ACON and TokenPilot are useful policy references for observation/history compression and cache-aware eviction.
- A learned reducer never owns authoritative history, memory, code identity, routing, or usage policy.
- Reducer output must keep recovery references for removed exact material.

The default Harness should first use deterministic parsing, graph retrieval, exact deduplication, and cache-aware retention. Enable a learned reducer only when benchmarks show additional value.

## Reuse versus implementation

| Area | Default decision |
| --- | --- |
| per-step resource planning | implement deterministic Phenix `UsagePolicy`; learned estimators remain optional derived inputs |
| provider prompt cache and native compaction | use provider support behind provider-neutral policy |
| Agent Skills progressive disclosure | implement through existing Phenix skill/context contracts |
| cross-session memory | extend `phenix.memory`; do not add a second default memory store |
| context retention and cache epochs | implement in Phenix context policy |
| large tool output retention | use Phenix artifacts plus typed compact views |
| deterministic multi-tool processing | implement in Phenix execution/tool plugins |
| Rust semantic facts | reuse rust-analyzer/SCIP where practical |
| portable code-index interchange | support SCIP where it reduces custom indexing work |
| repository logical identity and temporal lineage | implement normalized Phenix semantics |
| Java refactoring lineage | optional RefactoringMiner adapter |
| general code property graph | optional Joern provider |
| structure-aware code read/edit | implement a Phenix contract; do not depend on the non-commercial CodeStruct release |
| isolated repository exploration | implement with ordinary #510 delegation; do not depend on withdrawn FastContext artifacts |
| task-aware neural code pruning | optional SWE-Pruner provider |
| generic external memory systems | optional import/export or experimental providers only |

## Measurement

`model-turn-protocol.md` and `observability.md` should expose enough data to distinguish, when the provider reports it:

```text
fresh input tokens
cache-read tokens
cache-write tokens
output tokens
reasoning tokens
tool-definition tokens
context bytes/tokens by category
model target and requested/effective effort
planned versus effective tool/skill provision
helper/delegated usage
retry usage
reacquisition work after omission/compaction
latency
compaction/cache-epoch transitions
task outcome
```

Do not collapse cache reads, fresh prefill, output, delegated work, retries, or reacquisition into one token number before policy decisions.

Reacquisition means later retrieval, tool, model, or reasoning work needed because useful state was previously omitted, referenced, or compacted away. Attribute it to the responsible plan/reduction where causal evidence exists. Otherwise keep it as ordinary unattributed usage rather than reporting it as free savings.

Efficiency reports use task outcome as the numerator. A policy that saves tokens but causes extra retries, repeated retrieval, or lower task success can lose overall.

## Rollout rule

Every reduction stage and adaptive planning input must be independently disableable for measurement. Compare stages and combinations on representative coding and long-horizon worker tasks.

A default-on policy must improve the success/cost frontier on representative tasks. Keep a policy optional when it only wins for a narrow workload.

Do not multiply standalone paper compression ratios to predict combined savings. Measure marginal savings after earlier stages have already removed the same bytes.

## Implementation order

1. Implement the typed model-turn and usage accounting required by `model-turn-protocol.md` and `observability.md`, including attempt/child attribution and enough evidence to record reacquisition when known.
2. Implement deterministic `UsagePolicy -> StepPlan` planning over task requirements, execution state, effective capabilities, and remaining root budget. No learned estimator is required.
3. Implement stable prompt assembly, cache epochs, deferred tool schemas, progressive skills, typed/lazy tool observations, artifact references, and exact deduplication under the plan.
4. Implement the context retention lifecycle and cache-aware batched compaction on top of the existing memory checkpoint contract.
5. Use #510 delegation for isolated exploration and selective child-context inheritance within the plan's delegation budget.
6. Add first-party repository code graph, structured reads/edits, stable logical entity identity, and temporal lineage.
7. Let memory depend on logical code entities and relations so repository knowledge survives ordinary refactors and invalidates on semantic changes.
8. Add primitive-agent export and delta resume.
9. Benchmark optional learned pruning and compression providers after deterministic stages are measurable.
10. Feed measured cost, latency, reliability, cache-loss, retry, delegation, and reacquisition estimates into `UsagePolicy` and #509 ranking without changing their consumer contracts.

## Required regressions

- deterministic usage planning works on fresh state with one configured model and no learned estimator;
- hard authority/capability/deadline/root-budget requirements cannot be weakened by `StepPlan` optimization;
- unsupported reasoning/tool/context controls are reported as degraded or unavailable rather than falsely enforced;
- append-only turns preserve a stable provider prefix until an intentional cache-epoch transition;
- deferred tools and inactive skill bodies do not enter model context;
- a large tool result can collapse to a typed view plus exact recoverable reference;
- an unchanged reusable observation can be referenced without reinserting its full bytes;
- volatile tool output is never reused without declared invalidation semantics;
- compaction keeps raw-source provenance and does not promote memory implicitly;
- cache policy can retain safe cached context instead of rewriting it solely to reduce visible token count;
- a delegated explorer receives selected context rather than the parent transcript and returns exact evidence references;
- child/helper/retry/verifier reservations cannot oversubscribe the root budget;
- a reduction that causes later retrieval/reasoning work exposes that reacquisition rather than reporting it as free savings;
- moving or renaming a code entity can preserve logical identity;
- changing a supporting code entity or relation moves dependent memory through freshness revalidation;
- primitive-agent export fits its requested budget and keeps resolvable evidence references;
- telemetry distinguishes fresh input, cache reads/writes, output, reasoning, delegated work, retries, and known reacquisition when available;
- each reduction stage and adaptive estimator can be measured independently against the same task outcome.