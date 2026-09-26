# Provider cache integration

status: implementation-in-progress

Tracks #516 and the cache policy in `token-efficiency.md`.

## Gap

Merged #563 implements stable prompt assembly, projection revisions, cache epochs, compaction, and cache-usage accounting. Provider adapters currently consume cache usage when providers report it, but Phenix does not yet expose a complete provider-neutral contract for explicit cache breakpoints, cache writes, or native compaction.

## Implementation progress

- [x] Provider cache usage normalization distinguishes inclusive OpenAI-style totals from exclusive Anthropic-style counters and rejects impossible subsets.
- [x] Raw provider usage plus an explicit mapping revision is preserved for downstream evaluation/provenance.
- [x] Cache capability fields exist on effective model capabilities with unknown-by-default semantics, with regression coverage that unknown support remains optional.

## Required implementation

- [x] Add provider-neutral cache-control capabilities without making cache support mandatory.
- [x] Map provider cache controls only where the selected effective capability snapshot supports them. Context exposes an exact stable-prefix byte boundary; OpenAI Responses and Anthropic Messages encode explicit interior breakpoints, while unsupported/unknown targets degrade to no wire control unless the caller explicitly requires one.
- [x] Preserve stable prefix ordering and deterministic serialization within an epoch; context materialization now publishes a prefix identity before the volatile request suffix and binds it to the cache epoch.
- [x] Record prefix/model/provider/protocol/endpoint/configuration changes in a local compatibility identity; actual cache reads/writes remain provider-reported evidence only.
- [x] Let context policy choose retain vs compact using declared fresh/cache-read/cache-write/setup/reacquisition cost over one future-turn horizon; incomplete monetary estimates fall back only to deterministic context pressure/retention rules.
- [x] Split provider-native compaction into #611 because current opaque provider compaction items require structural continuation round-tripping before they can be safe; deterministic Phenix checkpoint compaction remains the baseline.
- [x] Keep opaque/implicit-cache providers usable with unknown cache-control semantics; compatible gateways/custom deployments stay `Unknown` rather than inheriting direct-provider assumptions.

## Acceptance

- [x] Direct OpenAI/Anthropic fixtures cover published cache capabilities, explicit/request-end wire controls, and cache read/write usage normalization; compatible gateways remain unknown unless separately proven.
- [x] Explicit cache control never changes canonical Phenix history; dispatch regression proves cache metadata leaves provider-visible context bytes unchanged.
- [x] A provider without cache control follows the same context semantics; unsupported/unknown cache support degrades wire controls while preserving the admitted context.
- [x] Cache-aware retention can prefer a larger cached prefix over a fresh rewritten prefix when the declared total future cost is lower.
- [x] Local cache compatibility changes are attributable to the context epoch/prefix or model/provider/configuration identity; provider hit/write evidence remains separate.

## Ownership

Phenix context owns policy and cache epochs. Provider adapters own wire mapping. Provider caches are accelerators, never durable state.

## Baseline and dependencies

Checked against `main` at `46aa246361a7`. Extend `EffectiveModelCapabilities`, context materialization/projection state, and adapter request mapping. `UsageQuantity` and `ModelTurnUsage` are re-exported by the SDK from Core. #592 changes schema membership through this same epoch owner. #599 consumes cache evidence; its evaluator is not required for deterministic cache policy.

## Implementation draft

1. Model cache support as independent capabilities: breakpoint control, cache-write policy, retention hints, and usage reporting. Distinguish unsupported from unknown. Keep native compaction and context-control capabilities separate. Publish limits per concrete deployment and adapter generation; model family alone is insufficient.
2. Context proposes a stable boundary using ordered item IDs/revisions. The adapter validates that boundary against its supported wire blocks and computes a diagnostic digest of the actual model-facing prefix plus target/configuration/authority identity. Exclude request IDs, timestamps, and transport headers. This digest detects local changes; it is not proof of a provider hit or the provider's cache key.
3. The context owner advances the epoch for semantic prefix changes or incompatible provider state. Compatible suffix append preserves the covered prefix. Authority revocation takes effect before dispatch. Append-only sessions must acknowledge reset/replay before a rewritten projection is considered active; rejected resets leave the old projection explicitly ineffective for the new request.
4. Each adapter maps supported controls and normalizes usage at its boundary. An inclusive input total subtracts reported cache-read and cache-write subsets; an exclusive fresh counter does not. Missing breakdowns stay unavailable unless the adapter contract establishes zero. Validate impossible totals as inconsistent telemetry. Preserve the raw usage reference and mapping version for #599.
5. Compare alternatives over the same configured future-turn horizon: remaining input cost if retained versus reducer/helper cost, rebuild/prefill/write cost, future reads, and estimated reacquisition if compacted. Record price revision, horizon, cache-hit/lifetime assumptions, and estimate provenance. Missing estimates use deterministic capacity/retention rules. Cache cost cannot relax actual context capacity, mandatory content, or explicit budgets.
6. Provider-native compaction requires lossless opaque continuation-item round-tripping and is tracked in #611. Until that contract exists, adapters do not enable native compaction; deterministic Phenix checkpoints remain authoritative and recoverable from exact sources.
7. Expose requested/effective controls, local prefix/compatibility identity, and provider-reported usage separately. Capability generation and caller authority participate in local compatibility identity. Epoch cause remains context-owned; turning cache optimization off preserves admission and durable history.

## Fixture sequence

- [x] Identical prefix plus suffix/request change preserves local prefix identity; cache epoch changes invalidate it, and transport metadata is excluded.
- Schema/authority change rebuilds the applicable projection before dispatch; append-only reset failure cannot masquerade as success.
- [x] Inclusive total 1,000 with 600 reads and 100 writes yields 300 fresh; exclusive fresh 300 with the same cache counters also yields total 1,000.
- [x] Absent cache fields remain unavailable; invalid subsets produce an accounting error without rerunning completed work.
- [x] Known-cost fixture prefers a larger cached prefix; unknown estimates use deterministic capacity/retention fallback rather than zero cost.
- Provider-native handle/block expiration and reconstruction moved to #611; deterministic Phenix compaction already retains exact recovery sources and rejects stale proposals.

## References

- [Cache policy](../token-efficiency.md#cache-policy), [compaction commit](../context-compaction.md#admission-and-commit-boundary), [backend mapping](../model-turn-protocol.md#provider-adapters).
- [Current capability and usage contracts](https://github.com/matthis-k/phenix-ai/blob/46aa246361a7bfc1ea477d08ab40c204acc89a54/rust/crates/phenix-sdk/src/contracts/usage.rs).
- [OpenAI prompt caching](https://developers.openai.com/api/docs/guides/prompt-caching) and [Claude prompt caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching) specify different input-counter conventions and deployment-specific controls. Pin fixture API/model versions instead of treating every compatible endpoint alike.
- [TokenPilot](https://arxiv.org/abs/2606.17016) is a reference for cache-aware retention evaluation, not a source for Phenix defaults.
