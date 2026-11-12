# Graph Removal Regression Ledger

M6 removes the old implementation, not just its exports. This ledger inventories
the retired suites and names executable native replacements. Coverage is grouped
by capability, not claimed to be a one-to-one port of every old assertion.
**Status: removal and native capability migration implemented.** This is grouped
regression evidence, not one-to-one equivalence of every historical assertion.

## Native Coverage Keys

Every name below is an exact test name. Paths are relative to
`packages/phenix-graphs/tests/` unless specified otherwise.

| Key | Capability | Native executable coverage |
| --- | --- | --- |
| T | Local and portable identity, positive and negative schemas | `fbp_types::local_types_with_identical_shape_are_not_interchangeable`, `independently_declared_portable_rust_types_connect_by_complete_schema`, `portable_mismatches_in_each_schema_field_reject_dynamic_wiring`, `portable_declarations_and_typed_lookup_do_not_trust_overridden_wire_labels`, `exact_interface_equality_includes_extra_inputs_outputs_and_types` |
| B | Builder scopes, revisions, replacement, independent definitions | `fbp_components::builder_scope_is_checked_for_connect_export_start_and_replacement`, `immutable_definition_branches_have_independent_authoring_revisions`, `replacement_in_both_body_directions_preserves_edges_and_exports_and_revokes_handles`, `failed_replacement_is_atomic_and_does_not_revoke_current_handles` |
| P | Public projections, aliases, flattening | `fbp_components::public_input_fanout_and_projection_aliases_preserve_capacities`, `conflicting_projections_reject_without_overwriting_the_first_declaration`, `physical_alias_collisions_are_rejected_after_projection_expansion`, `nested_repeated_modules_flatten_to_only_their_physical_processes`; `fbp_streams::capture_aliases_are_one_atomic_fanout` |
| C | Claims, definitions, binding, final requirements | `fbp_contracts::definition_equality_is_exact_and_shape_never_infers_semantics`, `restricted_builder_binding_uses_actual_scope_revision_and_ports`, `compiled_binding_restricts_ports_types_and_generation`, `all_nested_claims_and_registered_definitions_share_one_namespace`, `replacement_preserves_claims_and_removed_private_requirements`, `frozen_base_obligations_survive_composer_replacement_and_rollback` |
| A | Exact edit authority and provenance | `fbp_authority::grants_are_exact_not_prefixes_or_origin_ancestry`, `component_grant_never_authorizes_connection_export_or_source_writes`, `nested_payload_cannot_hide_a_forged_origin`, `native_claim_and_binding_never_grant_composition_writes`, `native_inspection_preserves_exact_root_declared_and_inherited_provenance` |
| M | Composition, priorities, replay, claims | `fbp_composition::plugin_a_add_b_extend_c_replace_opposite_body_and_revoke`, `independent_registrations_commute_even_when_admitted_separately`, `explicit_priority_defeats_prerequisite_and_all_transitive_dependents`, `defeated_intermediate_cannot_suppress_an_independent_lower_priority_patch`, `missing_and_cyclic_dependencies_are_atomic`, `final_compile_failure_discards_entire_candidate_and_registry`, `introduced_add_and_replace_claims_cannot_be_stripped_by_later_registration`, `replacement_and_unrelated_rewire_do_not_conflict_or_touch_incidents` |
| S | Splices and precise edge writes | `fbp_composition::splice_chains_keep_original_anchor_capacity_and_dependency_order`, `independent_splices_are_ambiguous_even_with_priorities`, `generated_splice_edges_conflict_with_explicit_writes`, `splice_generated_edges_require_separate_explicit_grants` |
| X | Atomic edits and escaped handles | `fbp_transactions::errors_and_panics_restore_all_topology_and_revoke_escaped_handles`, `nested_commits_are_revoked_by_outer_failure_and_inner_failure_is_local`, `foreign_handles_cannot_author_edges_or_exports_in_a_transaction`, `failed_derived_like_export_can_retry_same_name_without_partial_edits` |
| F | FIFO, fanout, preparation, type safety | `fbp_connections::fanout_waits_atomically_when_one_recipient_is_full`, `blocked_retries_never_repeat_preparation_callbacks`, `preparation_error_and_panic_cannot_partially_deliver`, `duplicate_and_invalid_selection_reject_before_any_preparation`, `adapter_output_identity_is_validated_before_any_mutation`, `empty_selection_is_a_successful_publication_without_delivery`, `randomized_fanout_and_fifo_operations_match_independent_bounded_reference` |
| Q | Native per-emission selection of existing downstream connections | `fbp_routing::default_broadcast_and_explicit_one_many_or_empty_preserve_captures`, `explicit_empty_selection_is_intentional_discard_without_capture_or_outgoing_edges`, `unknown_duplicate_foreign_and_unconnected_selections_deliver_nothing_atomically`, `selection_cannot_borrow_an_edge_from_another_output`, `erased_paths_resolve_only_existing_edges_and_never_grant_injection_authority`, `nested_repeated_module_aliases_select_their_flattened_fanout_without_wrapper_execution`, `alias_overlap_and_partially_connected_alias_fanout_reject_before_any_delivery`, `oversized_selection_rejects_before_resolution_and_captures_do_not_expand_the_budget`, `capacity_one_retry_retains_selected_fanout_and_never_repeats_action_effects`, `an_unselected_full_connection_does_not_block_selected_delivery`, `committed_progress_within_one_activation_survives_a_later_invalid_selection` |
| O | Owned work, delayed tickets, no double execution | `fbp_owned_steps::different_processes_can_complete_out_of_dispatch_order`, `foreign_completion_is_returned_intact_and_does_not_replace_state`, `rejected_dispatch_keeps_reservation_and_process_and_renews_step_ticket`, `capacity_one_stall_and_resume_preserve_effect_and_activation_exactly_once`; `fbp_streams::delayed_completion_wrong_owner_and_cancellation` |
| R | Correlated and staged policies, absence | `fbp_streams::all_join_keeps_requests_separate_and_never_skips_a_fifo_head`, `custom_initial_policy_and_staged_reservation_keep_request_activation`, `at_least_policy_requires_distinct_ports_and_reserves_exactly_n`, `scoped_optional_uses_real_upstream_work_and_retires_absence_between_requests`, `scoped_staged_closure_preserves_activation_and_does_not_leak_history`, `optional_scope_cannot_settle_behind_another_requests_handed_job` |
| L | Limits, cycles, lifecycle, cancellation | `fbp_components::compile_limits_bound_depth_instances_including_empty_modules_and_work`, `compile_work_limit_accounts_for_expanded_connection_fanout`; `fbp_cycles::self_cycle_is_bounded_and_reports_capture_backpressure`, `full_cycle_has_real_queue_dependencies_not_a_progress_guarantee`; `fbp_streams::budget_exhaustion_is_resumable_and_explicit_cancellation_allows_followup_request`, `queued_request_cancellation_preserves_other_requests_fifo_and_process_state`, `session_lease_outlives_capture_and_late_completion_payloads`; `fbp_owned_steps::late_emission_payload_drops_before_its_job_releases_code_lease` |
| E | Explicit sources, sinks, multiple outputs | `fbp_components::zero_input_zero_output_sources_sinks_and_multioutput_are_native`, `explicit_sources_start_only_selected_roots_even_in_sink_only_composites`; `fbp_streams::split_join_multiple_outputs_and_zero_output_sink`, `explicit_source_wakes_later_and_local_type_validation` |
| V | Resource authority and local ownership | `fbp_streams::resources_reference_existing_store_and_require_explicit_per_process_grants`; `fbp_connections::local_move_only_fanout_shares_arcs_and_preserves_exact_origin`, `local_value_and_typed_clones_drop_payload_before_all_leases`; `integration_regressions::store_preserves_typed_replacement_and_grants_do_not_follow_claims` |
| W | Independent compilation and codec conformance | `integration_regressions::independently_compiled_portable_crates_execute`, `typed_connections_and_required_schema_are_compile_checked`, `malformed_wire_payload_fails_without_poisoning_an_independent_session`; retained `portable_codec::decoder_member_names_cannot_shadow_its_state`, `conformance_checks_complete_descriptors_and_both_codec_directions` |
| D | New typed-port derive | `fbp_port_keys::derived_interfaces_are_exactly_the_manual_canonical_interfaces`, `local_and_encoded_keys_use_equal_node_and_module_syntax`, `keys_are_typed_names_not_owner_or_identity_authority`, `empty_source_and_sink_are_native_without_unit_ports`, `contract_value_record_codec_keeps_its_length_framed_golden_bytes`. Old recursive layout syntax is deliberately not retained; see gaps for unsupported convenience semantics. |

## Retired Suite Inventory

Each row names every old integration suite in its group, its meaningful behavior,
and the exact-test coverage keys above. All names have the `.rs` suffix.

| Old suites | Meaningful behavior and disposition | Coverage |
| --- | --- | --- |
| `action_lowering`, `graph_api`, `manual_api`, `unified_api` | Single compiler/runtime path, explicit declarations, decode failures | B, T, W, E |
| `authoring_registration`, `clone_lineage`, `patch_lineage` | Registration identity, clone scopes and revoked revisions | B, X, M |
| `canonical_interfaces`, `compiled_bindings`, `static_contracts` | Shape is not a claim, restricted binding, full schema, no resource authority | C, T, V |
| `contract_dependency_identity`, `scoped_claim_ownership` | Nested same-ID conflicts, retained requests and claim ownership | C, A, M |
| `composition_origin` | Allocation identity, ancestry, replacement origin | A, B; retained `provenance::tests::ancestry_preserves_distinct_composition_identities` |
| `assembly_cancel`, `assembly_model_checks` | Atomic assembly, cancellation, cross-kind replacement | M, X, B, L; `integration_regressions::generated_dags_and_contributor_orders_match_an_independent_reference` |
| `patch_planner`, `patch_syntax` | Dependency order, priorities, exact edges, splices, rollback | M, S, X; patch macro grammar retired |
| `patch_output_routing` | Independent replacement writes, no forged edges, selection of existing routes | M, A, F, Q; old output-selector syntax retired, native per-emission selection preserved |
| `operation_substitution` | Opposite body replacement, claims, private requirements, revision revocation | B, C, M, P |
| `module_projection_semantics`, `matching_projection`, `module_slots` | Public fanout, alias collision, nested projection without wrapper execution | P, T, X; grouped slot metadata retired |
| `grouped_authoring`, `grouped_wire_authoring` | Public boundaries, promised paths, portable fields | P, C, T; derive field convenience D |
| `source_operations`, `output_presence`, `typed_single_output`, `iteration_thirteen_unit` | Explicit zero-input sources, outputs and unit payloads | E; mandatory single output intentionally replaced |
| `output_semantics`, `output_wrapper`, `iteration_nine` | Option/Result are data, shared fanout, one/many/none selection and atomic validation | F, V, Q; payload regression added to `integration_regressions::option_and_result_remain_data`; native `ProcessEmission::select` replaces explicit target semantics, old target/prefix/filter wrapper syntax retired |
| `core_iteration`, `iteration_two`, `iteration_three`, `iteration_four`, `iteration_five` | Type modes, limits, manifests, replacement, fanout/preparation atomicity | T, B, F, L, P; wave reports and numeric manifests retired |
| `iteration_ten`, `iteration_eleven_layout`, `declaration_layout`, `layout_handle_identity` | Frozen typed layouts, endpoints, retargeting, revisions | B, T, D; pending mutable layout model retired |
| `iteration_thirteen_unit`, `node_definition_modes`, `inference_experiments` | Unit handling, independent input/output modes, signature validation | E, T, D |
| `typed_operations`, `selected_operations` | Typed discovery, public projections and custom process dispatch | B, C, P, O, D; selected-operation convenience helper retired |
| `lean_api`, `layout_api`, `consolidation_api` | Local move-only values, fanout, nested composition, codecs, cycles | T, V, F, P, W, L, D |
| `input_fields`, `iteration_eleven_layout`, `symbolic_pending` | Raw/renamed fields, nested layouts, failed late edits | B, X, D; old derives retired, typed parity pending |
| `explicit_arc_input`, `quality_10` | Zero deep clone, ownership, forged labels, adapters | V, T, F; `integration_regressions::generated_dags_and_contributor_orders_match_an_independent_reference` |
| `execution_budget`, `scheduler_lifecycle` | Budget, delayed work, cancellation, independent sessions | O, L; resumable budget design change below |
| `testing_api` | Helpers must use the real executor, not a test engine | W, native examples; old Fixture/report convenience removed |
| `portable_wire` | Positive/negative schema checks, compile-time typed checks, separate rustc crates | T, W, B |
| `portable_codec` | Codec locals/raw names, literal vectors, malformed bytes, descriptor checks | Retained except old layout-specific raw-name test; D gap |
| `inspection_live`, `inspection_cost` | Authoritative views, cheap metadata reads, stale binding | C, B; mutable pending layouts and timing assertions retired |
| `regressions_9` | Foreign handles, duplicate edges, adapter errors, recursive layouts | B, F, D; old depth-layout syntax retired |
| `iteration_four` | Public names, structure distinct from policy behavior | P, C, R; wave publication assertions retired |
| `iteration_nine` | Tuple broadcast and Option/Result payloads | F, V, `integration_regressions::option_and_result_remain_data` |
| `iteration_two` | Portable/local coexistence and diagnostics | T, W |
| `iteration_three` | Conditional execution, replacement, retained Result | E, B, `integration_regressions::option_and_result_remain_data` |

## Other Removed Sources

- `src/tests.rs`, including `conformance`, `partitioned_active_coloring_conformance`,
  `declarative_consumer_api_tests`, `echo_agent_poc`, and `sparse_activity_partition_tests`:
  expression DAGs, repeat admission, any-two, pair-then-third, upstream closure,
  skipped branches, cohorts and guards map to P, R, E, O and L. Exact sparse
  coloring/accounting counters are old scheduler implementation assertions.
- `src/inspection_tests.rs`: physical paths and IDs, nested declarations,
  claims and origins map to `fbp_components::process_ids_connections_and_exports_are_deterministic_by_full_path`,
  C and A. Native inspection, not an old CompiledGraph inspection adapter, owns provenance.
- `src/schema_ownership_tests.rs`: owned schema/error/derive descriptor reclamation
  maps to `integration_regressions::network_schema_metadata_and_captures_release_after_unload`,
  `schema_descriptors_and_interface_errors_are_reclaimable`
  and `derived_identities_are_not_cached_and_local_types_remain_distinct`; payload/code
  lifetime maps to V and L.
- Embedded tests in deleted authoring/observer/patching/scheduler sources map to
  B, P, M, S, X, R, O and L. Their closure identity, patch fingerprint, numeric
  slot and wave accounting assertions are not part of the native API.
- `interface.rs` legacy tests: exact named sets and local/wire separation map to T.
  Unnamed numeric slots and exactly-one-output restrictions are intentionally gone.
- `examples/grouped_contract.rs`, `examples/multi_contributor.rs`: replaced by
  `examples/plugin_composition.rs` and C/P/M tests. Native examples also cover
  basic execution, owned dispatch and streaming. `benches/layout.rs` and
  `benches/ownership.rs` measure deleted layout/engine APIs; no native performance
  parity is claimed and their Cargo bench declarations are removed.
- `tests/support/selected_operation.rs` is removed with its old typed helper.
  Portable provider/consumer fixtures remain independently compiled, with native
  execution substituted for the old engine.

## Deliberate Semantic Changes

- Zero, one, or many named outputs replace the mandatory single slot-zero output.
  Sources and sinks need no dummy unit port. Port names are opaque nonempty names,
  not a second component-path grammar. Unnamed numeric endpoints no longer exist.
- Exact shape matching is preserved: `Interface::check_shape` requires the full
  named input/output sets and type identities, rejecting missing, extra, mistyped,
  or wrong-direction ports. `Contract::check_shape` delegates and adds semantic ID
  context; matching shape alone never supplies an explicit contract claim.
- Persistent bounded FIFO sessions and request scopes replace waves, coloring and
  report retention. Policies cannot skip FIFO heads. Optional inputs require real
  EOS or request absence proofs, not global quiescence. Staged requests retain one
  activation without keeping a permanent old scheduler history.
- Step-budget exhaustion is resumable, not implicit cancellation of all work.
  Cancellation invalidates authority but cannot preempt a handed-out process.
  Restart policy is explicit; effects and already committed publications do not roll back.
- Multiple `Step::Emit` results replace one-shot return adapters. Processes choose
  output names; `ProcessEmission::new` broadcasts over compiled edges, while
  `ProcessEmission::select(output, value, destinations)` selects one, many, or no
  existing process connections using stable input `PortPath`s. Public module
  input aliases resolve their full flattened fanout. Unknown, unconnected, or
  duplicate physical destinations (including overlapping aliases) reject the
  entire emission before delivery. Erased paths grant no authoring or injection
  authority. Captures are explicit source-output exports and remain connected
  observations even for an empty process selection; an empty selection with no
  captures is intentional discard. The destination count is checked against the
  source output's compiled process-connection budget before routing allocation,
  and publication retries retain the resolved selection without rerunning effects.
  Old target/prefix/filter return wrappers and patch macro/default-endpoint grammar
  are not retained. Prefix/callback selector conveniences are not reintroduced;
  ordinary native multi-output routers remain the usual routing alternative.
- Immutable component interfaces and explicit replacement replace late mutable
  layout configuration. New Ports derives are clients of that canonical model,
  not a compatibility implementation of old InputFields/PortArgs/Layout macros.
- Typed `connect` requires the same Rust payload parameter. Independently declared
  portable Rust types use explicit `connect_dynamic` plus full-schema validation;
  the old wire-mode ordinary-connect overload is not retained.
- Composition uses exact grants and explicit dependency order, not origin ancestry
  as authority or fractional splice positions. Private nested writes reject explicitly.

## Honest Gaps

- Ports tests cover raw identifiers, generic local types, canonical manual
  equivalence, key direction/type/representation checks and golden bytes. Renamed
  and recursively flattened input records now use the same selectable named ports
  for metadata and reconstruction. Unique non-Clone fields move; explicit Arc
  fields share; reconstructed values retain source leases. Compile-fail doctests
  in `src/port_keys.rs` cover payload
  mismatch, direction mismatch, conflicting directions, uppercase name collisions,
  and encoded generic codec bounds. Broader macro syntax remains independently owned.
- `integration_regressions::generated_dags_and_contributor_orders_match_an_independent_reference`
  covers 32 generated multi-input DAGs, forward/reverse contributor admission and
  three repeated inputs per session against integer equations independent of the
  compiler/runtime. The randomized composition gap now has native executable
  coverage in `fbp_composition_model.rs`: an independent subset-selection oracle
  and integer-map topology replay, without deriving expected selections or edges
  through the composer. Exact tests and bounded counts are:
  `bounded_priority_dependency_selections_match_independent_subsets` checks all
  27 assignments of three priorities in {-1, 0, 1} across four dependency shapes,
  each in forward/reverse batch order (216 cases), including transitive dependent
  deactivation, equal-priority rejection and revoke-to-base replay;
  `fixed_seed_replacement_rewire_export_and_revoke_sequences_match_model` uses
  seed `0x6d6f64656c666270` for 16 sequences of 32 generated decisions (512 decisions,
  each replayed in both batch orders, plus registry bounding and final cleanup),
  covering primitive/module replacement with identical interfaces, additions,
  connect/disconnect, exports, conflicts and revoke;
  `fixed_seed_ordered_splices_match_reference_topology_and_noncommuting_arithmetic`
  uses that seed XOR `0x73706c696365` for 32 generated map pairs in both batch orders
  (64 ordered splice/revoke cases and 64 unordered-splice rejection cases);
  `missing_exact_grant_rejects_whole_batch_and_preserves_generation_and_provenance`
  checks five omitted exact grants, including a generated splice edge. Checks
  include symbolic and flattened topology/capacities, arithmetic captures for
  three repeated inputs, exact provenance and definition identity, failed-batch
  registry/publication atomicity, and old bindings surviving rejection but not
  resolving against a new compiled generation. All four tests and targeted Clippy
  pass with default and no-default features. Full graph no-default tests/Clippy
  also pass. The temporary missing `InputRecord`/`InputGroup` facade exports have
  been added and reconstruction regressions pass. This
  closes the missing randomized composition-model coverage, not an exhaustive
  claim over all networks, priorities, dependency DAGs or edit sequences. No
  timing/performance parity is asserted.
- Native inspection has functional no-spawn and deterministic metadata tests, but
  the retired timing benchmarks have no native performance baseline replacement.
- Old report text/event-count/wave-color partition metrics are intentionally not
  preserved. Consumers needing analogous observability need a separately specified
  native telemetry contract, not guessed compatibility.

## Workspace Callers

Workspace-wide Rust search (including actual `examples` and `experiments` paths)
found no legacy Graph API references outside `packages/phenix-graphs` at removal
time. Kernel/Plugin/StorageGraph migration and Ports derive work are independently
owned; this ledger does not certify their tests or modify those packages.

## Verification

- Graph library and integration tests pass with default and no-default features.
  `integration_regressions.rs` adds nine native tests, including separate rustc
  compilation, malformed decode isolation, Store authorization, schema reclamation,
  Option/Result data and generated multi-contributor DAG execution.
- All 13 graph doctests pass, including the five typed-port compile failures.
- Clippy passes for graph library/tests in both configurations using
  `nix shell nixpkgs#clippy -c cargo clippy ...`; the existing now-unused
  `value::identity` helper warns. `value.rs` is outside removal ownership.
- `cargo check --workspace --all-targets` is attempted but blocked by native
  examples outside removal ownership: `owned_dispatch.rs:100`,
  `plugin_composition.rs:93`, and `streaming_fbp.rs:66` access private `Packet.value`
  instead of calling `value()`. These are not legacy Graph API callers.
- Root facade is 43 lines, canonical interface 162, canonical contract 98,
  limits 17, resources 26, provenance 73, and testing facade 2 after formatting.
  The old facade alone was 5,165 lines at the start of removal. Fifty-eight old
  integration suites plus their selected-operation support file are retired with
  the mappings above; codec conformance and independent portable fixtures remain.
- The temporary `contract::native` import seam and unused legacy identity helper
  are removed. Compiler/component imports use `contract::catalog` directly.
