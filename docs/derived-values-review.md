# Derived Values Review

## Scope And Status

V1 is implemented in the current worktree; final exact-head CI is **pending**. This ledger distinguishes code/test inspection from executed verification. [Usage](derived-values.md) documents actual APIs; [design section 17](derived-values-design.md#17-implementation-status-and-deliberate-deviations) records deviations while preserving the original rationale.

The fresh design baseline `0bb81da48` contains storage docs head `86c29792f` through ancestry. Fresh formatting, Cargo tests, Clippy and Nix passed before implementation edits. The storage prerequisite is delivered by `60c4ec611` and capability-preflight fix `44f66c1d1` in [draft PR #1249](https://github.com/matthis-k/phenix-ai/pull/1249). Its exact-head [CI run 42465075375](https://github.com/matthis-k/phenix-ai/actions/runs/42465075375) completed successfully; this does not substitute for Part B CI.

## Three Passes

1. **Initial implementation.** Added authorized snapshots/singleton cells, typed graph lowering, explicit groups, deferred observation, terminal demand taps and a plugin/SQLite example. The first runtime binding approach relied on ambient build/factory registration and generation tokens.
2. **Independent review.** Found three correctness gaps: graph actions could initiate nested demand execution; temporary deployments could pollute ambient factory-registration membership; publication payload identities could be self-consistent without belonging to the exact expected endpoint. Successful happy-path arithmetic was insufficient evidence for these boundaries.
3. **Refactor and regressions.** Removed the ambient `BUILD` registry and registration/generation tokens. Added exact deployed action membership through the generic `Engine::node_ids_for_action` lookup, exact source/tap endpoint selection, current wave/cohort/count checks, and a panic-safe host-only dispatch/execution guard. Tests exercise discarded/temporary/failed/nested builds, wrong specs, ambiguous instances, spoofed endpoints, payload counts and replay.

This is a record of implementation/review passes, not a claim of three independent external approvals or completed final CI runs.

## Reviewed Boundaries

| Boundary | Current implementation and deliberate tradeoff |
|---|---|
| Storage binding | `Storage::bind`, not plugin `cx.bind`; no Kernel dependency in Storage. Domain `Amount` supplies `Stored`/`Codec`, not a primitive blanket codec. |
| Authority | `ReadSource` pins immutable session grants and resource identity; reads authorize each source independently. No grant union, live revocation or native-plugin sandbox claim. |
| Snapshots | Ordered revisioned entries, duplicates and explicit absence; Memory mount lock and SQLite transaction. Derived groups request atomic same-mount reads only; low-level best effort remains available separately. |
| Source roots | Explicit `sources`/`derive_from` supports siblings sum/diff, product both, chain and fanout. Independently created roots reject combination, even with matching leaves. |
| Terminal retention | Explicit `query_spec` adds a dedicated terminal tap rather than assuming a nonterminal exposed compute output retains its payload. |
| Runtime | `DerivedRuntime` pins Kernel lifetime and guards dispatch, delegating once to the supplied executor. It is not a new scheduler; queries use `Kernel::fire`. |
| Membership | Generic Engine pointer lookup requires one physical source and one tap and captures their exact IDs. Trigger inspection also validates the exclusive source ingress before any wave. No compiler/scheduler changes or new third-party dependencies. |
| Deployment reuse | Shared stateless source/tap `with_dyn_action` allocations permit compatible authored-module redeployment; new incompatible recomposition rejects the old spec. Multiple physical instances sharing actions reject as ambiguous. |
| Query receipt | Exact source slot 1 and tap slot 0, successful report, payload/event cardinality and current wave/cohort; identities asserted by payloads alone are not trusted. |
| Cycles | Immutable topological derived construction makes derived cycles unconstructible. An undefined-variable compile-fail doctest is not a cycle detector. General Graph loops are unchanged. |
| Value ownership | Graph inputs/outputs use `Clone + Send + Sync + 'static`; codecs in stored graph inputs require `Send + Sync`. Bound-cell cloning shares handles/codecs; value delivery/query return may clone. |

## Regression Evidence

These tests passed locally in [tests/derived.rs](../packages/phenix-storage-graph/tests/derived.rs); hosted Part B CI remains a separate gate.

| Concern | Named coverage |
|---|---|
| No construction/binding reads; one batch; native denial recovery | `one_batch_per_query_no_build_binding_reads_and_runtime_native_denial_recovers` |
| Arithmetic, start requirement, missing input, incompatible recompose | `demand_arithmetic_changes_start_guard_generation_and_recovery` |
| Siblings, chains, diamonds, arities, fanout/tap | `shared_siblings_chain_diamond_arity_and_terminal_tap_with_fanout` |
| Independent roots, foreign handles, collisions without mutation | `rejected_roots_foreign_handles_and_collisions_do_not_mutate_or_revoke_user_handles` |
| Optional absence and codec failures | `optional_and_codec_failures_are_native_wave_failures_not_result_values` |
| Temporary/discarded deployment membership | `binding_uses_final_engine_not_temporary_or_discarded_deployments` |
| Failed/nested build isolation | `failed_and_nested_builds_have_no_ambient_membership_state` |
| Ambiguous instances and deliberate reuse | `shared_actions_in_two_module_instances_are_ambiguous`; `an_authored_module_can_be_deliberately_redeployed_and_each_query_pins_its_runtime` |
| Same-runtime and cross-runtime nested queries, offload | `demand_node_cannot_recursively_query_its_own_runtime`; `demand_and_observer_nodes_cannot_query_another_runtime_even_when_offloaded` |
| Guard restoration after error/panic | `dispatch_guard_restores_after_executor_error_and_panic_and_delegates_once`; `dispatch_guard_restores_after_node_panic` |
| Replayed payloads | `earlier_terminal_publications_are_not_accepted_as_fresh` |
| Zero/multiple payloads, wrong cohorts | `zero_and_multiple_publications_never_return_an_old_result`; `terminal_multiple_cohorts_and_single_wrong_cohort_fail_validation` |
| Same-wave spoof at compatible but wrong endpoints | `same_wave_payloads_republished_at_other_compatible_endpoints_are_rejected` |
| Demand ingress preflight | `query_binding_preflights_missing_misdirected_and_multi_target_module_routes` |
| Commit between snapshot and return | `query_returns_its_snapshot_not_a_commit_after_the_source_read` |
| Independent dataset-restricted sessions | `same_mount_sources_keep_separate_dataset_session_authority` |
| Runtime and retained-spec lifetime | `backend_lives_until_runtime_and_all_saved_specs_are_dropped_in_either_order` |
| Startup, overflow, teardown, wake driver | `observer_subscribe_before_bootstrap_overflow_both_sources_and_teardown`; `observer_bootstrap_and_later_writes_wake_existing_kernel_driver` |
| Activation failures, fairness, bounded tails | `partial_subscription_activation_failure_can_retry_without_bootstrap`; `missing_subscribe_start_failure_is_retryable_and_never_bootstraps`; `independent_observers_are_fair_with_bounded_tails_and_drop_together` |

[tests/derived_plugins.rs](../packages/phenix-storage-graph/tests/derived_plugins.rs) imports the actual runnable example. It covers the canonical run, independent readers, B-only changes, demand side effects, fixed host grant denial, writer/read separation, and SQLite reopen preserving revisions. Storage's [snapshot.rs](../packages/phenix-storage/src/snapshot.rs) includes concurrent Memory and independent SQLite WAL-writer snapshot tests. Preserve existing general graph loop/conformance coverage as well.

## Honest Limits

- Subscriptions precede bootstrap, but notifications are bounded and registry-local, not a durable or cross-process changefeed. There is no coherent startup watermark/latest guarantee. Overflow means rescan, not replay every notice exactly once.
- A demand result is fresh at its source snapshot, not necessarily latest at return. Querying one endpoint runs siblings and downstream effects sharing its root.
- Optional absence has no memo/history token. No memoization, materialization, change suppression or `latest()` is shipped.
- Atomic reads cover a source group, not arbitrary cross-mount transactions or independent sequential writes. Backend atomic support can fail at read time.
- Atomic backend support is checked during snapshot construction after all schemas
  and grants, and checked again at read time. Missing or misdirected query routes
  are rejected at binding, not discovered by firing a speculative wave.
- The host-only query guard applies to nodes dispatched through `DerivedRuntime`, including delegated action execution threads; it is not process-wide tracking of arbitrary engines or detached threads.

## Verification Ledger

Complete this table only from the final exact head, after code/tests stop changing. Record failed checks and fixes rather than replacing them with baseline success.

| Check | Current evidence/status |
|---|---|
| Baseline design/storage ancestry | `0bb81da48` descends from `86c29792f`; pre-edit fmt/test/Clippy/Nix passed as reported at handoff. |
| Storage prerequisite | 66 tests passed locally; Part A exact-head CI completed successfully. |
| Derived/graph tests | 29 derived tests, 5 persistent plugin tests, 3 compile-fail derived doctests passed; existing graph conformance suites also passed. |
| Compact guide Rust example | Local `rustdoc --test --edition=2024 docs/derived-values.md` in `nix develop`, with workspace crate `--extern` arguments and `-L dependency=target/debug/deps`: 1 passed, 0 failed. This compile-tests the exact Rust block; its empty main does not exercise host setup. |
| Runnable example | `cargo run --locked -p phenix-storage-graph --example derived`: passed with `sum=9 diff=-1 both=-9 twice=18`. |
| `cargo fmt --all --check` | Passed in the repository's Nix development shell. |
| `cargo test --workspace --locked` | Passed, including existing graph conformance and kernel suites. |
| `cargo test --workspace --no-default-features --locked` | Passed. |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed. |
| `nix flake check --no-update-lock-file --print-build-logs` | Passed, including new `derived` and `no-default` checks and generated-workflow synchronization. |
| Hosted Part B CI | Awaiting the pushed implementation head; no success inferred from Part A. |

Additional explicit checks passed: `cargo test --locked -p phenix-storage`,
`cargo test --locked -p phenix-storage-graph --test derived --test derived_plugins`,
and `cargo test --locked -p phenix-kernel` (17 integration tests).
The only ignored derived doctest is the intentionally incomplete introductory
authoring sketch; the complete guide block was compiled separately and the full
plugin example is exercised by tests. No execution tests were removed or ignored.

Documentation verification on 2026-11-08: the bare shell lacked `rustdoc`. The first Nix-shell attempt selected stale dependency artifacts and failed to resolve two crates; selecting the newest built `.rlib` for each workspace crate resolved this, and the guide's single Rust test passed. These are local worktree checks, not final exact-head hosted CI evidence.
