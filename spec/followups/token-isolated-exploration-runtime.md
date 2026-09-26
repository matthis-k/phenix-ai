# Isolated exploration runtime

status: implementation-in-progress

Tracks #516 slice 5.

## Gap

`ExplorationPolicy` and bounded delegation contracts exist, and #563 implements ordinary delegated-task budgets/lifecycle. No runtime currently turns an exploration opportunity into an ordinary delegated child and re-admits compact findings to the parent.

## Implementation progress

- [x] Finite root cost caps reject unknown reservation cost instead of allowing an unbounded reservation.
- [x] Unknown actual cost retains the conservative reserved charge.
- [x] Delegated result validation measures the actual serialized result and cannot be bypassed by an underreported byte count.
- [x] Accepted exploration decisions now project into the ordinary atomic delegated-admission contract, with deterministic task/reservation identity and pre-admission authority/deadline checks.
- [x] Planning exposes a typed handoff that packages an explicit exploration candidate plus inline/delegated cost estimates into an `ExplorationOpportunity` without scheduling work.
- [x] Pending delegated work can be cancelled before start while atomically releasing its unused reservation; started work cannot take this rollback path.
- [x] Durably admitted delegated tasks expose a read-only runnable projection for scheduler/recovery discovery; starting a task removes it from that projection without creating a second task store.
- [x] Completed delegated results have a context-owned readmission operation that loads the exact result once, re-prepares current parent context, and commits through ordinary context admission.
- [x] Delegated task bindings persist optional originating-attempt identity so later child/retry/reacquisition accounting has a stable parent attribution key.
- [x] The delegated worker allocates its first charged attempt with that originating attempt as the parent and the durable task ID; retries inherit the same task identity and remain under the delegated reservation lineage.
- [x] Step attempts expose replay-idempotent durable reacquisition accounting keyed by a unique occurrence ID, with optional same-root source-attempt linkage. Delegated-result admission records the parent-context reacquisition against the originating attempt and links it to the successful delegated attempt; replay keeps one receipt.
- [x] Delegated retries remain inside the delegated task lineage: retry eligibility includes the initial delegated attempt, retry validation stops at the delegated/root boundary, and allocated retries inherit the task identity/reservation lineage.
- [x] Removed the dead delegated-task variants from the generic execution API; the execution-resource service is the single durable owner of delegated task admission/lifecycle state.

## Required implementation

- [x] Add the planner/context handoff that creates an `ExplorationOpportunity`.
- [x] Evaluate `ExplorationPolicy` only after task separability and parent-context cost are known. `AssessExplorationOpportunity` first constructs the complete typed opportunity from the explicit separability/transcript candidate and inline/delegated cost estimate, then calls `ExplorationPolicy::assess`; the planning regression locks that ordering.
- [x] Create the child through the existing delegation/worker path. (`phenix.delegated-worker@1` consumes the durable runnable projection, creates/reuses the attenuated child execution, runs the pinned route through the ordinary step runner, settles the delegated reservation, and re-admits completed findings through context.)
- [x] Give the child selected exact references and attenuated authority, not the parent transcript.
- [x] Return bounded typed findings plus exact evidence references.
- [x] Re-admit findings through ordinary context admission. (`ContextCommand::AdmitDelegatedResult` owns exact result injection and current-parent re-admission; retries re-admit current context without duplicating the delegated-result injection.)
- [x] Attribute child cost, retries, and later parent reacquisition to the originating attempt. (The delegated attempt is parented by the originating attempt, retries preserve the delegated task lineage, and parent result admission records a replay-idempotent reacquisition receipt with the successful child attempt as its source.)
- [x] Keep automatic exploration disabled until benchmarked.

## Acceptance

- [x] Context pressure alone cannot spawn a child.
- [x] A non-separable task stays in the parent.
- [x] A delegated explorer has no implicit parent-transcript access.
- [x] Child results cannot exceed the reserved result bound.
- [ ] Benchmarking compares total task work against the inline-parent baseline.

## Ownership

Execution owns child lifecycle and budget. Delegation owns constraints. Context owns what reenters the parent. There is no special explorer runtime.

## Baseline and dependencies

Checked against `main` at `46aa246361a7`. Reuse `ExplorationPolicy::assess`, `DelegationTaskBinding`, `DelegatedWorkResources`, `DelegatedWorkerResult`, and `ExecutionResourceCommand::AdmitDelegated`. The latter already reserves budget and creates the task atomically. A separate `Reserve` before it would double-reserve. #592 supplies bounded artifact views; #599 is the rollout gate for automatic exploration.

## Implementation draft

1. Planning produces an opportunity with a separable question, pinned constraints, selected exact evidence, and estimation provenance. Keep default automatic exploration disabled. The existing `ExplorationPolicy::assess` counts only parent-input savings and assigns `expected_parent_reacquisition_tokens` to the inline path. The opportunity now carries separate inline and delegated parent-reacquisition estimates; keep both in the feasibility comparison. Keep `assess` as a feasibility gate: before automatic spawn, compare both alternatives including child/helper work, expected quality/retries, and parent reacquisition under one cost basis. Unknown decision-critical estimates keep the work in the parent. Explicit bounded delegation remains available.
2. Intersect the opportunity with the parent's `StepPlan` delegation, depth, child count, attempts, deadline, and result limits. Attenuate authority before resolving selected references. Materialize fixed constraints and the immutable contract corresponding to `contract_fingerprint`; a fingerprint alone is not an executable child instruction.
3. Enforce monetary limits in the shared `RootBudgetLedger`, not only in `ExplorationPolicy`: its current `validate_fits` accepts `cost_microunits: None` when a finite root limit exists. Under a hard cap, require a conservative bounded reservation derived from supported provider controls, price and token limits; reject when one cannot be established. Settlement with missing actual cost charges the reserved bound, as the current ledger already does when a bound exists. Apply this rule to root/helper/retry/verifier reservations too. A post-dispatch overrun is an explicit breach, never a retroactive enforcement claim.
4. Submit one `AdmitDelegated` request with the reservation and task. Use the enclosing reservation ID for nested budgets. Persist an opportunity/parent-attempt identity so recovery looks up the same task before scheduling. Reusing that identity with changed contract/resources is a conflict. No second task or reservation is created after a crash.
5. Schedule the ordinary worker only after durable admission. Before start, a confirmed schedule failure cancels the task and releases its unused reservation atomically through `CancelDelegatedBeforeStart`. After dispatch or an uncertain outcome, settle known use or retain conservative reservation charges. Never release spent/unknown usage as zero.
6. The child receives selected scoped refs and pinned constraints, with no implicit parent transcript. Parent cancellation uses ordinary worker teardown. Record terminal state and reconcile budget even for cancellation, deadline, malformed output, and escalation.
7. Bound the encoded result at the receiving boundary, including findings and refs. Compute actual bytes instead of trusting `encoded_result_bytes`. Persist oversized raw evidence before producing a bounded envelope; reject an envelope that still exceeds the limit. Use existing escalation/outcome types. A summary does not make inaccessible evidence admissible.
8. Validate source revisions and authority under the current parent projection. Historical evidence can remain historical; it cannot be presented as current. Admit findings once using the opportunity/result identity. Stale parent context requires re-admission, not respawning the explorer.
9. Evaluate parent, child, helper, retry, and reacquisition work together before automatic rollout. Explicit delegation remains available under the selected model without requiring a cheaper deployment.

## Fixture sequence

- Disabled/non-separable/transcript-dependent opportunities create no child.
- A reservation with unknown cost is rejected under a finite root cap for root/helper/child work; a known bound with unknown actual cost charges the reserved amount. No cap remains usable.
- Equal parent-token savings with a costly child or extra delegated reacquisition stays in the parent; unknown critical estimates never trigger automatic exploration.
- Concurrent children and nested reservations cannot overspend; recovery after admission schedules the same task once.
- Failure before start releases only unused resources; uncertain dispatch preserves accounting.
- Underreported result-byte field is rejected using actual encoded size.
- Parent cancellation ends the child; a late result cannot mutate cancelled parent context.
- Historical/inaccessible refs remain labeled or rejected; duplicate completion cannot duplicate parent findings.

## References

- [Delegation policy](../token-efficiency.md#delegated-exploration), [usage planning](../token-efficiency.md#per-step-usage-planning), [#510 design](https://github.com/matthis-k/phenix-ai/pull/510).
- [Existing assessment](https://github.com/matthis-k/phenix-ai/blob/46aa246361a7bfc1ea477d08ab40c204acc89a54/rust/crates/phenix-sdk/src/contracts/exploration.rs), [Existing worker binding/result](https://github.com/matthis-k/phenix-ai/blob/46aa246361a7bfc1ea477d08ab40c204acc89a54/rust/crates/phenix-sdk/src/contracts/delegation.rs), [Atomic task admission and settlement](https://github.com/matthis-k/phenix-ai/blob/46aa246361a7bfc1ea477d08ab40c204acc89a54/rust/crates/phenix-plugin-execution/src/resource_transaction.rs), [root ledger cost checks](https://github.com/matthis-k/phenix-ai/blob/46aa246361a7bfc1ea477d08ab40c204acc89a54/rust/crates/phenix-sdk/src/contracts/budget.rs).
