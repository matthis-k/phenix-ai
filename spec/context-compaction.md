# Execution context budgeting and compaction

status: specification-only

## Purpose

Keep each execution within the resolved model's actual context capacity without sacrificing durable semantics or exact provenance.

`phenix.context` owns execution context budgeting and final projection. In the normal Phenix suite, reversible model-backed compaction is provided by `phenix.memory` through `context.compact@1`, with progressive expansion through `context.expand@1` as defined by `spec/plugin-memory.md`.

`token-efficiency.md` defines the separate per-step `UsagePolicy -> StepPlan` contract. A plan may request a context budget or retention preference. Context remains authoritative for mandatory-content protection, admissible reduction, exact recovery references, and typed exhaustion.

## Order of operations

1. avoid unnecessary injection;
2. honor an explicit bounded delegation plan when semantically appropriate;
3. deterministic pruning;
4. structured result collapse;
5. reuse valid compact memory/checkpoint nodes;
6. model-backed compaction only when still required.

## Contract

- Context budgeting is per execution, not session-global.
- Capacity is derived from the resolved model and accounts for tool schemas, output reserve, and safety margin.
- Category budgets are dynamic rather than fixed quotas.
- Category demand is derived from the canonical execution context projection/accounting; callers do not provide independent category demand or quota state.
- A `StepPlan` may bound optional context and request a retention preference. It cannot authorize dropping mandatory content or bypass context authority/provenance rules.
- If a plan cannot fit after permitted reduction, context returns typed exhaustion or bounded replan input. It does not silently widen tools, change target selection, increase root budget, or discard required material.
- Model-backed compaction uses a typed `context.compact@1` provider.
- The normal Phenix provider is `phenix.memory`; another compatible provider may replace it through ordinary plugin composition.
- Memory-backed summarization uses the `memory.summarize` callable through the execution's pinned model routing profile rather than a second compactor-specific router.
- Each compaction request carries its memory scope; checkpoints and summary nodes remain inside that scope.
- Compaction and budget policy come from the immutable configuration revision pinned to the execution.
- The compactor has zero authority to mutate objectives, plans, decisions, execution authority/state, workspace observations, artifacts, or raw session/source history.
- Compaction creates a durable `ContextCheckpoint` containing a compact summary node, exact covered history/source ranges, retained exact refs, and generation/configuration metadata.
- Later compaction may consume a prior checkpoint as an optimization, but provenance continues to resolve to raw durable source ranges rather than summary-only ancestry.
- `context.expand@1` may progressively rehydrate a checkpoint into child summaries, events, or exact raw sources when more detail is needed.
- Context compaction does not imply long-term memory promotion. Promotion is a separate memory state transition.
- A confirmed provider rejection for context overflow permits one emergency
  prune/compact retry on that target. It shares the root budget and never reruns
  completed tool effects. Unknown dispatch outcomes follow `model-turn-protocol.md`.
- Context pressure may inform planning but never auto-spawns child executions solely due to a token threshold.
- Compaction savings and later state reacquisition are accounted separately. A reduction that forces repeated expansion/retrieval must not appear efficient merely because the immediate prompt shrank.

## Failure semantics

A failed compaction never replaces or discards the detailed active context it was asked to compact. The caller may prune further, request bounded replanning through the usage-policy path, choose another compatible provider/route according to pinned policy, or fail with explicit context exhaustion.

Failure to expand a compact node is visible and never causes the node's summary to be presented as exact source evidence.

## Admission and commit boundary

Compute the mandatory floor before optional admission. It includes current
instructions, the active request and fixed constraints, selected schemas, required
tool-call/result groups, output reserve, and safety margin. If the floor cannot fit,
use the bounded routing rule in `model-routing.md` or return context exhaustion.
Truncating pinned text is not a valid fallback.

Keep tool-call/result groups structurally complete. A pending call and its identity
remain pinned until resolved. Deduplication may share payload storage but preserves
each call occurrence and its result binding.

Prepare compaction against an immutable projection/source/configuration revision.
Commit the new checkpoint and active projection reference atomically only if that
revision is still current. Cancellation, steering, authority changes, or concurrent
context updates discard the stale proposal. Preserve the detailed projection on
failure and reassemble using current authorized state.

Persist exact artifacts before replacing their bytes with references. Retention
keeps referenced sources reachable while an active execution or retained checkpoint
requires expansion. Permission revocation still applies at resolution time. Missing
or revoked sources produce typed unavailable evidence, never fabricated expansion.

The portable baseline works without model-backed compaction: deterministic omission,
typed collapse, exact references, and bounded exhaustion handling remain available.
Use the helper-call rules in `model-routing.md` for summarization. Default: one
summarization call per compaction attempt, no recursive compaction of that call.
The helper consumes a reserved share of the root/step budget and records its own
attempt/usage identity. Slice 11 learned reducers remain optional.

Backend context-control modes from `model-turn-protocol.md` govern how a committed
projection becomes effective. An append-only or opaque session cannot claim an
old context item was removed without an acknowledged replacement/reset operation.

## Reacquisition accounting

Expansion or fresh retrieval after earlier omission/compaction is ordinary work. When the caller can identify the causal checkpoint/reduction, record that relationship for observability and later policy estimation. Context does not need to infer causality from content similarity.

Do not charge exact source access twice merely because a compact view exists. Record the actual model/tool/retrieval work performed. Unknown causal attribution remains unknown rather than being counted as zero.

## Invariants

1. Context management is per execution.
2. Compaction cannot mutate canonical durable semantics.
3. Checkpoints retain exact provenance to raw durable sources.
4. Deterministic pruning precedes model-backed compaction.
5. Model route/capacity changes recalculate budget from the resolved target.
6. Overflow recovery is explicit and inspectable.
7. Compaction policy and target selection use the execution's pinned configuration revision.
8. Compaction is reversible while its durable sources remain valid.
9. Repeated compaction never degrades provenance into summary-only ancestry.
10. Compaction and long-term memory promotion are separate operations.
11. `StepPlan` resource intent cannot weaken mandatory context, authority, or provenance requirements.
12. Immediate token savings and later reacquisition cost remain distinguishable.

## Non-goals

- decisions/history retrieval implementation;
- usage-policy or model-routing implementation;
- worker scheduling policy;
- semantic plan mutation;
- defining long-term memory extraction/consolidation policy outside `spec/plugin-memory.md`.