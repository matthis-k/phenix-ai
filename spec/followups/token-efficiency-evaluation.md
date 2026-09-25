# Success-normalized efficiency evaluation

status: implementation-in-progress

Tracks #516 slices 1 and 10 plus the rollout rule in `token-efficiency.md`.

## Gap

Merged runtime work records typed usage, cache counters where available, attempts, retries, budgets, routing evidence, reacquisition, and outcomes. There is no single evaluation/reporting layer that compares efficiency policies by successful task work instead of raw token reduction.

## Implementation progress

- [x] Added task/cohort evaluation contracts with terminal outcome classes, complete-cost coverage, success-normalized cost, and explicit unresolved work.
- [x] Cohort reports preserve fresh/cache-read/cache-write/output/reasoning/reacquisition usage categories and reject mixed policy/evaluator/price cohorts.
- [x] Added paired-policy comparison that rejects mismatched task fixture sets before comparing reports.
- [x] Added typed task derivation from distinct charged attempt records, rejecting duplicate, cross-root, and cross-policy charges while including helper/delegated work exactly once.

## Required implementation

- [ ] Build derived efficiency records from existing durable attempt, usage, routing, context, delegation, and outcome facts. (Attempt-level derivation and de-duplication are implemented; durable source collection/outcome-evidence integration remains.)
- [x] Keep fresh input, cache reads, cache writes, output, reasoning, retries, reacquisition, latency, and cost separate in the evaluation contract/report. (Helper/delegated source-record construction remains.)
- [x] Define task-level success/outcome evidence without creating a second canonical task state; derived task records require a matching evaluator/source evidence identity and revision.
- [x] Compare policy variants only after validating the same task fixture set.
- [x] Report marginal effect of independently disableable reduction stages through explicit baseline/variant stage sets and paired cohort comparisons.
- [ ] Feed historical estimates into routing/UsagePolicy only as derived evidence for later attempts.
- [x] Keep current-turn outcome out of its own planning/routing decision; step-runner records routing evidence only after dispatch completes/fails, and derived estimates are rebuilt afterward for later selections.
- [x] Expose unknown/unavailable usage and incomplete monetary accounting explicitly rather than treating them as zero.

## Acceptance

- [x] A policy with cheaper successful work but more expensive failures can score worse in success-normalized cost.
- [x] Cached and fresh input remain distinct in cohort reports.
- [x] Delegated/helper work appears in total task cost when supplied as distinct charged attempt records; duplicate/cross-root charges are rejected.
- [x] Reacquisition caused by prior reduction is attributable when causal evidence exists; evaluator records preserve `cause_identity` and originating attempt instead of only totals.
- [x] Evaluation can compare baseline vs one optimization at a time and combined profiles; variant-set reports preserve active/added/removed stage identities.
- [x] Derived routing estimates can be deleted/rebuilt without losing evidence: the estimate map is omitted from durable routing snapshots and deterministically rebuilt from durable completed evidence on restore.

## Ownership

Execution/model/context owners remain authoritative for facts. The evaluator is derived analytics and optional estimator input, not a new runtime coordinator.

## Baseline and dependencies

Checked against `main` at `46aa246361a7`. Reuse `UsageAttribution`, `AttemptUsageRecord`, `UsageAggregate`, and canonical root outcomes. `ModelTurnUsage`/`UsageQuantity` are shared Core types re-exported by the SDK. The evaluator can land before #591-#598 using deterministic fixtures; those PRs add evidence producers and later benchmark variants. Missing events or outcomes make coverage incomplete, not invented.

## Implementation draft

1. Build derived evaluation records keyed by task fixture/revision, root execution, policy/configuration revision, attempt ID, route/capability generation, and outcome-evidence identity. Deduplicate event replay by canonical record/attempt identity. Terminal task success requires its declared evaluator evidence, such as accepted tests; a successful model/tool invocation alone is insufficient. Preserve failed, cancelled, timed-out, and unresolved outcomes separately.
2. Keep provider raw usage and versioned normalization. Partition fresh/cache-read/cache-write input once. Preserve whether reasoning is a subset of output or already disjoint; cost adapters use the matching convention exactly once. Cumulative streaming updates replace earlier snapshots instead of summing them. Missing categories stay unavailable and inconsistent counters stay flagged.
3. Aggregate the distinct charged attempt records across root, retries, helpers, children, and verification. A parent's rolled-up child counters are summaries, not additional charges. Reacquisition is a label on actual work, never an amount added again to the task total. When known, link the consumer attempt/tool operation and source reduction/checkpoint; otherwise retain unattributed work. Separate provider monetary charges from local compute estimates.
4. Define the comparison cohort as terminal tasks with the same outcome evaluator and cost unit. Include all work under successful, failed, cancelled, and timed-out roots in the total. Report `cost_per_success = total_cost_of_terminal_cohort / successful_tasks`, success rate, outcome coverage, and price/accounting coverage. With zero successes, report undefined/no-success rather than zero. Keep unresolved tasks and their known spend visible separately; block a rollout comparison until they resolve or a declared timeout/censoring rule applies. Pin price revisions and separate measured provider charges from estimated local compute. If any required cost is unknown, report known subtotal plus missing coverage; do not rank that subtotal as a full cost.
5. Record root elapsed wall time separately from summed model/tool/child durations; parallel work must not be double-counted as elapsed latency. Report successes' latency distribution alongside failure/cancellation durations and completion rate. Publish per-category token counts and per-stage deltas as diagnostics without collapsing away cache/reasoning provenance.
6. Compare paired variants on the same frozen task/repo/dependency snapshots, outcome evaluator, budget/deadline, model/adapter revisions, and declared warm/cold cache protocol. Include repeated runs when inference is stochastic, task-level paired uncertainty, and failure counts. Run deterministic normalization/report fixtures in CI; real-model benchmarks require a separately recorded run, not a CI truth assertion.
7. Preregister the allowed success-rate degradation and cost-improvement criterion in benchmark configuration before seeing outcomes. Use held-out tasks for promotion. One-stage ablations and combined profiles measure marginal effects; their ratios cannot be multiplied.
8. Build historical estimates only from completed evidence at a recorded cutoff. Task-level quality waits for terminal task outcome; per-attempt cost may use earlier finalized attempts. Pin the estimator snapshot in each plan. An unresolved attempt's own later evidence cannot retroactively affect its route or resource choice. Derived records/indexes can be rebuilt without altering source events.

## Fixture sequence

- Policy A succeeds on 8/10 tasks, costing 80 units on successes plus 20 on failures: 12.5 units per success. Policy B succeeds on 8/10 with 64 plus 56 units: 15 per success. B loses despite cheaper successes.
- Root/child summaries and cumulative usage updates do not duplicate billed attempts; reasoning subsets and reacquisition labels do not add charges twice.
- Zero successes, unresolved outcome, missing price, and unknown child cost never become zero-cost wins; an unresolved task with known spend remains visible but cannot silently leave a rollout cohort.
- Two parallel 5-second child calls do not imply a 10-second root elapsed duration.
- A late event rebuilds the report and preserves the original estimator cutoff and plan.
- Paired baseline and optimization runs share task snapshots but isolate declared cache state; an optimization cannot select an easier task subset.

## References

- [Measurement and outcome objective](../token-efficiency.md#measurement), [rollout](../token-efficiency.md#rollout-rule), [usage normalization](../model-turn-protocol.md#usage-and-retries), [observability](../observability.md#metrics), [routing estimates](../model-routing.md#adaptive-estimates).
- [Existing attribution, usage records, and aggregates](https://github.com/matthis-k/phenix-ai/blob/46aa246361a7bfc1ea477d08ab40c204acc89a54/rust/crates/phenix-sdk/src/contracts/usage.rs).
- [SWE-bench evaluation guide](https://github.com/SWE-bench/SWE-bench/blob/main/docs/guides/evaluation.md) supplies a reproducible patch/test outcome model. Phenix additionally needs long-horizon and interruption/reacquisition tasks.
