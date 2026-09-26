# Optional learned reducers

status: implementation-in-progress

Tracks #516 slice 11.

## Gap

Phenix implements deterministic admission, compaction, provenance, and reacquisition accounting. No learned reducer is integrated or benchmarked after those deterministic stages.

## Implementation progress

- [x] Added typed reducer request/proposal contracts pinned to projection, configuration, authority, capability generation, stage, helper reservation, and output bound.
- [x] Proposal validation rejects fabricated/duplicate/missing item decisions, missing exact recovery for omitted payloads, source-less summaries, stale projections, and underreported encoded size.
- [x] Added the provider-neutral `phenix.context-reducer@1` request/proposal interface; context imports it optionally with no persistence/canonical-state authority.
- [x] Context can request a reduction from the optional backend only for the current projection, then revalidates every returned proposal against the pinned request and current projection before exposing it; the backend still has no commit path.
- [x] Reducer stages are independently opt-in at the context owner; the default context factory enables none, and disabled stages fail before invoking an optional backend.

## Required implementation

- [x] Define the replaceable runtime reducer backend/invocation interface without canonical-state authority.
- [x] Keep exact recovery references for every removed exact payload.
- [ ] Benchmark SWE-Pruner-style code evidence reduction after deterministic graph/context filtering.
- [ ] Benchmark ACON-style history reduction separately from TokenPilot-inspired cache/layout policy.
- [ ] Route reducer model work through ordinary bounded helper invocation.
- [ ] Attribute reducer cost and later reacquisition to the same task evaluation.
- [x] Keep reducers independently disableable.
- [ ] Reject a default-on reducer unless it improves the success/cost frontier on representative tasks.

## Acceptance

- [ ] Baseline with reducer disabled remains fully functional.
- [ ] Reducer cannot mutate history, memory, code identity, routing, or usage-policy truth.
- [ ] Removed material stays exactly recoverable while retention requires it.
- [ ] Benchmarks report marginal savings after deterministic reductions, not standalone compression ratios.
- [ ] Quality regressions and retries count against the reducer.

## Ownership

Phenix owns the reducer contract and evaluation. External learned models/providers are optional implementations.

## Baseline and dependencies

Checked against `main` at `46aa246361a7`. Extend the existing context proposal/commit mechanism and ordinary helper-attempt accounting. #592 supplies exact observation capture; #591 owns cache/layout policy; #599 must supply evaluation before default enablement. #594 can narrow code evidence when available, but a code graph is optional. Evaluation must also work with file/search baselines.

## Implementation draft

1. Define an optional reducer request pinned to projection, source/configuration/authority revisions, selected backend capability generation, task query, eligible item IDs/ranges, exact recovery references, and reserved helper/output bounds. Only reducible content enters the request. Mandatory instructions, active request, fixed constraints, and unresolved call/result groups stay outside its mutation scope.
2. Return a proposal with typed retained selections, omitted coverage, derived summaries with exact source refs, and helper usage. Extraction must select exact authorized ranges; a summary is labeled derived. Validate membership, bounds, mandatory coverage, group integrity, output size, and exact recovery availability. Confidence is diagnostic, never evidence of correctness. Resolve optional source expansion under the same bounded authority.
3. Context rechecks the pinned revisions and commits through its existing compaction/projection transaction. Steering, cancellation, source changes, and authority changes discard stale proposals. Provider reset/replay must be acknowledged before claiming a rewritten context took effect. The reducer has no canonical-state mutation capability.
4. Run every reducer invocation, including local learned models, as an attributed bounded helper. Reserve before invocation and account failure/timeout/unknown usage. On rejection or failure, preserve the detailed sources, recompute deterministic admission against the remaining budget, and continue only if it fits. Otherwise return ordinary bounded replan/exhaustion. Never promise a free or byte-identical fallback after a helper consumed resources.
5. Keep code-selection, observation-summary, and history-summary stages independently disableable. SWE-Pruner informs task-conditioned code selection; ACON informs observation/history compression. TokenPilot is a cache/layout-policy reference evaluated through #591, not assumed to be another summarizer model. Pin external code/model revisions and verify their licenses before packaging an adapter.
6. Record reduction identity, pinned inputs, admitted outputs, exact recovery refs, helper attempt, and affected epoch. Later recovery may reference that reduction identity; do not infer a causal saving from byte similarity. Failed candidates still count in task cost.
7. Compare each stage after the same deterministic baseline using #599's complete-task metrics. Default enablement requires the preregistered success threshold and paired cost evidence on a held-out suite. Missing provider/dependency leaves the stage disabled. No automatic model downloads or optional service startup are required for baseline use.

## Fixture sequence

- Disabled reducer matches deterministic admission at identical source/configuration/budget inputs.
- Failed reducer charges helper work; remaining budget may produce exhaustion while exact sources survive.
- Fabricated IDs/ranges, omitted mandatory groups, inaccessible refs, and oversized output reject the proposal.
- Steering/revocation between preparation and commit makes the proposal stale.
- A local learned model and remote helper both appear in task accounting with honest coverage.
- Compare deterministic-only, each reducer alone, combined reducers, and cache policy separately; include failures and reacquisition.

## References

- [Learned reducers](../token-efficiency.md#learned-reducers), [compaction commit](../context-compaction.md#admission-and-commit-boundary), [rollout rule](../token-efficiency.md#rollout-rule), [#599 evaluation](https://github.com/matthis-k/phenix-ai/pull/599).
- [Existing projection revision and proposal state](https://github.com/matthis-k/phenix-ai/blob/46aa246361a7bfc1ea477d08ab40c204acc89a54/rust/crates/phenix-plugin-context/src/projection_state.rs).
- Primary references: [SWE-Pruner paper](https://arxiv.org/abs/2601.16746) and [implementation](https://github.com/Ayanami1314/swe-pruner), [ACON paper](https://arxiv.org/abs/2510.00615) and [implementation](https://github.com/microsoft/acon), [TokenPilot](https://arxiv.org/abs/2606.17016). These are adapter/policy research inputs; their standalone compression ratios are not combined Phenix performance estimates.
