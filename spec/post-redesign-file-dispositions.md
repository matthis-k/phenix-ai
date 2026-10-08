# Post-redesign repository file dispositions

status: specification-only

## Scope, snapshot, and decision semantics

- Repository: `matthis-k/phenix-ai`.
- Snapshot: `main` Git tree `cc914e2ebfff56df428e54af0d01399a199efc73`, **591 existing tracked files**; this new audit file is the 592th file.
- Coverage: **592/592** paths have exactly one expected disposition in the table below, including generated-looking assets, Nix, scripts, tests, documentation and configuration.
- **This is an expected post-#726–#731 disposition, not proof that the current file is unused. No code deletion is authorized by this inventory alone.**
- Evidence `Source` means the named file was opened for targeted inspection in this planning pass; `Architecture` means the classification follows file ownership, role or the redesign contract and still requires individual semantic verification before mutation.
- Each row is provisional until its consumer graph, runtime role and parity tests are checked. `Keep` does not mean no edits; `Review` is an explicit disposition indicating evidence still needed, not a silent exclusion.
- A downstream cleanup PR must reference exact file rows, identify the replacement, and prove no selection, import, Nix artifact, test or external consumer breaks.

| Expected disposition | Meaning | Number of files |
| --- | --- | ---: |
| Keep | Remains owned at this location, perhaps with normal maintenance | 374 |
| Refactor | Retain file/owner but remove redundant responsibilities | 138 |
| Move | Move functionality/tests to new owner, then delete original | 46 |
| Retire | Delete after explicit replacement and parity checks | 8 |
| Review | Ownership or liveness unresolved; inspect before deciding | 26 |
| **Total** | **Every tracked file plus this inventory** | **592** |

## Verified architectural pressure points

1. `phenix-core/src/agent.rs` defines model inference, tools, skills and context contracts. These are domain semantics and should move out of Core into application/plugin contract ownership, without changing their versioned external identities.
2. `phenix-plugin-hooks` is explicitly designated a legacy dispatcher by `spec/kernel-hooks.md`; use the existing Service Layers and Event/Listener mechanisms, with parity tests, before retiring it.
3. `phenix-agent-configurations/src/lib.rs` still selects `phenix.hooks` in Advanced defaults, contradicting the hooks specification's statement that it is outside the default suite. Resolve this as part of the hooks migration; do not delete it without updating selection.
4. `phenix-plugin-basic-model` is a deterministic echo provider selected by the Basic suite. Preserve it as test support, but cease treating it as a production model selection.
5. `phenix-plugin-catalog/src/lib.rs` is a manually curated registry/re-export surface. Replace its product registration ownership with plugin discovery/contributions; relocate its `tests/coordination.rs` behavioral evidence.
6. `phenix-harness/src/runtime_builder.rs` and `src/basic_suite.rs` enumerate first-party factories; this is deployment/application assembly that should eventually consume generic typed descriptors and portable profiles.
7. `phenix-plugin-execution/src/tool_schedule.rs` owns a tool-specific concurrency scheduler. Consolidate with generic graph scheduling or a tool-kind policy provider; never silently remove the exclusivity semantics.
8. `phenix-sdk/src/contracts/*.rs` currently carries many agent-specific types. Move domain contracts into plugin-owned contract packages; keep the SDK itself domain-neutral and compatible with external plugins.
9. Do not discard test cases simply because their legacy owning crate is retired: rehome behavior/authority/generation parity tests first.
10. Existing #726–#731 branches remain the source of truth for intermediate ownership. Refresh this snapshot after they merge; this is a **post-redesign** target.

## Explicit dependency order

| Slice | Prerequisites | Changes | Proof to permit deletion |
| --- | --- | --- | --- |
| Core/SDK semantic boundary | #726, #728 | Move agent contract types from Core, stabilize typed declarations | Same public contract IDs, schema and consumer outcomes |
| Composition unification | #729, #730 | Retire duplicate graph/registry semantics | Canonical graph/provider resolution, authority and provenance tests |
| Consumer migration | #731 | Tools, skills, agent loop, product assembly | Basic/Full behavior, non-agent Core canary, external frontend compatibility |
| Legacy retirements | Consumer migration + owner-specific parity | Hooks dispatcher, echo production registration, static catalog and imperative progression | No selection paths or direct dependencies remain, old tests rehomed |
| Repo normalization | Legacy retirements | Nix, CI, docs, scripts, fixture/dependency cleanup | Full tracked-file re-inventory; CI/Nix check coverage maintained |

## File-by-file expected disposition

### `.githooks/` (1 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `.githooks/pre-commit` | Review | Determine whether it duplicates canonical Nix-owned commit checks. | Nix tooling parity | Architecture |

### `.github/` (5 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `.github/workflows/ci.yml` | Refactor | Keep required CI workflow, audit duplicated shards, maintenance and tests after topology migration. | #731 + CI baseline | Architecture |
| `.github/workflows/dependency-security.yml` | Refactor | Keep required CI workflow, audit duplicated shards, maintenance and tests after topology migration. | #731 + CI baseline | Architecture |
| `.github/workflows/shared-binary-cache.yml` | Refactor | Keep required CI workflow, audit duplicated shards, maintenance and tests after topology migration. | #731 + CI baseline | Architecture |
| `.github/workflows/sync-maintenance.yml` | Refactor | Keep required CI workflow, audit duplicated shards, maintenance and tests after topology migration. | #731 + CI baseline | Architecture |
| `.github/workflows/worker-executor.yml` | Refactor | Keep required CI workflow, audit duplicated shards, maintenance and tests after topology migration. | #731 + CI baseline | Architecture |

### `.gitignore/` (1 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `.gitignore` | Keep | Repository generated-content hygiene. | — | Architecture |

### `AGENTS.md/` (1 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `AGENTS.md` | Keep | Repo operator/developer or architecture guidance; update stale references after migration. | #731 + docs sync | Architecture |

### `config/` (2 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `config/phenix/NOTICE.md` | Refactor | Retain portable product config; remove Nix-only feature selection and generated semantics. | #731 | Architecture |
| `config/phenix/runtime.nix` | Refactor | Retain portable product config; remove Nix-only feature selection and generated semantics. | #731 | Architecture |

### `DEVELOPMENT.md/` (1 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `DEVELOPMENT.md` | Keep | Repo operator/developer or architecture guidance; update stale references after migration. | #731 + docs sync | Architecture |

### `flake.lock/` (1 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `flake.lock` | Keep | Canonical dependency/package metadata; synchronize when moving or retiring crates. | — | Architecture |

### `flake.nix/` (1 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `flake.nix` | Keep | Canonical dependency/package metadata; synchronize when moving or retiring crates. | — | Architecture |

### `glossary.md/` (1 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `glossary.md` | Keep | Repo operator/developer or architecture guidance; update stale references after migration. | #731 + docs sync | Architecture |

### `LICENSE/` (1 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `LICENSE` | Keep | Repository licensing. | — | Architecture |

### `modules/` (9 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `modules/commit-candidate.nix` | Keep | Retain Nix deployment/build capability; no runtime semantic authority. | — | Architecture |
| `modules/development.nix` | Keep | Retain Nix deployment/build capability; no runtime semantic authority. | — | Architecture |
| `modules/flake-module.nix` | Keep | Retain Nix deployment/build capability; no runtime semantic authority. | — | Architecture |
| `modules/harness-product.nix` | Refactor | Thin portable product config wrapper; remove duplicated product selection. | #731 | Architecture |
| `modules/lua-binding-integration.nix` | Keep | Retain Nix deployment/build capability; no runtime semantic authority. | — | Architecture |
| `modules/package-sets.nix` | Refactor | Remove hardcoded plugin-name identity map and migration aliases after portable profile selection. | #730, #731 | Source |
| `modules/plugin-packaging.nix` | Refactor | Nix packages deployment artifacts, not tool/skill resolution or provider selection authority. | #730, #731 | Source |
| `modules/rust-artifacts.nix` | Keep | Retain Nix deployment/build capability; no runtime semantic authority. | — | Architecture |
| `modules/stitch.nix` | Review | Establish its live package consumer and unique responsibility before retaining/removing. | consumer + CI references | Architecture |

### `README.md/` (1 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `README.md` | Keep | Repo operator/developer or architecture guidance; update stale references after migration. | #731 + docs sync | Architecture |

### `rust/` (3 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/Cargo.lock` | Keep | Canonical dependency/package metadata; synchronize when moving or retiring crates. | — | Architecture |
| `rust/Cargo.toml` | Keep | Canonical dependency/package metadata; synchronize when moving or retiring crates. | — | Architecture |
| `rust/deny.toml` | Review | Unrecognized tracked artifact; verify purpose, ownership and consumer before removal. | consumer trace | Architecture |

### `rust/crates/phenix-acp-stdio/` (8 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-acp-stdio/Cargo.toml` | Keep | ACP stdio application and transport boundary; independent from Core. | — | Architecture |
| `rust/crates/phenix-acp-stdio/examples/observable_callback_fixture.rs` | Keep | ACP stdio application and transport boundary; independent from Core. | — | Architecture |
| `rust/crates/phenix-acp-stdio/src/client_tools.rs` | Keep | ACP stdio application and transport boundary; independent from Core. | — | Architecture |
| `rust/crates/phenix-acp-stdio/src/lib.rs` | Keep | ACP stdio application and transport boundary; independent from Core. | — | Architecture |
| `rust/crates/phenix-acp-stdio/src/transport.rs` | Keep | ACP stdio application and transport boundary; independent from Core. | — | Architecture |
| `rust/crates/phenix-acp-stdio/tests/client_callback_retirement.rs` | Keep | ACP stdio application and transport boundary; independent from Core. | — | Architecture |
| `rust/crates/phenix-acp-stdio/tests/client_tool_reconnect.rs` | Keep | ACP stdio application and transport boundary; independent from Core. | — | Architecture |
| `rust/crates/phenix-acp-stdio/tests/nested_client_callable_input.rs` | Keep | ACP stdio application and transport boundary; independent from Core. | — | Architecture |

### `rust/crates/phenix-adapter-acp/` (15 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-adapter-acp/Cargo.toml` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Architecture |
| `rust/crates/phenix-adapter-acp/src/callbacks.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Architecture |
| `rust/crates/phenix-adapter-acp/src/dispatch.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Architecture |
| `rust/crates/phenix-adapter-acp/src/elicitation.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Architecture |
| `rust/crates/phenix-adapter-acp/src/errors.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Architecture |
| `rust/crates/phenix-adapter-acp/src/extension_callbacks.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Architecture |
| `rust/crates/phenix-adapter-acp/src/extension_dispatch.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Architecture |
| `rust/crates/phenix-adapter-acp/src/extensions.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Architecture |
| `rust/crates/phenix-adapter-acp/src/lib.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Architecture |
| `rust/crates/phenix-adapter-acp/src/updates.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Architecture |
| `rust/crates/phenix-adapter-acp/tests/application_dispatch.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Architecture |
| `rust/crates/phenix-adapter-acp/tests/capabilities.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Architecture |
| `rust/crates/phenix-adapter-acp/tests/disconnect.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Architecture |
| `rust/crates/phenix-adapter-acp/tests/runtime_plugin.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Architecture |
| `rust/crates/phenix-adapter-acp/tests/session_lifecycle.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Architecture |

### `rust/crates/phenix-agent-configurations/` (2 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-agent-configurations/Cargo.toml` | Refactor | Retain pure product-profile declarations; drop old plugin identity selections when replacements land. | #731 | Architecture |
| `rust/crates/phenix-agent-configurations/src/lib.rs` | Refactor | Advanced defaults currently select legacy phenix.hooks despite spec/kernel-hooks.md retirement contract; replace profile selection. | #731 + hook parity | Source |

### `rust/crates/phenix-application-interface/` (15 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-application-interface/Cargo.toml` | Keep | Portable frontend and application contracts. | — | Architecture |
| `rust/crates/phenix-application-interface/fixtures/application.rs` | Keep | Portable frontend and application contracts. | — | Architecture |
| `rust/crates/phenix-application-interface/src/bin/phenix-application-descriptor.rs` | Keep | Portable frontend and application contracts. | — | Architecture |
| `rust/crates/phenix-application-interface/src/catalog.rs` | Keep | Portable frontend and application contracts. | — | Architecture |
| `rust/crates/phenix-application-interface/src/client.rs` | Keep | Portable frontend and application contracts. | — | Architecture |
| `rust/crates/phenix-application-interface/src/descriptor.rs` | Keep | Portable frontend and application contracts. | — | Architecture |
| `rust/crates/phenix-application-interface/src/generate.rs` | Keep | Portable frontend and application contracts. | — | Architecture |
| `rust/crates/phenix-application-interface/src/lib.rs` | Keep | Portable frontend and application contracts. | — | Architecture |
| `rust/crates/phenix-application-interface/src/tests.rs` | Keep | Portable frontend and application contracts. | — | Architecture |
| `rust/crates/phenix-application-interface/src/types/discovery.rs` | Keep | Portable frontend and application contracts. | — | Architecture |
| `rust/crates/phenix-application-interface/src/types/interaction.rs` | Keep | Portable frontend and application contracts. | — | Architecture |
| `rust/crates/phenix-application-interface/src/types/log.rs` | Keep | Portable frontend and application contracts. | — | Architecture |
| `rust/crates/phenix-application-interface/src/types/mod.rs` | Keep | Portable frontend and application contracts. | — | Architecture |
| `rust/crates/phenix-application-interface/src/types/observable.rs` | Keep | Portable frontend and application contracts. | — | Architecture |
| `rust/crates/phenix-application-interface/src/types/session.rs` | Keep | Portable frontend and application contracts. | — | Architecture |

### `rust/crates/phenix-binding-generator/` (2 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-binding-generator/Cargo.toml` | Keep | Generate typed foreign-language bindings from declared interfaces. | — | Architecture |
| `rust/crates/phenix-binding-generator/src/lib.rs` | Keep | Generate typed foreign-language bindings from declared interfaces. | — | Architecture |

### `rust/crates/phenix-binding-lua/` (5 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-binding-lua/Cargo.toml` | Keep | Lua/Neovim integration must remain behind the client boundary. | — | Architecture |
| `rust/crates/phenix-binding-lua/src/facade.rs` | Keep | Lua/Neovim integration must remain behind the client boundary. | — | Architecture |
| `rust/crates/phenix-binding-lua/src/lib.rs` | Keep | Lua/Neovim integration must remain behind the client boundary. | — | Architecture |
| `rust/crates/phenix-binding-lua/src/tools.rs` | Keep | Lua/Neovim integration must remain behind the client boundary. | — | Architecture |
| `rust/crates/phenix-binding-lua/src/tools_regression.rs` | Keep | Lua/Neovim integration must remain behind the client boundary. | — | Architecture |

### `rust/crates/phenix-client/` (2 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-client/Cargo.toml` | Keep | Transport-neutral application client DTOs. | — | Architecture |
| `rust/crates/phenix-client/src/lib.rs` | Keep | Transport-neutral application client DTOs. | — | Source |

### `rust/crates/phenix-client-acp/` (3 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-client-acp/Cargo.toml` | Keep | Typed ACP client implementation. | — | Architecture |
| `rust/crates/phenix-client-acp/build.rs` | Keep | Typed ACP client implementation. | — | Architecture |
| `rust/crates/phenix-client-acp/src/lib.rs` | Keep | Typed ACP client implementation. | — | Architecture |

### `rust/crates/phenix-contract/` (9 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-contract/Cargo.toml` | Keep | Versioned portable types and canonical typed contributions. | — | Architecture |
| `rust/crates/phenix-contract/src/contract.rs` | Keep | Versioned portable types and canonical typed contributions. | — | Architecture |
| `rust/crates/phenix-contract/src/contract_wire.rs` | Keep | Versioned portable types and canonical typed contributions. | — | Architecture |
| `rust/crates/phenix-contract/src/identity.rs` | Keep | Versioned portable types and canonical typed contributions. | — | Architecture |
| `rust/crates/phenix-contract/src/infallible_value.rs` | Keep | Versioned portable types and canonical typed contributions. | — | Architecture |
| `rust/crates/phenix-contract/src/interface.rs` | Keep | Versioned portable types and canonical typed contributions. | — | Architecture |
| `rust/crates/phenix-contract/src/lib.rs` | Keep | Versioned portable types and canonical typed contributions. | — | Architecture |
| `rust/crates/phenix-contract/src/std_value.rs` | Keep | Versioned portable types and canonical typed contributions. | — | Architecture |
| `rust/crates/phenix-contract/src/structural_value.rs` | Keep | Versioned portable types and canonical typed contributions. | — | Architecture |

### `rust/crates/phenix-core/` (89 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-core/Cargo.toml` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/agent.rs` | Move | AI-only inference/tool/skill/context contracts belong in application contract package, not Core. | #731 + references | Source |
| `rust/crates/phenix-core/src/artifact.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Source |
| `rust/crates/phenix-core/src/authority.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/callable.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/component_endpoint_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/src/component_reentrancy_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/src/composition/activation.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/composition/component.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/composition/component_invocation.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/composition/inspection.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/composition/manifest.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/composition/mod.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/composition/provider_resolution.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/composition/registry.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/composition/resolver.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/configuration/mod.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/configuration/resolution.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/configuration_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/src/content_reference.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/events.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/host_authority_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/src/invalid_candidate_activation_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/src/invocation.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Source |
| `rust/crates/phenix-core/src/layer_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/src/lib.rs` | Refactor | Retain generic Core public API, remove AI-specific reexports after domain contracts migrate. | #731 | Architecture |
| `rust/crates/phenix-core/src/logging.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/metadata/composition.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/metadata/frontend.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/metadata/input.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/metadata/inspection.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/metadata/mod.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/metadata/reconciliation.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/metadata_semantic_identity_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/src/observable.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/observable/helpers.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/observable/model.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/observable/store.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/observable/transaction_canonical.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/observable/transaction_finish.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/observable/transaction_mutate.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/observable/transaction_types.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/persistence/backend.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/persistence/bootstrap.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/persistence/mod.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/persistence/provider.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/persistence/provider/tests.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/persistence/value.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/plugin/build.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/plugin/build_execution.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/plugin/context.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/plugin/management.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/plugin/mod.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/plugin/prepared_mutation.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/plugin_build_loading_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/src/plugin_management_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/src/plugin_runtime_adapter_host_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/src/plugin_runtime_adapter_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/src/provider_availability_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/src/provider_fallback_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/src/provider_rebind_generation_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/src/reconciliation/graph.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/reconciliation/inspection.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/reconciliation/live.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/reconciliation/mod.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/runtime.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/runtime/dispatch.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/runtime/host.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/runtime/kernel.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/runtime/listener.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/runtime/owned_transactions.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/runtime/persistence_bootstrap.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/runtime/reconciliation.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/runtime/residency.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/runtime/tests.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/runtime/trace.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Architecture |
| `rust/crates/phenix-core/src/runtime_component_parity_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/src/runtime_topology_generation_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/src/sdk.rs` | Review | Generic SDK observable machinery may remain in Core; separate public SDK integration from kernel authority. | #728 + API ownership review | Source |
| `rust/crates/phenix-core/src/service_layer_dispatch_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/src/tasks.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Source |
| `rust/crates/phenix-core/src/third_party_component_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/tests/event_delivery_contract.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/tests/event_generation_provenance.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/tests/kernel_concurrency_contract.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/tests/observable_allocations.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/tests/observable_semantics.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/tests/persistence_backend_conformance.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |
| `rust/crates/phenix-core/tests/resource_metadata_reconciliation.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Architecture |

### `rust/crates/phenix-domain/` (16 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-domain/Cargo.toml` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | Architecture |
| `rust/crates/phenix-domain/src/attempts.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | Architecture |
| `rust/crates/phenix-domain/src/client_tools.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | Architecture |
| `rust/crates/phenix-domain/src/debug.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | Architecture |
| `rust/crates/phenix-domain/src/delegation.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | Architecture |
| `rust/crates/phenix-domain/src/delegation/tests.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | Architecture |
| `rust/crates/phenix-domain/src/failures.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | Architecture |
| `rust/crates/phenix-domain/src/lib.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | Source |
| `rust/crates/phenix-domain/src/workspace/context.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | Architecture |
| `rust/crates/phenix-domain/src/workspace/decisions.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | Architecture |
| `rust/crates/phenix-domain/src/workspace/language.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | Architecture |
| `rust/crates/phenix-domain/src/workspace/language_operations.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | Architecture |
| `rust/crates/phenix-domain/src/workspace/mod.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | Architecture |
| `rust/crates/phenix-domain/src/workspace/objectives.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | Architecture |
| `rust/crates/phenix-domain/src/workspace/plans.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | Architecture |
| `rust/crates/phenix-domain/tests/context_serialization.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | Architecture |

### `rust/crates/phenix-harness/` (23 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-harness/Cargo.toml` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | Architecture |
| `rust/crates/phenix-harness/src/application.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | Architecture |
| `rust/crates/phenix-harness/src/authority.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | Architecture |
| `rust/crates/phenix-harness/src/basic_suite.rs` | Move | Move hardcoded echo/basic product setup to data-driven reference profiles and fixtures. | #731 | Source |
| `rust/crates/phenix-harness/src/bin/phenix-acp-fixture.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | Architecture |
| `rust/crates/phenix-harness/src/context_recovery.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | Architecture |
| `rust/crates/phenix-harness/src/exact_selected_suite_tests.rs` | Refactor | Keep product parity assertions but eliminate fixtures coupled to old hardcoded plugin registration. | #731 | Architecture |
| `rust/crates/phenix-harness/src/lib.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | Architecture |
| `rust/crates/phenix-harness/src/main.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | Architecture |
| `rust/crates/phenix-harness/src/model_surface_fixture.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | Architecture |
| `rust/crates/phenix-harness/src/persistence.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | Architecture |
| `rust/crates/phenix-harness/src/runtime.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | Architecture |
| `rust/crates/phenix-harness/src/runtime_builder.rs` | Refactor | Manual add_selected/plugin-factory roster should lower from selected manifests/contributions. | #728, #730, #731 | Source |
| `rust/crates/phenix-harness/src/runtime_config.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | Architecture |
| `rust/crates/phenix-harness/src/tests.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | Architecture |
| `rust/crates/phenix-harness/src/workspace_discovery.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | Architecture |
| `rust/crates/phenix-harness/tests/acp_session_lifecycle.rs` | Refactor | Keep product parity assertions but eliminate fixtures coupled to old hardcoded plugin registration. | #731 | Architecture |
| `rust/crates/phenix-harness/tests/application_projection.rs` | Refactor | Keep product parity assertions but eliminate fixtures coupled to old hardcoded plugin registration. | #731 | Architecture |
| `rust/crates/phenix-harness/tests/application_review.rs` | Refactor | Keep product parity assertions but eliminate fixtures coupled to old hardcoded plugin registration. | #731 | Architecture |
| `rust/crates/phenix-harness/tests/component_graph.rs` | Refactor | Keep product parity assertions but eliminate fixtures coupled to old hardcoded plugin registration. | #731 | Architecture |
| `rust/crates/phenix-harness/tests/invocation_defaults.rs` | Refactor | Keep product parity assertions but eliminate fixtures coupled to old hardcoded plugin registration. | #731 | Architecture |
| `rust/crates/phenix-harness/tests/process_roundtrip.rs` | Refactor | Keep product parity assertions but eliminate fixtures coupled to old hardcoded plugin registration. | #731 | Architecture |
| `rust/crates/phenix-harness/tests/supported_product_journeys.rs` | Refactor | Keep product parity assertions but eliminate fixtures coupled to old hardcoded plugin registration. | #731 | Architecture |

### `rust/crates/phenix-model-adapter/` (2 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-model-adapter/Cargo.toml` | Keep | Shared model-adapter application contract, never required by Core. | — | Architecture |
| `rust/crates/phenix-model-adapter/src/lib.rs` | Keep | Shared model-adapter application contract, never required by Core. | — | Architecture |

### `rust/crates/phenix-model-adapter-acp/` (7 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-model-adapter-acp/Cargo.toml` | Keep | ACP model-provider adapter implementation. | — | Architecture |
| `rust/crates/phenix-model-adapter-acp/src/lib.rs` | Keep | ACP model-provider adapter implementation. | — | Architecture |
| `rust/crates/phenix-model-adapter-acp/src/mcp_bridge.rs` | Keep | ACP model-provider adapter implementation. | — | Architecture |
| `rust/crates/phenix-model-adapter-acp/tests/fixtures/acp_continuity_agent.rs` | Keep | ACP model-provider adapter implementation. | — | Architecture |
| `rust/crates/phenix-model-adapter-acp/tests/fixtures/acp_tool_bridge_agent.rs` | Keep | ACP model-provider adapter implementation. | — | Architecture |
| `rust/crates/phenix-model-adapter-acp/tests/persistent_continuity.rs` | Keep | ACP model-provider adapter implementation. | — | Architecture |
| `rust/crates/phenix-model-adapter-acp/tests/tool_bridge.rs` | Keep | ACP model-provider adapter implementation. | — | Architecture |

### `rust/crates/phenix-model-adapter-native/` (6 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-model-adapter-native/Cargo.toml` | Keep | Native model-provider adapter implementation. | — | Architecture |
| `rust/crates/phenix-model-adapter-native/src/credentials.rs` | Keep | Native model-provider adapter implementation. | — | Architecture |
| `rust/crates/phenix-model-adapter-native/src/lib.rs` | Keep | Native model-provider adapter implementation. | — | Architecture |
| `rust/crates/phenix-model-adapter-native/src/oauth.rs` | Keep | Native model-provider adapter implementation. | — | Architecture |
| `rust/crates/phenix-model-adapter-native/src/providers.rs` | Keep | Native model-provider adapter implementation. | — | Architecture |
| `rust/crates/phenix-model-adapter-native/src/schema_adapter.rs` | Keep | Native model-provider adapter implementation. | — | Architecture |

### `rust/crates/phenix-plugin-api/` (3 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-api/Cargo.toml` | Refactor | Retain application-facing API, but use tool/skill kind-provider contracts instead of duplicated catalogs. | #730, #731 | Architecture |
| `rust/crates/phenix-plugin-api/src/lib.rs` | Refactor | Retain application-facing API, but use tool/skill kind-provider contracts instead of duplicated catalogs. | #730, #731 | Source |
| `rust/crates/phenix-plugin-api/src/tests.rs` | Refactor | Retain application-facing API, but use tool/skill kind-provider contracts instead of duplicated catalogs. | #730, #731 | Architecture |

### `rust/crates/phenix-plugin-artifacts/` (4 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-artifacts/Cargo.toml` | Keep | Artifact-specific plugin behavior, outside the kernel. | — | Architecture |
| `rust/crates/phenix-plugin-artifacts/src/component.rs` | Keep | Artifact-specific plugin behavior, outside the kernel. | — | Architecture |
| `rust/crates/phenix-plugin-artifacts/src/implementation.rs` | Keep | Artifact-specific plugin behavior, outside the kernel. | — | Architecture |
| `rust/crates/phenix-plugin-artifacts/src/lib.rs` | Keep | Artifact-specific plugin behavior, outside the kernel. | — | Architecture |

### `rust/crates/phenix-plugin-basic-agent/` (4 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-basic-agent/Cargo.toml` | Refactor | Retain only if new topology/node provider crate reuses package; remove obsolete loop package afterward. | #726, #731 | Architecture |
| `rust/crates/phenix-plugin-basic-agent/src/agent_loop.rs` | Retire | Replace imperative run progression with topology plugin and independently provided nodes; preserve parity tests. | #726, #731 | Architecture |
| `rust/crates/phenix-plugin-basic-agent/src/component_regression.rs` | Move | Rehome valuable agent-loop behavior proofs in the topology/node integration suite. | #726, #731 | Architecture |
| `rust/crates/phenix-plugin-basic-agent/src/lib.rs` | Refactor | Preserve agent-loop semantics in topology/node plugins; remove imperative progression after parity. | #726, #731 | Architecture |

### `rust/crates/phenix-plugin-basic-context/` (2 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-basic-context/Cargo.toml` | Keep | Minimal replaceable context provider useful as a baseline and fixture. | — | Architecture |
| `rust/crates/phenix-plugin-basic-context/src/lib.rs` | Keep | Minimal replaceable context provider useful as a baseline and fixture. | — | Source |

### `rust/crates/phenix-plugin-basic-model/` (2 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-basic-model/Cargo.toml` | Retire | Delete production crate manifest after echo implementation and references move to fixtures. | #731 + migrate echo fixture | Architecture |
| `rust/crates/phenix-plugin-basic-model/src/lib.rs` | Move | Preserve deterministic echo implementation as test-support fixture, not a production plugin. | #731 | Source |

### `rust/crates/phenix-plugin-basic-skills/` (18 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-basic-skills/Cargo.toml` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | Architecture |
| `rust/crates/phenix-plugin-basic-skills/skills/architect/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | Architecture |
| `rust/crates/phenix-plugin-basic-skills/skills/grilling/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | Architecture |
| `rust/crates/phenix-plugin-basic-skills/skills/implement/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | Architecture |
| `rust/crates/phenix-plugin-basic-skills/skills/interrogate/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | Architecture |
| `rust/crates/phenix-plugin-basic-skills/skills/investigate/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | Architecture |
| `rust/crates/phenix-plugin-basic-skills/skills/pickup/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | Architecture |
| `rust/crates/phenix-plugin-basic-skills/skills/plan/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | Architecture |
| `rust/crates/phenix-plugin-basic-skills/skills/pstack-LICENSE` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | Architecture |
| `rust/crates/phenix-plugin-basic-skills/skills/reflect/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | Architecture |
| `rust/crates/phenix-plugin-basic-skills/skills/rust/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | Architecture |
| `rust/crates/phenix-plugin-basic-skills/skills/rust/references/ecosystem.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | Architecture |
| `rust/crates/phenix-plugin-basic-skills/skills/rust/references/research.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | Architecture |
| `rust/crates/phenix-plugin-basic-skills/skills/ship/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | Architecture |
| `rust/crates/phenix-plugin-basic-skills/skills/to-questionnaire/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | Architecture |
| `rust/crates/phenix-plugin-basic-skills/skills/verify/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | Architecture |
| `rust/crates/phenix-plugin-basic-skills/skills/write/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | Architecture |
| `rust/crates/phenix-plugin-basic-skills/src/lib.rs` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | Architecture |

### `rust/crates/phenix-plugin-basic-tools/` (2 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-basic-tools/Cargo.toml` | Refactor | Become a standard plugin-defined tool kind/template consumer/provider. | #730, #731 | Architecture |
| `rust/crates/phenix-plugin-basic-tools/src/lib.rs` | Refactor | Become a standard plugin-defined tool kind/template consumer/provider. | #730, #731 | Architecture |

### `rust/crates/phenix-plugin-catalog/` (3 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-catalog/Cargo.toml` | Retire | Delete static roster crate manifest once first-party discovery/fixtures move. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-plugin-catalog/src/lib.rs` | Retire | Remove manually maintained first-party plugin re-export/registration catalog once dynamic discovery works. | #730, #731 | Source |
| `rust/crates/phenix-plugin-catalog/tests/coordination.rs` | Move | Move provider coordination verification into application integration tests before retiring roster. | #730, #731 | Source |

### `rust/crates/phenix-plugin-command-toolbelt/` (4 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-command-toolbelt/Cargo.toml` | Keep | Concrete command implementations are separate from tool registration mechanisms. | — | Architecture |
| `rust/crates/phenix-plugin-command-toolbelt/src/component.rs` | Keep | Concrete command implementations are separate from tool registration mechanisms. | — | Architecture |
| `rust/crates/phenix-plugin-command-toolbelt/src/implementation.rs` | Keep | Concrete command implementations are separate from tool registration mechanisms. | — | Architecture |
| `rust/crates/phenix-plugin-command-toolbelt/src/lib.rs` | Keep | Concrete command implementations are separate from tool registration mechanisms. | — | Architecture |

### `rust/crates/phenix-plugin-context/` (11 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-context/Cargo.toml` | Refactor | Retain context projection and prompt policy, remove overlapping orchestration/registration. | #731 | Architecture |
| `rust/crates/phenix-plugin-context/src/component.rs` | Refactor | Retain context projection and prompt policy, remove overlapping orchestration/registration. | #731 | Architecture |
| `rust/crates/phenix-plugin-context/src/implementation.rs` | Refactor | Retain context projection and prompt policy, remove overlapping orchestration/registration. | #731 | Architecture |
| `rust/crates/phenix-plugin-context/src/implementation_state.rs` | Refactor | Retain context projection and prompt policy, remove overlapping orchestration/registration. | #731 | Architecture |
| `rust/crates/phenix-plugin-context/src/lib.rs` | Refactor | Retain context projection and prompt policy, remove overlapping orchestration/registration. | #731 | Source |
| `rust/crates/phenix-plugin-context/src/materialization.rs` | Refactor | Retain context projection and prompt policy, remove overlapping orchestration/registration. | #731 | Architecture |
| `rust/crates/phenix-plugin-context/src/materialization_integration.rs` | Refactor | Retain context projection and prompt policy, remove overlapping orchestration/registration. | #731 | Architecture |
| `rust/crates/phenix-plugin-context/src/projection_state.rs` | Refactor | Retain context projection and prompt policy, remove overlapping orchestration/registration. | #731 | Architecture |
| `rust/crates/phenix-plugin-context/src/prompt.rs` | Refactor | Retain context projection and prompt policy, remove overlapping orchestration/registration. | #731 | Architecture |
| `rust/crates/phenix-plugin-context/src/state_integration.rs` | Refactor | Retain context projection and prompt policy, remove overlapping orchestration/registration. | #731 | Architecture |
| `rust/crates/phenix-plugin-context/src/state_service.rs` | Refactor | Retain context projection and prompt policy, remove overlapping orchestration/registration. | #731 | Architecture |

### `rust/crates/phenix-plugin-debug/` (4 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-debug/Cargo.toml` | Keep | Optional inspection provider based on generic Core observability. | — | Architecture |
| `rust/crates/phenix-plugin-debug/src/component.rs` | Keep | Optional inspection provider based on generic Core observability. | — | Architecture |
| `rust/crates/phenix-plugin-debug/src/implementation.rs` | Keep | Optional inspection provider based on generic Core observability. | — | Architecture |
| `rust/crates/phenix-plugin-debug/src/lib.rs` | Keep | Optional inspection provider based on generic Core observability. | — | Architecture |

### `rust/crates/phenix-plugin-efficiency-evaluation/` (4 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-efficiency-evaluation/Cargo.toml` | Keep | Optional benchmarking and evaluation implementation. | — | Architecture |
| `rust/crates/phenix-plugin-efficiency-evaluation/src/benchmark_outcomes.rs` | Keep | Optional benchmarking and evaluation implementation. | — | Architecture |
| `rust/crates/phenix-plugin-efficiency-evaluation/src/implementation.rs` | Keep | Optional benchmarking and evaluation implementation. | — | Architecture |
| `rust/crates/phenix-plugin-efficiency-evaluation/src/lib.rs` | Keep | Optional benchmarking and evaluation implementation. | — | Source |

### `rust/crates/phenix-plugin-environment-local/` (2 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-environment-local/Cargo.toml` | Keep | Concrete local execution environment implementation. | — | Architecture |
| `rust/crates/phenix-plugin-environment-local/src/lib.rs` | Keep | Concrete local execution environment implementation. | — | Architecture |

### `rust/crates/phenix-plugin-execution/` (20 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-execution/Cargo.toml` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | Architecture |
| `rust/crates/phenix-plugin-execution/src/agent_loop_regression.rs` | Move | Transfer behavioral assertions to resolved workflow integration tests before removing old loop. | #726, #731 | Architecture |
| `rust/crates/phenix-plugin-execution/src/attempt_integration.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | Architecture |
| `rust/crates/phenix-plugin-execution/src/attempt_service.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | Architecture |
| `rust/crates/phenix-plugin-execution/src/component.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | Architecture |
| `rust/crates/phenix-plugin-execution/src/configuration.rs` | Refactor | OrchestrationDefinition/Node must no longer compete with canonical workflow graph declarations. | #729, #731 | Architecture |
| `rust/crates/phenix-plugin-execution/src/configuration/packaged.rs` | Refactor | Align packaged orchestration/configuration with canonical graph and product profiles. | #729, #731 | Architecture |
| `rust/crates/phenix-plugin-execution/src/configuration_regression.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | Architecture |
| `rust/crates/phenix-plugin-execution/src/delegated_task_state.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | Architecture |
| `rust/crates/phenix-plugin-execution/src/generation_regression.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | Architecture |
| `rust/crates/phenix-plugin-execution/src/implementation.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | Architecture |
| `rust/crates/phenix-plugin-execution/src/lib.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | Source |
| `rust/crates/phenix-plugin-execution/src/resource_integration.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | Architecture |
| `rust/crates/phenix-plugin-execution/src/resource_service.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | Architecture |
| `rust/crates/phenix-plugin-execution/src/resource_transaction.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | Architecture |
| `rust/crates/phenix-plugin-execution/src/review.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | Architecture |
| `rust/crates/phenix-plugin-execution/src/root_reservation_integration.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | Architecture |
| `rust/crates/phenix-plugin-execution/src/step_transaction_service.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | Architecture |
| `rust/crates/phenix-plugin-execution/src/tool_schedule.rs` | Move | Consolidate concurrency planning with generic graph scheduling or tool-policy plugin; avoid a second scheduler. | #729, #731 | Source |
| `rust/crates/phenix-plugin-execution/tests/step_transaction.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | Architecture |

### `rust/crates/phenix-plugin-frontend/` (4 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-frontend/Cargo.toml` | Keep | Replaceable frontend discovery and capability provider. | — | Architecture |
| `rust/crates/phenix-plugin-frontend/src/component.rs` | Keep | Replaceable frontend discovery and capability provider. | — | Architecture |
| `rust/crates/phenix-plugin-frontend/src/implementation.rs` | Keep | Replaceable frontend discovery and capability provider. | — | Architecture |
| `rust/crates/phenix-plugin-frontend/src/lib.rs` | Keep | Replaceable frontend discovery and capability provider. | — | Source |

### `rust/crates/phenix-plugin-hooks/` (5 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-hooks/Cargo.toml` | Retire | Legacy hook dispatcher; use kernel Service Layers and Events with plugin-owned behavior. | #729, #731 + listener parity | Architecture |
| `rust/crates/phenix-plugin-hooks/src/component.rs` | Retire | Legacy hook dispatcher; use kernel Service Layers and Events with plugin-owned behavior. | #729, #731 + listener parity | Architecture |
| `rust/crates/phenix-plugin-hooks/src/implementation.rs` | Retire | Legacy hook dispatcher; use kernel Service Layers and Events with plugin-owned behavior. | #729, #731 + listener parity | Architecture |
| `rust/crates/phenix-plugin-hooks/src/lib.rs` | Retire | Legacy hook dispatcher; use kernel Service Layers and Events with plugin-owned behavior. | #729, #731 + listener parity | Source |
| `rust/crates/phenix-plugin-hooks/src/ownership_regression.rs` | Move | Retain layer/listener ownership regression under canonical Core/SDK tests before retiring hook crate. | #729, #731 + listener parity | Architecture |

### `rust/crates/phenix-plugin-interactive-ui/` (2 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-interactive-ui/Cargo.toml` | Keep | Portable UI document vocabulary and optional frontend contribution kind. | — | Architecture |
| `rust/crates/phenix-plugin-interactive-ui/src/lib.rs` | Keep | Portable UI document vocabulary and optional frontend contribution kind. | — | Source |

### `rust/crates/phenix-plugin-invocation-defaults/` (2 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-invocation-defaults/Cargo.toml` | Refactor | Separate invocations, clock, context recovery and routing policies into replaceable providers. | #731 | Architecture |
| `rust/crates/phenix-plugin-invocation-defaults/src/lib.rs` | Refactor | Separate invocations, clock, context recovery and routing policies into replaceable providers. | #731 | Source |

### `rust/crates/phenix-plugin-jobs/` (4 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-jobs/Cargo.toml` | Keep | Persistent job domain service, not a second kernel event loop. | — | Architecture |
| `rust/crates/phenix-plugin-jobs/src/component.rs` | Keep | Persistent job domain service, not a second kernel event loop. | — | Architecture |
| `rust/crates/phenix-plugin-jobs/src/implementation.rs` | Keep | Persistent job domain service, not a second kernel event loop. | — | Architecture |
| `rust/crates/phenix-plugin-jobs/src/lib.rs` | Keep | Persistent job domain service, not a second kernel event loop. | — | Architecture |

### `rust/crates/phenix-plugin-language/` (4 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-language/Cargo.toml` | Keep | Language/code intelligence provider. | — | Architecture |
| `rust/crates/phenix-plugin-language/src/component.rs` | Keep | Language/code intelligence provider. | — | Architecture |
| `rust/crates/phenix-plugin-language/src/implementation.rs` | Keep | Language/code intelligence provider. | — | Architecture |
| `rust/crates/phenix-plugin-language/src/lib.rs` | Keep | Language/code intelligence provider. | — | Architecture |

### `rust/crates/phenix-plugin-memory/` (21 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-memory/Cargo.toml` | Keep | Memory service and persistence/invalidation policy outside Core. | — | Architecture |
| `rust/crates/phenix-plugin-memory/src/association_store.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | Architecture |
| `rust/crates/phenix-plugin-memory/src/component.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | Architecture |
| `rust/crates/phenix-plugin-memory/src/context_service_integration.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | Architecture |
| `rust/crates/phenix-plugin-memory/src/context_service_state.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | Architecture |
| `rust/crates/phenix-plugin-memory/src/embedding_integration.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | Architecture |
| `rust/crates/phenix-plugin-memory/src/error.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | Architecture |
| `rust/crates/phenix-plugin-memory/src/freshness.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | Architecture |
| `rust/crates/phenix-plugin-memory/src/freshness_integration.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | Architecture |
| `rust/crates/phenix-plugin-memory/src/implementation.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | Architecture |
| `rust/crates/phenix-plugin-memory/src/lib.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | Architecture |
| `rust/crates/phenix-plugin-memory/src/maintenance_integration.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | Architecture |
| `rust/crates/phenix-plugin-memory/src/package.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | Architecture |
| `rust/crates/phenix-plugin-memory/src/persistence.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | Architecture |
| `rust/crates/phenix-plugin-memory/src/provenance_integration.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | Architecture |
| `rust/crates/phenix-plugin-memory/src/reranking_integration.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | Architecture |
| `rust/crates/phenix-plugin-memory/src/restart_integration.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | Architecture |
| `rust/crates/phenix-plugin-memory/src/retrieval.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | Architecture |
| `rust/crates/phenix-plugin-memory/src/revalidation_failure_integration.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | Architecture |
| `rust/crates/phenix-plugin-memory/src/supersession_integration.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | Architecture |
| `rust/crates/phenix-plugin-memory/src/tests.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | Architecture |

### `rust/crates/phenix-plugin-models/` (9 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-models/Cargo.toml` | Keep | Model selection, routing and provider behavior stays application-side. | — | Architecture |
| `rust/crates/phenix-plugin-models/src/component.rs` | Keep | Model selection, routing and provider behavior stays application-side. | — | Architecture |
| `rust/crates/phenix-plugin-models/src/implementation_sdk.rs` | Keep | Model selection, routing and provider behavior stays application-side. | — | Architecture |
| `rust/crates/phenix-plugin-models/src/implementation_sdk/catalog.rs` | Keep | Model selection, routing and provider behavior stays application-side. | — | Architecture |
| `rust/crates/phenix-plugin-models/src/implementation_sdk/packaged.rs` | Keep | Model selection, routing and provider behavior stays application-side. | — | Architecture |
| `rust/crates/phenix-plugin-models/src/implementation_sdk/tests.rs` | Keep | Model selection, routing and provider behavior stays application-side. | — | Architecture |
| `rust/crates/phenix-plugin-models/src/lib.rs` | Keep | Model selection, routing and provider behavior stays application-side. | — | Source |
| `rust/crates/phenix-plugin-models/src/routing_service.rs` | Keep | Model selection, routing and provider behavior stays application-side. | — | Architecture |
| `rust/crates/phenix-plugin-models/src/routing_state.rs` | Keep | Model selection, routing and provider behavior stays application-side. | — | Architecture |

### `rust/crates/phenix-plugin-openai-codex/` (2 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-openai-codex/Cargo.toml` | Keep | Specific model provider integration. | — | Architecture |
| `rust/crates/phenix-plugin-openai-codex/src/lib.rs` | Keep | Specific model provider integration. | — | Architecture |

### `rust/crates/phenix-plugin-options/` (2 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-options/Cargo.toml` | Keep | Application options provider; avoid a second kernel resolver. | — | Architecture |
| `rust/crates/phenix-plugin-options/src/lib.rs` | Keep | Application options provider; avoid a second kernel resolver. | — | Architecture |

### `rust/crates/phenix-plugin-planning/` (4 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-planning/Cargo.toml` | Keep | Optional planning policy implementation. | — | Architecture |
| `rust/crates/phenix-plugin-planning/src/component.rs` | Keep | Optional planning policy implementation. | — | Architecture |
| `rust/crates/phenix-plugin-planning/src/implementation.rs` | Keep | Optional planning policy implementation. | — | Architecture |
| `rust/crates/phenix-plugin-planning/src/lib.rs` | Keep | Optional planning policy implementation. | — | Source |

### `rust/crates/phenix-plugin-providers/` (3 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-providers/Cargo.toml` | Keep | Provider presets and registration data; avoid Nix-authoritative semantics. | — | Architecture |
| `rust/crates/phenix-plugin-providers/README.md` | Keep | Provider presets and registration data; avoid Nix-authoritative semantics. | — | Architecture |
| `rust/crates/phenix-plugin-providers/src/lib.rs` | Keep | Provider presets and registration data; avoid Nix-authoritative semantics. | — | Source |

### `rust/crates/phenix-plugin-repository-workers/` (4 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-repository-workers/Cargo.toml` | Keep | Optional repository-worker orchestration built on generic Core primitives. | — | Architecture |
| `rust/crates/phenix-plugin-repository-workers/src/component.rs` | Keep | Optional repository-worker orchestration built on generic Core primitives. | — | Architecture |
| `rust/crates/phenix-plugin-repository-workers/src/implementation.rs` | Keep | Optional repository-worker orchestration built on generic Core primitives. | — | Architecture |
| `rust/crates/phenix-plugin-repository-workers/src/lib.rs` | Keep | Optional repository-worker orchestration built on generic Core primitives. | — | Architecture |

### `rust/crates/phenix-plugin-session-tree/` (8 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-session-tree/Cargo.toml` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | Architecture |
| `rust/crates/phenix-plugin-session-tree/src/component.rs` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | Architecture |
| `rust/crates/phenix-plugin-session-tree/src/disable_independently_regression.rs` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | Architecture |
| `rust/crates/phenix-plugin-session-tree/src/implementation.rs` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | Architecture |
| `rust/crates/phenix-plugin-session-tree/src/interface.rs` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | Architecture |
| `rust/crates/phenix-plugin-session-tree/src/lib.rs` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | Architecture |
| `rust/crates/phenix-plugin-session-tree/src/session_layering_regression.rs` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | Architecture |
| `rust/crates/phenix-plugin-session-tree/src/session_tree_atomicity_regression.rs` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | Architecture |

### `rust/crates/phenix-plugin-sessions/` (4 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-sessions/Cargo.toml` | Keep | Session semantics and storage outside Core. | — | Architecture |
| `rust/crates/phenix-plugin-sessions/src/history_integration.rs` | Keep | Session semantics and storage outside Core. | — | Architecture |
| `rust/crates/phenix-plugin-sessions/src/implementation.rs` | Keep | Session semantics and storage outside Core. | — | Architecture |
| `rust/crates/phenix-plugin-sessions/src/lib.rs` | Keep | Session semantics and storage outside Core. | — | Architecture |

### `rust/crates/phenix-plugin-step-runner/` (6 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-step-runner/Cargo.toml` | Refactor | Keep step execution; remove agent-loop progression once topology owns control flow. | #726, #729 | Architecture |
| `rust/crates/phenix-plugin-step-runner/src/lib.rs` | Refactor | Keep step execution; remove agent-loop progression once topology owns control flow. | #726, #729 | Source |
| `rust/crates/phenix-plugin-step-runner/src/runner.rs` | Refactor | Limit to one step invocation; remove any graph/run progression authority. | #726, #729 | Architecture |
| `rust/crates/phenix-plugin-step-runner/tests/invocation.rs` | Refactor | Keep step execution; remove agent-loop progression once topology owns control flow. | #726, #729 | Architecture |
| `rust/crates/phenix-plugin-step-runner/tests/planned_step.rs` | Refactor | Keep step execution; remove agent-loop progression once topology owns control flow. | #726, #729 | Architecture |
| `rust/crates/phenix-plugin-step-runner/tests/recovery_invocation.rs` | Refactor | Keep step execution; remove agent-loop progression once topology owns control flow. | #726, #729 | Architecture |

### `rust/crates/phenix-plugin-workspace/` (4 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-plugin-workspace/Cargo.toml` | Keep | Workspace selection, policies and state remain domain-specific plugins. | — | Architecture |
| `rust/crates/phenix-plugin-workspace/src/component.rs` | Keep | Workspace selection, policies and state remain domain-specific plugins. | — | Architecture |
| `rust/crates/phenix-plugin-workspace/src/implementation.rs` | Keep | Workspace selection, policies and state remain domain-specific plugins. | — | Architecture |
| `rust/crates/phenix-plugin-workspace/src/lib.rs` | Keep | Workspace selection, policies and state remain domain-specific plugins. | — | Architecture |

### `rust/crates/phenix-provider-sdk/` (7 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-provider-sdk/Cargo.toml` | Keep | Reusable provider integration helpers, outside Core. | — | Architecture |
| `rust/crates/phenix-provider-sdk/src/auth.rs` | Keep | Reusable provider integration helpers, outside Core. | — | Architecture |
| `rust/crates/phenix-provider-sdk/src/lib.rs` | Keep | Reusable provider integration helpers, outside Core. | — | Architecture |
| `rust/crates/phenix-provider-sdk/src/protocol.rs` | Keep | Reusable provider integration helpers, outside Core. | — | Architecture |
| `rust/crates/phenix-provider-sdk/src/runtime.rs` | Keep | Reusable provider integration helpers, outside Core. | — | Architecture |
| `rust/crates/phenix-provider-sdk/src/store.rs` | Keep | Reusable provider integration helpers, outside Core. | — | Architecture |
| `rust/crates/phenix-provider-sdk/src/types.rs` | Keep | Reusable provider integration helpers, outside Core. | — | Architecture |

### `rust/crates/phenix-runtime/` (3 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-runtime/Cargo.toml` | Keep | Standalone generic kernel runtime with no AI plugin requirement. | — | Architecture |
| `rust/crates/phenix-runtime/src/lib.rs` | Keep | Standalone generic kernel runtime with no AI plugin requirement. | — | Source |
| `rust/crates/phenix-runtime/src/main.rs` | Keep | Standalone generic kernel runtime with no AI plugin requirement. | — | Architecture |

### `rust/crates/phenix-sdk/` (65 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-sdk/Cargo.toml` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/README.md` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/api.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/authoring.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/authoring/context.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/authoring/event_context.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/authoring/plugin.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/authoring/static_component.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/authoring/static_config.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/authoring/static_dispatch.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/authoring/static_graph_runtime.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/authoring/static_import.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/authoring/static_lifecycle.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/authoring/static_resource.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/agent_diagnostics.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/agent_loop.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/budget.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/context.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/context_admission.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/context_compaction.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/context_recovery_bootstrap.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/delegation.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/efficiency_evaluation.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/environment.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/execution.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/execution_resources.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/exploration.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/frontend.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/jobs.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/language.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/memory.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/memory_context.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/memory_freshness.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/mod.rs` | Refactor | Remove re-exports of relocated agent-domain contract definitions. | #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/model_dispatch.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/models.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/options.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/planning.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/primitive_agent_export.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/sessions.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/skills.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/step_attempt.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/step_runner.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/step_transaction.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/tool_observation.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/usage.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/usage_policy.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/contracts/workspace.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/lib.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/providers.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/src/public_projection.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/tests/fallible_provider_dispatch.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/tests/incompatible_schema.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/tests/plugin_attribute_graph.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/tests/plugin_attribute_only_gate.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/tests/plugin_component_authoring.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/tests/plugin_config_authoring.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/tests/plugin_dependency_authoring.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/tests/plugin_import_authoring.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/tests/plugin_layer_authority.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/tests/plugin_lifecycle_authoring.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/tests/plugin_manifest_authoring.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/tests/plugin_public_projection.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/tests/plugin_resource_authoring.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | Architecture |
| `rust/crates/phenix-sdk/tests/plugin_stateless_manifest_authoring.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | Architecture |

### `rust/crates/phenix-sdk-macros/` (10 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `rust/crates/phenix-sdk-macros/Cargo.toml` | Keep | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | Architecture |
| `rust/crates/phenix-sdk-macros/src/component_attr.rs` | Keep | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | Architecture |
| `rust/crates/phenix-sdk-macros/src/component_runtime_attr.rs` | Keep | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | Architecture |
| `rust/crates/phenix-sdk-macros/src/expose_attr.rs` | Keep | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | Architecture |
| `rust/crates/phenix-sdk-macros/src/interface_attr.rs` | Keep | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | Architecture |
| `rust/crates/phenix-sdk-macros/src/lib.rs` | Keep | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | Architecture |
| `rust/crates/phenix-sdk-macros/src/plugin_attr.rs` | Keep | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | Architecture |
| `rust/crates/phenix-sdk-macros/src/plugin_attr_core.rs` | Keep | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | Architecture |
| `rust/crates/phenix-sdk-macros/src/plugin_attr_legacy.rs` | Keep | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | Architecture |
| `rust/crates/phenix-sdk-macros/src/resource_attr.rs` | Keep | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | Architecture |

### `scripts/` (5 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `scripts/check-plugin-architecture.sh` | Move | Move enforcement/check logic into Nix-supported dev tooling, retaining coverage before retiring standalone shell script. | Nix check replacement + CI proof | Architecture |
| `scripts/check-rust-safety-policy.sh` | Move | Move enforcement/check logic into Nix-supported dev tooling, retaining coverage before retiring standalone shell script. | Nix check replacement + CI proof | Architecture |
| `scripts/check-spec-lifecycle-fixtures.sh` | Move | Move enforcement/check logic into Nix-supported dev tooling, retaining coverage before retiring standalone shell script. | Nix check replacement + CI proof | Architecture |
| `scripts/check-spec-lifecycle.sh` | Move | Move enforcement/check logic into Nix-supported dev tooling, retaining coverage before retiring standalone shell script. | Nix check replacement + CI proof | Architecture |
| `scripts/check-structural-boundaries.sh` | Move | Move enforcement/check logic into Nix-supported dev tooling, retaining coverage before retiring standalone shell script. | Nix check replacement + CI proof | Architecture |

### `share/` (1 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `share/phenix/interfaces/phenix.application@1.json` | Keep | Versioned language-neutral interface/schema asset. | — | Architecture |

### `spec/` (98 files)

| Tracked path | Expected | Why still needed / target replacement | Blocked by | Evidence |
| --- | --- | --- | --- | --- |
| `spec/acp-stdio.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/adapter-acp.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/agent-configurations.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/agent-runtime-orchestration.md` | Refactor | Align agent-specific orchestration language with generic workflow graph and advanced plugin topology. | #726, #729 | Architecture |
| `spec/application-cli.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/application-integration-terminology.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/application-interface.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/binding-lua.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/client-provided-tools-implementation.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/client-provided-tools.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/client-sdk-acp.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/composable-agent-runtime.md` | Refactor | Replace interim architecture mechanisms with normative usage-agnostic Core/plugin boundary. | #729, #730 | Architecture |
| `spec/configuration-frontends.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/context-artifacts-pruning.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/context-catalog.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/context-compaction.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/decisions-history.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/environment-workspace-boundary.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/execution-context-projection.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/fallback-memory-recall.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/followups/call-scope-endpoints.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Architecture |
| `spec/followups/core-module-boundaries.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Architecture |
| `spec/followups/memory-retrieval.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Architecture |
| `spec/followups/observable-handles.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Architecture |
| `spec/followups/plugin-durable-storage.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Architecture |
| `spec/followups/resolved-dispatch-plan.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Architecture |
| `spec/followups/token-cache-provider-integration.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Architecture |
| `spec/followups/token-code-entity-lineage.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Architecture |
| `spec/followups/token-efficiency-evaluation.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Architecture |
| `spec/followups/token-isolated-exploration-runtime.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Architecture |
| `spec/followups/token-lazy-tools-observations.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Architecture |
| `spec/followups/token-memory-code-dependencies.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Architecture |
| `spec/followups/token-primitive-agent-export.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Architecture |
| `spec/followups/token-structured-code-actions.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Architecture |
| `spec/frontend-services.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/interactive-ui.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/kernel-hooks.md` | Refactor | Keep retirement/migration invariants until legacy hooks are deleted, then consolidate with Layer/Event specs. | #731 | Source |
| `spec/language-intelligence.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/language-service.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/lifecycle-hooks.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/microkernel-composition-roadmap.md` | Keep | Normative usage-agnostic kernel boundary and accepted dependency ordering. | — | Source |
| `spec/model-routing.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/model-turn-protocol.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/objectives.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/observability.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/observable-values.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/persistent-terminals-jobs.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/plans.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/plugin-architecture-enforcement.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Source |
| `spec/plugin-artifact-readers.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/plugin-authoring-macro.md` | Refactor | Document typed const/field contributions and kind-generic annotation syntax. | #728, #730 | Architecture |
| `spec/plugin-call-binding.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/plugin-cli.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/plugin-context.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/plugin-contributions.md` | Refactor | Reconcile old contribution language against typed descriptor/kind template contracts. | #728, #730 | Architecture |
| `spec/plugin-durable-data.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/plugin-embedded-runtime.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/plugin-events.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/plugin-host.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/plugin-memory-freshness.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/plugin-memory.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/plugin-options.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/plugin-persistence.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/plugin-process-runtime-bridge.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/plugin-resolution.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/plugin-runtime-bridges.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/plugin-sdk-context.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/plugin-sdk.md` | Refactor | Remove deprecated first-party authoring shortcuts in favor of generic SDK contract. | #728, #730 | Architecture |
| `spec/plugin-service-layering.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/plugin-sessions.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/plugin-threading.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/post-redesign-file-dispositions.md` | Keep | This frozen per-file expected disposition and proof of audit coverage; refresh after merges. | — | Architecture |
| `spec/process-confinement.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/provider-protocol-sdk.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/referenced-logging.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/runtime-entry-triggers.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/runtime-host-interfaces.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/runtime-inspection.md` | Refactor | Add resolved graph/kind origin and provenance inspection after graph patch migration. | #729, #730 | Architecture |
| `spec/runtime-topology-generation.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/rust-library-ownership-audit.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/scheduled-work.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/secrets.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/selectable-generations.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/skill-discovery.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/spec-coverage-lifecycle.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/token-efficiency.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/transcript-streaming-and-admission.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/transport-socket.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/typed-structural-boundaries.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/unified-semantic-code-query.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/value-capability-sdk.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/web-access.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/worker-pr-runtime.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/worker-profile-runtime.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/worker-results-verification.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/worker-task-dag.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/workspace-execution.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |
| `spec/workspace-persistence.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | Architecture |

## Before closing a cleanup PR

A cleanup PR must record for every changed file:

1. Evidence of its current callers, imports, package role and configuration/profile selections.
2. Expected replacement (or a reason no replacement is needed) and exact transitive dependency/consumer changes.
3. Parity tests for functionality, errors, authority, event delivery, generation pinning and protocol compatibility as applicable.
4. Updated `Cargo.toml`/`Cargo.lock`, Nix package exports and wrappers, profiles, docs, CI and integration fixtures.
5. A refreshed complete inventory from `git ls-files` with `Keep/Refactor/Move/Retire/Review` dispositions and no missing or duplicate rows.
6. A documented reason for every `Review` row resolved or explicitly carried forward. **CI green is not sufficient evidence of semantic completion.**

### Snapshot verification

The source snapshot above was derived from the complete non-truncated Git tree, with one row for each blob. It will become stale when implementation PRs land. The cleanup implementer should independently reproduce and compare paths using a tracked-file listing, excluding no category by default; the table must be regenerated when the tree changes. This is **classification coverage**, not a claim that all file bodies have received semantic review.
