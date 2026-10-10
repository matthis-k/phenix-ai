# Repository file audit matrix

Source: `main` nontruncated Git tree `d488c4056ee334938c2ca88aec9017379a3ef7c4` at tracker creation, 2026-10-10. The [merged provisional inventory](../post-redesign-file-dispositions.md) gives the expected disposition and explanation for 592 prior files. Four files added after that snapshot are identified as `New`. Each file below is individually actionable. `TODO` means **not yet verified**, including files marked Keep. An earlier expected classification is not proof of current use or safety to delete.

## Updating each row

Use the immutable F ID; never renumber. Change state `TODO -> IN PROGRESS -> PROVEN` or `REMOVED`. For PROVEN, fill the decision/evidence cell with **keep/refactor/move/retire**, replacement PR/commit, consumer check and current-head test URL as applicable. A retain decision needs a source/use explanation. A removed file stays listed as REMOVED. Add newly tracked paths with the next unused F ID, even if alphabetic order changes. Close each row only after checking its exact path. Refresh against `git ls-files`; don't silently drop new paths.

Initial coverage: 596 main-tree files. Baseline audit: 592 paths. New since audit: 4. Every initial row starts TODO because the earlier design classification did not prove liveness.


### `.githooks/`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0001 | IN PROGRESS | `.githooks/pre-commit` | Review | Determine whether it duplicates canonical Nix-owned commit checks. | Nix tooling parity | Current hook calls Nix maintenance `fix`, re-stages only original paths and runs staged whitespace check. `modules/development.nix` generates this hook and checks `diff -u` against the tracked file. Candidate KEEP; run exact-head `phenix-maintenance-git-hooks` equivalence and hook test before PROVEN. | 

### `.github/`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0002 | TODO | `.github/workflows/ci.yml` | Refactor | Keep required CI workflow, audit duplicated shards, maintenance and tests after topology migration. | #731 + CI baseline | pending | 
| F0003 | TODO | `.github/workflows/dependency-security.yml` | Refactor | Keep required CI workflow, audit duplicated shards, maintenance and tests after topology migration. | #731 + CI baseline | pending | 
| F0004 | TODO | `.github/workflows/shared-binary-cache.yml` | Refactor | Keep required CI workflow, audit duplicated shards, maintenance and tests after topology migration. | #731 + CI baseline | pending | 
| F0005 | TODO | `.github/workflows/sync-maintenance.yml` | Refactor | Keep required CI workflow, audit duplicated shards, maintenance and tests after topology migration. | #731 + CI baseline | pending | 
| F0006 | TODO | `.github/workflows/worker-executor.yml` | Refactor | Keep required CI workflow, audit duplicated shards, maintenance and tests after topology migration. | #731 + CI baseline | pending | 

### `root files`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0007 | PROVEN | `.gitignore` | Keep | Repository generated-content hygiene. | — | KEEP. Direct source review on main `d488c405`, blob `b5e60517`: excludes Nix `result*`, `/rust/target/`, `.direnv`, cache and log files. Git-only hygiene; no runtime replacement or test needed. | 
| F0008 | TODO | `AGENTS.md` | Keep | Repo operator/developer or architecture guidance; update stale references after migration. | #731 + docs sync | pending | 
| F0009 | TODO | `DEVELOPMENT.md` | Keep | Repo operator/developer or architecture guidance; update stale references after migration. | #731 + docs sync | pending | 
| F0010 | PROVEN | `LICENSE` | Keep | Repository licensing. | — | KEEP. Direct source review on main `d488c405`, blob `831ecf00`: MIT grant, 2026 copyright, attribution and redistribution notice remain part of repository distribution. No runtime substitute or CI test applies. | 
| F0011 | TODO | `README.md` | Keep | Repo operator/developer or architecture guidance; update stale references after migration. | #731 + docs sync | pending | 

### `config/`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0012 | TODO | `config/phenix/NOTICE.md` | Refactor | Retain portable product config; remove Nix-only feature selection and generated semantics. | #731 | pending | 
| F0013 | TODO | `config/phenix/runtime.nix` | Refactor | Retain portable product config; remove Nix-only feature selection and generated semantics. | #731 | pending | 

### `root files`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0014 | TODO | `flake.lock` | Keep | Canonical dependency/package metadata; synchronize when moving or retiring crates. | — | pending | 
| F0015 | TODO | `flake.nix` | Keep | Canonical dependency/package metadata; synchronize when moving or retiring crates. | — | pending | 
| F0016 | TODO | `glossary.md` | Keep | Repo operator/developer or architecture guidance; update stale references after migration. | #731 + docs sync | pending | 

### `modules/`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0017 | IN PROGRESS | `modules/cargo-source.nix` | Review (new) | Inspect source-dependency pin and its Nix consumers; retain only if not duplicating package derivations. | CI source derivation parity | Live consumer paths: `modules/rust-artifacts.nix`, `modules/package-sets.nix`, `modules/harness-product.nix`, `modules/lua-binding-integration.nix`. Source `9941dcf2` derives local Rust dependency closure from Cargo manifests. Candidate KEEP; compare Nix closure and cached rebuild behavior before certification. | 
| F0018 | IN PROGRESS | `modules/commit-candidate.nix` | Keep | Retain Nix deployment/build capability; no runtime semantic authority. | — | Imported by `modules/development.nix`. Source `c138bc02` defines staged-only `maintenance commit`, with success, unrelated-edit and untracked-file fixture cases. Candidate KEEP; verify current-head Nix test target and hook interaction before certification. | 
| F0019 | IN PROGRESS | `modules/development.nix` | Keep | Retain Nix deployment/build capability; no runtime semantic authority. | — | Owns maintenance registration, generated-hook parity check and safety-policy check used by the dev/CI entry. Review production shell/check dependency closure and execute exact-head Nix checks before KEEP proof. | 
| F0020 | IN PROGRESS | `modules/flake-module.nix` | Keep | Retain Nix deployment/build capability; no runtime semantic authority. | — | Source `9451a73c` exports `phenixWrapped` for phenix, kernel, runtime, harness and Stitch packages; imported as default flake module. Candidate KEEP; verify these exports by flake evaluation and consumer checks. | 
| F0021 | TODO | `modules/harness-product.nix` | Refactor | Thin portable product config wrapper; remove duplicated product selection. | #731 | pending | 
| F0022 | TODO | `modules/lua-binding-integration.nix` | Keep | Retain Nix deployment/build capability; no runtime semantic authority. | — | pending | 
| F0023 | TODO | `modules/package-sets.nix` | Refactor | Remove hardcoded plugin-name identity map and migration aliases after portable profile selection. | #730, #731 | pending | 
| F0024 | TODO | `modules/plugin-packaging.nix` | Refactor | Nix packages deployment artifacts, not tool/skill resolution or provider selection authority. | #730, #731 | pending | 
| F0025 | TODO | `modules/rust-artifacts.nix` | Keep | Retain Nix deployment/build capability; no runtime semantic authority. | — | pending | 
| F0026 | IN PROGRESS | `modules/stitch.nix` | Review | Establish its live package consumer and unique responsibility before retaining/removing. | consumer + CI references | Live consumer confirmed: `flake.nix` imports `modules/stitch.nix`; it exports `stitch`/`stitch-mcp` packages/apps. `modules/development.nix` invokes its `stitch-runtime-smoke` check. Source `ea7d94c4`. Candidate KEEP, not dead code; pending exact-head Nix smoke and MCP package check. | 

### `rust/`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0027 | TODO | `rust/Cargo.lock` | Keep | Canonical dependency/package metadata; synchronize when moving or retiring crates. | — | pending | 
| F0028 | TODO | `rust/Cargo.toml` | Keep | Canonical dependency/package metadata; synchronize when moving or retiring crates. | — | pending | 

### `rust/crates/phenix-acp-stdio`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0029 | TODO | `rust/crates/phenix-acp-stdio/Cargo.toml` | Keep | ACP stdio application and transport boundary; independent from Core. | — | pending | 
| F0030 | TODO | `rust/crates/phenix-acp-stdio/examples/observable_callback_fixture.rs` | Keep | ACP stdio application and transport boundary; independent from Core. | — | pending | 
| F0031 | TODO | `rust/crates/phenix-acp-stdio/src/client_tools.rs` | Keep | ACP stdio application and transport boundary; independent from Core. | — | pending | 
| F0032 | TODO | `rust/crates/phenix-acp-stdio/src/lib.rs` | Keep | ACP stdio application and transport boundary; independent from Core. | — | pending | 
| F0033 | TODO | `rust/crates/phenix-acp-stdio/src/transport.rs` | Keep | ACP stdio application and transport boundary; independent from Core. | — | pending | 
| F0034 | TODO | `rust/crates/phenix-acp-stdio/tests/client_callback_retirement.rs` | Keep | ACP stdio application and transport boundary; independent from Core. | — | pending | 
| F0035 | TODO | `rust/crates/phenix-acp-stdio/tests/client_tool_reconnect.rs` | Keep | ACP stdio application and transport boundary; independent from Core. | — | pending | 
| F0036 | TODO | `rust/crates/phenix-acp-stdio/tests/nested_client_callable_input.rs` | Keep | ACP stdio application and transport boundary; independent from Core. | — | pending | 

### `rust/crates/phenix-adapter-acp`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0037 | TODO | `rust/crates/phenix-adapter-acp/Cargo.toml` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | pending | 
| F0038 | TODO | `rust/crates/phenix-adapter-acp/src/callbacks.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | pending | 
| F0039 | TODO | `rust/crates/phenix-adapter-acp/src/dispatch.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | pending | 
| F0040 | TODO | `rust/crates/phenix-adapter-acp/src/elicitation.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | pending | 
| F0041 | TODO | `rust/crates/phenix-adapter-acp/src/errors.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | pending | 
| F0042 | TODO | `rust/crates/phenix-adapter-acp/src/extension_callbacks.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | pending | 
| F0043 | TODO | `rust/crates/phenix-adapter-acp/src/extension_dispatch.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | pending | 
| F0044 | TODO | `rust/crates/phenix-adapter-acp/src/extensions.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | pending | 
| F0045 | TODO | `rust/crates/phenix-adapter-acp/src/lib.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | pending | 
| F0046 | TODO | `rust/crates/phenix-adapter-acp/src/updates.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | pending | 
| F0047 | TODO | `rust/crates/phenix-adapter-acp/tests/application_dispatch.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | pending | 
| F0048 | TODO | `rust/crates/phenix-adapter-acp/tests/capabilities.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | pending | 
| F0049 | TODO | `rust/crates/phenix-adapter-acp/tests/disconnect.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | pending | 
| F0050 | TODO | `rust/crates/phenix-adapter-acp/tests/runtime_plugin.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | pending | 
| F0051 | TODO | `rust/crates/phenix-adapter-acp/tests/session_lifecycle.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | pending | 

### `rust/crates/phenix-agent-configurations`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0052 | TODO | `rust/crates/phenix-agent-configurations/Cargo.toml` | Refactor | Retain pure product-profile declarations; drop old plugin identity selections when replacements land. | #731 | pending | 
| F0053 | TODO | `rust/crates/phenix-agent-configurations/src/lib.rs` | Refactor | Advanced defaults currently select legacy phenix.hooks despite spec/kernel-hooks.md retirement contract; replace profile selection. | #731 + hook parity | pending | 

### `rust/crates/phenix-application-interface`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0054 | TODO | `rust/crates/phenix-application-interface/Cargo.toml` | Keep | Portable frontend and application contracts. | — | pending | 
| F0055 | TODO | `rust/crates/phenix-application-interface/fixtures/application.rs` | Keep | Portable frontend and application contracts. | — | pending | 
| F0056 | TODO | `rust/crates/phenix-application-interface/src/bin/phenix-application-descriptor.rs` | Keep | Portable frontend and application contracts. | — | pending | 
| F0057 | TODO | `rust/crates/phenix-application-interface/src/catalog.rs` | Keep | Portable frontend and application contracts. | — | pending | 
| F0058 | TODO | `rust/crates/phenix-application-interface/src/client.rs` | Keep | Portable frontend and application contracts. | — | pending | 
| F0059 | TODO | `rust/crates/phenix-application-interface/src/descriptor.rs` | Keep | Portable frontend and application contracts. | — | pending | 
| F0060 | TODO | `rust/crates/phenix-application-interface/src/generate.rs` | Keep | Portable frontend and application contracts. | — | pending | 
| F0061 | TODO | `rust/crates/phenix-application-interface/src/lib.rs` | Keep | Portable frontend and application contracts. | — | pending | 
| F0062 | TODO | `rust/crates/phenix-application-interface/src/tests.rs` | Keep | Portable frontend and application contracts. | — | pending | 
| F0063 | TODO | `rust/crates/phenix-application-interface/src/types/discovery.rs` | Keep | Portable frontend and application contracts. | — | pending | 
| F0064 | TODO | `rust/crates/phenix-application-interface/src/types/interaction.rs` | Keep | Portable frontend and application contracts. | — | pending | 
| F0065 | TODO | `rust/crates/phenix-application-interface/src/types/log.rs` | Keep | Portable frontend and application contracts. | — | pending | 
| F0066 | TODO | `rust/crates/phenix-application-interface/src/types/mod.rs` | Keep | Portable frontend and application contracts. | — | pending | 
| F0067 | TODO | `rust/crates/phenix-application-interface/src/types/observable.rs` | Keep | Portable frontend and application contracts. | — | pending | 
| F0068 | TODO | `rust/crates/phenix-application-interface/src/types/session.rs` | Keep | Portable frontend and application contracts. | — | pending | 

### `rust/crates/phenix-binding-generator`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0069 | TODO | `rust/crates/phenix-binding-generator/Cargo.toml` | Keep | Generate typed foreign-language bindings from declared interfaces. | — | pending | 
| F0070 | TODO | `rust/crates/phenix-binding-generator/src/lib.rs` | Keep | Generate typed foreign-language bindings from declared interfaces. | — | pending | 

### `rust/crates/phenix-binding-lua`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0071 | TODO | `rust/crates/phenix-binding-lua/Cargo.toml` | Keep | Lua/Neovim integration must remain behind the client boundary. | — | pending | 
| F0072 | TODO | `rust/crates/phenix-binding-lua/src/facade.rs` | Keep | Lua/Neovim integration must remain behind the client boundary. | — | pending | 
| F0073 | TODO | `rust/crates/phenix-binding-lua/src/lib.rs` | Keep | Lua/Neovim integration must remain behind the client boundary. | — | pending | 
| F0074 | TODO | `rust/crates/phenix-binding-lua/src/tools.rs` | Keep | Lua/Neovim integration must remain behind the client boundary. | — | pending | 
| F0075 | TODO | `rust/crates/phenix-binding-lua/src/tools_regression.rs` | Keep | Lua/Neovim integration must remain behind the client boundary. | — | pending | 

### `rust/crates/phenix-client-acp`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0076 | TODO | `rust/crates/phenix-client-acp/Cargo.toml` | Keep | Typed ACP client implementation. | — | pending | 
| F0077 | TODO | `rust/crates/phenix-client-acp/build.rs` | Keep | Typed ACP client implementation. | — | pending | 
| F0078 | TODO | `rust/crates/phenix-client-acp/src/lib.rs` | Keep | Typed ACP client implementation. | — | pending | 

### `rust/crates/phenix-client`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0079 | TODO | `rust/crates/phenix-client/Cargo.toml` | Keep | Transport-neutral application client DTOs. | — | pending | 
| F0080 | TODO | `rust/crates/phenix-client/src/lib.rs` | Keep | Transport-neutral application client DTOs. | — | pending | 

### `rust/crates/phenix-contract`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0081 | TODO | `rust/crates/phenix-contract/Cargo.toml` | Keep | Versioned portable types and canonical typed contributions. | — | pending | 
| F0082 | TODO | `rust/crates/phenix-contract/src/contract.rs` | Keep | Versioned portable types and canonical typed contributions. | — | pending | 
| F0083 | TODO | `rust/crates/phenix-contract/src/contract_wire.rs` | Keep | Versioned portable types and canonical typed contributions. | — | pending | 
| F0084 | TODO | `rust/crates/phenix-contract/src/identity.rs` | Keep | Versioned portable types and canonical typed contributions. | — | pending | 
| F0085 | TODO | `rust/crates/phenix-contract/src/infallible_value.rs` | Keep | Versioned portable types and canonical typed contributions. | — | pending | 
| F0086 | TODO | `rust/crates/phenix-contract/src/interface.rs` | Keep | Versioned portable types and canonical typed contributions. | — | pending | 
| F0087 | TODO | `rust/crates/phenix-contract/src/lib.rs` | Keep | Versioned portable types and canonical typed contributions. | — | pending | 
| F0088 | TODO | `rust/crates/phenix-contract/src/std_value.rs` | Keep | Versioned portable types and canonical typed contributions. | — | pending | 
| F0089 | TODO | `rust/crates/phenix-contract/src/structural_value.rs` | Keep | Versioned portable types and canonical typed contributions. | — | pending | 

### `rust/crates/phenix-core`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0090 | TODO | `rust/crates/phenix-core/Cargo.toml` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0091 | TODO | `rust/crates/phenix-core/src/agent.rs` | Move | AI-only inference/tool/skill/context contracts belong in application contract package, not Core. | #731 + references | pending | 
| F0092 | TODO | `rust/crates/phenix-core/src/artifact.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0093 | TODO | `rust/crates/phenix-core/src/authority.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0094 | TODO | `rust/crates/phenix-core/src/callable.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0095 | TODO | `rust/crates/phenix-core/src/component_endpoint_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0096 | TODO | `rust/crates/phenix-core/src/component_reentrancy_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0097 | TODO | `rust/crates/phenix-core/src/composition/activation.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0098 | TODO | `rust/crates/phenix-core/src/composition/component.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0099 | TODO | `rust/crates/phenix-core/src/composition/component_invocation.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0100 | TODO | `rust/crates/phenix-core/src/composition/inspection.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0101 | TODO | `rust/crates/phenix-core/src/composition/manifest.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0102 | TODO | `rust/crates/phenix-core/src/composition/mod.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0103 | TODO | `rust/crates/phenix-core/src/composition/provider_resolution.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0104 | TODO | `rust/crates/phenix-core/src/composition/registry.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0105 | TODO | `rust/crates/phenix-core/src/composition/resolver.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0106 | TODO | `rust/crates/phenix-core/src/configuration/mod.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0107 | TODO | `rust/crates/phenix-core/src/configuration/resolution.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0108 | TODO | `rust/crates/phenix-core/src/configuration_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0109 | TODO | `rust/crates/phenix-core/src/content_reference.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0110 | TODO | `rust/crates/phenix-core/src/events.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0111 | TODO | `rust/crates/phenix-core/src/host_authority_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0112 | TODO | `rust/crates/phenix-core/src/invalid_candidate_activation_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0113 | TODO | `rust/crates/phenix-core/src/invocation.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0114 | TODO | `rust/crates/phenix-core/src/layer_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0115 | TODO | `rust/crates/phenix-core/src/lib.rs` | Refactor | Retain generic Core public API, remove AI-specific reexports after domain contracts migrate. | #731 | pending | 
| F0116 | TODO | `rust/crates/phenix-core/src/logging.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0117 | TODO | `rust/crates/phenix-core/src/metadata/composition.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0118 | TODO | `rust/crates/phenix-core/src/metadata/frontend.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0119 | TODO | `rust/crates/phenix-core/src/metadata/input.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0120 | TODO | `rust/crates/phenix-core/src/metadata/inspection.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0121 | TODO | `rust/crates/phenix-core/src/metadata/mod.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0122 | TODO | `rust/crates/phenix-core/src/metadata/reconciliation.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0123 | TODO | `rust/crates/phenix-core/src/metadata_semantic_identity_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0124 | TODO | `rust/crates/phenix-core/src/observable.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0125 | TODO | `rust/crates/phenix-core/src/observable/helpers.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0126 | TODO | `rust/crates/phenix-core/src/observable/model.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0127 | TODO | `rust/crates/phenix-core/src/observable/store.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0128 | TODO | `rust/crates/phenix-core/src/observable/transaction_canonical.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0129 | TODO | `rust/crates/phenix-core/src/observable/transaction_finish.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0130 | TODO | `rust/crates/phenix-core/src/observable/transaction_mutate.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0131 | TODO | `rust/crates/phenix-core/src/observable/transaction_types.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0132 | TODO | `rust/crates/phenix-core/src/persistence/backend.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0133 | TODO | `rust/crates/phenix-core/src/persistence/bootstrap.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0134 | TODO | `rust/crates/phenix-core/src/persistence/mod.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0135 | TODO | `rust/crates/phenix-core/src/persistence/provider.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0136 | TODO | `rust/crates/phenix-core/src/persistence/provider/tests.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0137 | TODO | `rust/crates/phenix-core/src/persistence/value.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0138 | TODO | `rust/crates/phenix-core/src/plugin/build.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0139 | TODO | `rust/crates/phenix-core/src/plugin/build_execution.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0140 | TODO | `rust/crates/phenix-core/src/plugin/context.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0141 | TODO | `rust/crates/phenix-core/src/plugin/management.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0142 | TODO | `rust/crates/phenix-core/src/plugin/mod.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0143 | TODO | `rust/crates/phenix-core/src/plugin/prepared_mutation.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0144 | TODO | `rust/crates/phenix-core/src/plugin_build_loading_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0145 | TODO | `rust/crates/phenix-core/src/plugin_management_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0146 | TODO | `rust/crates/phenix-core/src/plugin_runtime_adapter_host_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0147 | TODO | `rust/crates/phenix-core/src/plugin_runtime_adapter_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0148 | TODO | `rust/crates/phenix-core/src/provider_availability_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0149 | TODO | `rust/crates/phenix-core/src/provider_fallback_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0150 | TODO | `rust/crates/phenix-core/src/provider_rebind_generation_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0151 | TODO | `rust/crates/phenix-core/src/reconciliation/graph.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0152 | TODO | `rust/crates/phenix-core/src/reconciliation/inspection.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0153 | TODO | `rust/crates/phenix-core/src/reconciliation/live.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0154 | TODO | `rust/crates/phenix-core/src/reconciliation/mod.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0155 | TODO | `rust/crates/phenix-core/src/runtime.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0156 | TODO | `rust/crates/phenix-core/src/runtime/dispatch.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0157 | TODO | `rust/crates/phenix-core/src/runtime/host.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0158 | TODO | `rust/crates/phenix-core/src/runtime/kernel.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0159 | TODO | `rust/crates/phenix-core/src/runtime/listener.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0160 | TODO | `rust/crates/phenix-core/src/runtime/owned_transactions.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0161 | TODO | `rust/crates/phenix-core/src/runtime/persistence_bootstrap.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0162 | TODO | `rust/crates/phenix-core/src/runtime/reconciliation.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0163 | TODO | `rust/crates/phenix-core/src/runtime/residency.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0164 | TODO | `rust/crates/phenix-core/src/runtime/tests.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0165 | TODO | `rust/crates/phenix-core/src/runtime/trace.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0166 | TODO | `rust/crates/phenix-core/src/runtime_component_parity_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0167 | TODO | `rust/crates/phenix-core/src/runtime_topology_generation_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0168 | TODO | `rust/crates/phenix-core/src/sdk.rs` | Review | Generic SDK observable machinery may remain in Core; separate public SDK integration from kernel authority. | #728 + API ownership review | pending | 
| F0169 | TODO | `rust/crates/phenix-core/src/service_layer_dispatch_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0170 | TODO | `rust/crates/phenix-core/src/tasks.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | pending | 
| F0171 | TODO | `rust/crates/phenix-core/src/third_party_component_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0172 | TODO | `rust/crates/phenix-core/tests/event_delivery_contract.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0173 | TODO | `rust/crates/phenix-core/tests/event_generation_provenance.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0174 | TODO | `rust/crates/phenix-core/tests/kernel_concurrency_contract.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0175 | TODO | `rust/crates/phenix-core/tests/observable_allocations.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0176 | TODO | `rust/crates/phenix-core/tests/observable_semantics.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0177 | TODO | `rust/crates/phenix-core/tests/persistence_backend_conformance.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 
| F0178 | TODO | `rust/crates/phenix-core/tests/resource_metadata_reconciliation.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | pending | 

### `rust/crates/phenix-domain`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0179 | TODO | `rust/crates/phenix-domain/Cargo.toml` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | pending | 
| F0180 | TODO | `rust/crates/phenix-domain/src/attempts.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | pending | 
| F0181 | TODO | `rust/crates/phenix-domain/src/client_tools.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | pending | 
| F0182 | TODO | `rust/crates/phenix-domain/src/debug.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | pending | 
| F0183 | TODO | `rust/crates/phenix-domain/src/delegation.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | pending | 
| F0184 | TODO | `rust/crates/phenix-domain/src/delegation/tests.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | pending | 
| F0185 | TODO | `rust/crates/phenix-domain/src/failures.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | pending | 
| F0186 | TODO | `rust/crates/phenix-domain/src/lib.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | pending | 
| F0187 | TODO | `rust/crates/phenix-domain/src/workspace/context.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | pending | 
| F0188 | TODO | `rust/crates/phenix-domain/src/workspace/decisions.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | pending | 
| F0189 | TODO | `rust/crates/phenix-domain/src/workspace/language.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | pending | 
| F0190 | TODO | `rust/crates/phenix-domain/src/workspace/language_operations.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | pending | 
| F0191 | TODO | `rust/crates/phenix-domain/src/workspace/mod.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | pending | 
| F0192 | TODO | `rust/crates/phenix-domain/src/workspace/objectives.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | pending | 
| F0193 | TODO | `rust/crates/phenix-domain/src/workspace/plans.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | pending | 
| F0194 | TODO | `rust/crates/phenix-domain/tests/context_serialization.rs` | Refactor | Retain client/application domain contracts outside Core; split independent ownership later. | #731 | pending | 

### `rust/crates/phenix-harness`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0195 | TODO | `rust/crates/phenix-harness/Cargo.toml` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | pending | 
| F0196 | TODO | `rust/crates/phenix-harness/src/application.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | pending | 
| F0197 | TODO | `rust/crates/phenix-harness/src/authority.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | pending | 
| F0198 | TODO | `rust/crates/phenix-harness/src/basic_suite.rs` | Move | Move hardcoded echo/basic product setup to data-driven reference profiles and fixtures. | #731 | pending | 
| F0199 | TODO | `rust/crates/phenix-harness/src/bin/phenix-acp-fixture.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | pending | 
| F0200 | TODO | `rust/crates/phenix-harness/src/context_recovery.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | pending | 
| F0201 | TODO | `rust/crates/phenix-harness/src/exact_selected_suite_tests.rs` | Refactor | Keep product parity assertions but eliminate fixtures coupled to old hardcoded plugin registration. | #731 | pending | 
| F0202 | TODO | `rust/crates/phenix-harness/src/lib.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | pending | 
| F0203 | TODO | `rust/crates/phenix-harness/src/main.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | pending | 
| F0204 | TODO | `rust/crates/phenix-harness/src/model_surface_fixture.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | pending | 
| F0205 | TODO | `rust/crates/phenix-harness/src/persistence.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | pending | 
| F0206 | TODO | `rust/crates/phenix-harness/src/runtime.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | pending | 
| F0207 | TODO | `rust/crates/phenix-harness/src/runtime_builder.rs` | Refactor | Manual add_selected/plugin-factory roster should lower from selected manifests/contributions. | #728, #730, #731 | pending | 
| F0208 | TODO | `rust/crates/phenix-harness/src/runtime_config.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | pending | 
| F0209 | TODO | `rust/crates/phenix-harness/src/tests.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | pending | 
| F0210 | TODO | `rust/crates/phenix-harness/src/workspace_discovery.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | pending | 
| F0211 | TODO | `rust/crates/phenix-harness/tests/acp_session_lifecycle.rs` | Refactor | Keep product parity assertions but eliminate fixtures coupled to old hardcoded plugin registration. | #731 | pending | 
| F0212 | TODO | `rust/crates/phenix-harness/tests/application_projection.rs` | Refactor | Keep product parity assertions but eliminate fixtures coupled to old hardcoded plugin registration. | #731 | pending | 
| F0213 | TODO | `rust/crates/phenix-harness/tests/application_review.rs` | Refactor | Keep product parity assertions but eliminate fixtures coupled to old hardcoded plugin registration. | #731 | pending | 
| F0214 | TODO | `rust/crates/phenix-harness/tests/component_graph.rs` | Refactor | Keep product parity assertions but eliminate fixtures coupled to old hardcoded plugin registration. | #731 | pending | 
| F0215 | TODO | `rust/crates/phenix-harness/tests/invocation_defaults.rs` | Refactor | Keep product parity assertions but eliminate fixtures coupled to old hardcoded plugin registration. | #731 | pending | 
| F0216 | TODO | `rust/crates/phenix-harness/tests/process_roundtrip.rs` | Refactor | Keep product parity assertions but eliminate fixtures coupled to old hardcoded plugin registration. | #731 | pending | 
| F0217 | TODO | `rust/crates/phenix-harness/tests/supported_product_journeys.rs` | Refactor | Keep product parity assertions but eliminate fixtures coupled to old hardcoded plugin registration. | #731 | pending | 

### `rust/crates/phenix-model-adapter-acp`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0218 | TODO | `rust/crates/phenix-model-adapter-acp/Cargo.toml` | Keep | ACP model-provider adapter implementation. | — | pending | 
| F0219 | TODO | `rust/crates/phenix-model-adapter-acp/src/lib.rs` | Keep | ACP model-provider adapter implementation. | — | pending | 
| F0220 | TODO | `rust/crates/phenix-model-adapter-acp/src/mcp_bridge.rs` | Keep | ACP model-provider adapter implementation. | — | pending | 
| F0221 | TODO | `rust/crates/phenix-model-adapter-acp/tests/fixtures/acp_continuity_agent.rs` | Keep | ACP model-provider adapter implementation. | — | pending | 
| F0222 | TODO | `rust/crates/phenix-model-adapter-acp/tests/fixtures/acp_tool_bridge_agent.rs` | Keep | ACP model-provider adapter implementation. | — | pending | 
| F0223 | TODO | `rust/crates/phenix-model-adapter-acp/tests/persistent_continuity.rs` | Keep | ACP model-provider adapter implementation. | — | pending | 
| F0224 | TODO | `rust/crates/phenix-model-adapter-acp/tests/tool_bridge.rs` | Keep | ACP model-provider adapter implementation. | — | pending | 

### `rust/crates/phenix-model-adapter-native`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0225 | TODO | `rust/crates/phenix-model-adapter-native/Cargo.toml` | Keep | Native model-provider adapter implementation. | — | pending | 
| F0226 | TODO | `rust/crates/phenix-model-adapter-native/src/credentials.rs` | Keep | Native model-provider adapter implementation. | — | pending | 
| F0227 | TODO | `rust/crates/phenix-model-adapter-native/src/lib.rs` | Keep | Native model-provider adapter implementation. | — | pending | 
| F0228 | TODO | `rust/crates/phenix-model-adapter-native/src/oauth.rs` | Keep | Native model-provider adapter implementation. | — | pending | 
| F0229 | TODO | `rust/crates/phenix-model-adapter-native/src/providers.rs` | Keep | Native model-provider adapter implementation. | — | pending | 
| F0230 | TODO | `rust/crates/phenix-model-adapter-native/src/schema_adapter.rs` | Keep | Native model-provider adapter implementation. | — | pending | 

### `rust/crates/phenix-model-adapter`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0231 | TODO | `rust/crates/phenix-model-adapter/Cargo.toml` | Keep | Shared model-adapter application contract, never required by Core. | — | pending | 
| F0232 | TODO | `rust/crates/phenix-model-adapter/src/lib.rs` | Keep | Shared model-adapter application contract, never required by Core. | — | pending | 

### `rust/crates/phenix-plugin-api`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0233 | TODO | `rust/crates/phenix-plugin-api/Cargo.toml` | Refactor | Retain application-facing API, but use tool/skill kind-provider contracts instead of duplicated catalogs. | #730, #731 | pending | 
| F0234 | TODO | `rust/crates/phenix-plugin-api/src/lib.rs` | Refactor | Retain application-facing API, but use tool/skill kind-provider contracts instead of duplicated catalogs. | #730, #731 | pending | 
| F0235 | TODO | `rust/crates/phenix-plugin-api/src/tests.rs` | Refactor | Retain application-facing API, but use tool/skill kind-provider contracts instead of duplicated catalogs. | #730, #731 | pending | 

### `rust/crates/phenix-plugin-artifacts`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0236 | TODO | `rust/crates/phenix-plugin-artifacts/Cargo.toml` | Keep | Artifact-specific plugin behavior, outside the kernel. | — | pending | 
| F0237 | TODO | `rust/crates/phenix-plugin-artifacts/src/component.rs` | Keep | Artifact-specific plugin behavior, outside the kernel. | — | pending | 
| F0238 | TODO | `rust/crates/phenix-plugin-artifacts/src/implementation.rs` | Keep | Artifact-specific plugin behavior, outside the kernel. | — | pending | 
| F0239 | TODO | `rust/crates/phenix-plugin-artifacts/src/lib.rs` | Keep | Artifact-specific plugin behavior, outside the kernel. | — | pending | 

### `rust/crates/phenix-plugin-basic-agent`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0240 | TODO | `rust/crates/phenix-plugin-basic-agent/Cargo.toml` | Refactor | Retain only if new topology/node provider crate reuses package; remove obsolete loop package afterward. | #726, #731 | pending | 
| F0241 | TODO | `rust/crates/phenix-plugin-basic-agent/src/agent_loop.rs` | Retire | Replace imperative run progression with topology plugin and independently provided nodes; preserve parity tests. | #726, #731 | pending | 
| F0242 | TODO | `rust/crates/phenix-plugin-basic-agent/src/component_regression.rs` | Move | Rehome valuable agent-loop behavior proofs in the topology/node integration suite. | #726, #731 | pending | 
| F0243 | TODO | `rust/crates/phenix-plugin-basic-agent/src/lib.rs` | Refactor | Preserve agent-loop semantics in topology/node plugins; remove imperative progression after parity. | #726, #731 | pending | 

### `rust/crates/phenix-plugin-basic-context`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0244 | TODO | `rust/crates/phenix-plugin-basic-context/Cargo.toml` | Keep | Minimal replaceable context provider useful as a baseline and fixture. | — | pending | 
| F0245 | TODO | `rust/crates/phenix-plugin-basic-context/src/lib.rs` | Keep | Minimal replaceable context provider useful as a baseline and fixture. | — | pending | 

### `rust/crates/phenix-plugin-basic-model`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0246 | TODO | `rust/crates/phenix-plugin-basic-model/Cargo.toml` | Retire | Delete production crate manifest after echo implementation and references move to fixtures. | #731 + migrate echo fixture | pending | 
| F0247 | TODO | `rust/crates/phenix-plugin-basic-model/src/lib.rs` | Move | Preserve deterministic echo implementation as test-support fixture, not a production plugin. | #731 | pending | 

### `rust/crates/phenix-plugin-basic-skills`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0248 | TODO | `rust/crates/phenix-plugin-basic-skills/Cargo.toml` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | pending | 
| F0249 | TODO | `rust/crates/phenix-plugin-basic-skills/skills/architect/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | pending | 
| F0250 | TODO | `rust/crates/phenix-plugin-basic-skills/skills/grilling/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | pending | 
| F0251 | TODO | `rust/crates/phenix-plugin-basic-skills/skills/implement/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | pending | 
| F0252 | TODO | `rust/crates/phenix-plugin-basic-skills/skills/interrogate/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | pending | 
| F0253 | TODO | `rust/crates/phenix-plugin-basic-skills/skills/investigate/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | pending | 
| F0254 | TODO | `rust/crates/phenix-plugin-basic-skills/skills/pickup/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | pending | 
| F0255 | TODO | `rust/crates/phenix-plugin-basic-skills/skills/plan/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | pending | 
| F0256 | TODO | `rust/crates/phenix-plugin-basic-skills/skills/pstack-LICENSE` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | pending | 
| F0257 | TODO | `rust/crates/phenix-plugin-basic-skills/skills/reflect/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | pending | 
| F0258 | TODO | `rust/crates/phenix-plugin-basic-skills/skills/rust/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | pending | 
| F0259 | TODO | `rust/crates/phenix-plugin-basic-skills/skills/rust/references/ecosystem.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | pending | 
| F0260 | TODO | `rust/crates/phenix-plugin-basic-skills/skills/rust/references/research.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | pending | 
| F0261 | TODO | `rust/crates/phenix-plugin-basic-skills/skills/ship/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | pending | 
| F0262 | TODO | `rust/crates/phenix-plugin-basic-skills/skills/to-questionnaire/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | pending | 
| F0263 | TODO | `rust/crates/phenix-plugin-basic-skills/skills/verify/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | pending | 
| F0264 | TODO | `rust/crates/phenix-plugin-basic-skills/skills/write/SKILL.md` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | pending | 
| F0265 | TODO | `rust/crates/phenix-plugin-basic-skills/src/lib.rs` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | pending | 

### `rust/crates/phenix-plugin-basic-tools`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0266 | TODO | `rust/crates/phenix-plugin-basic-tools/Cargo.toml` | Refactor | Become a standard plugin-defined tool kind/template consumer/provider. | #730, #731 | pending | 
| F0267 | TODO | `rust/crates/phenix-plugin-basic-tools/src/lib.rs` | Refactor | Become a standard plugin-defined tool kind/template consumer/provider. | #730, #731 | pending | 

### `rust/crates/phenix-plugin-catalog`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0268 | TODO | `rust/crates/phenix-plugin-catalog/Cargo.toml` | Retire | Delete static roster crate manifest once first-party discovery/fixtures move. | #728, #730, #731 | pending | 
| F0269 | TODO | `rust/crates/phenix-plugin-catalog/src/lib.rs` | Retire | Remove manually maintained first-party plugin re-export/registration catalog once dynamic discovery works. | #730, #731 | pending | 
| F0270 | TODO | `rust/crates/phenix-plugin-catalog/tests/coordination.rs` | Move | Move provider coordination verification into application integration tests before retiring roster. | #730, #731 | pending | 

### `rust/crates/phenix-plugin-command-toolbelt`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0271 | TODO | `rust/crates/phenix-plugin-command-toolbelt/Cargo.toml` | Keep | Concrete command implementations are separate from tool registration mechanisms. | — | pending | 
| F0272 | TODO | `rust/crates/phenix-plugin-command-toolbelt/src/component.rs` | Keep | Concrete command implementations are separate from tool registration mechanisms. | — | pending | 
| F0273 | TODO | `rust/crates/phenix-plugin-command-toolbelt/src/implementation.rs` | Keep | Concrete command implementations are separate from tool registration mechanisms. | — | pending | 
| F0274 | TODO | `rust/crates/phenix-plugin-command-toolbelt/src/lib.rs` | Keep | Concrete command implementations are separate from tool registration mechanisms. | — | pending | 

### `rust/crates/phenix-plugin-context`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0275 | TODO | `rust/crates/phenix-plugin-context/Cargo.toml` | Refactor | Retain context projection and prompt policy, remove overlapping orchestration/registration. | #731 | pending | 
| F0276 | TODO | `rust/crates/phenix-plugin-context/src/component.rs` | Refactor | Retain context projection and prompt policy, remove overlapping orchestration/registration. | #731 | pending | 
| F0277 | TODO | `rust/crates/phenix-plugin-context/src/implementation.rs` | Refactor | Retain context projection and prompt policy, remove overlapping orchestration/registration. | #731 | pending | 
| F0278 | TODO | `rust/crates/phenix-plugin-context/src/implementation_state.rs` | Refactor | Retain context projection and prompt policy, remove overlapping orchestration/registration. | #731 | pending | 
| F0279 | TODO | `rust/crates/phenix-plugin-context/src/lib.rs` | Refactor | Retain context projection and prompt policy, remove overlapping orchestration/registration. | #731 | pending | 
| F0280 | TODO | `rust/crates/phenix-plugin-context/src/materialization.rs` | Refactor | Retain context projection and prompt policy, remove overlapping orchestration/registration. | #731 | pending | 
| F0281 | TODO | `rust/crates/phenix-plugin-context/src/materialization_integration.rs` | Refactor | Retain context projection and prompt policy, remove overlapping orchestration/registration. | #731 | pending | 
| F0282 | TODO | `rust/crates/phenix-plugin-context/src/projection_state.rs` | Refactor | Retain context projection and prompt policy, remove overlapping orchestration/registration. | #731 | pending | 
| F0283 | TODO | `rust/crates/phenix-plugin-context/src/prompt.rs` | Refactor | Retain context projection and prompt policy, remove overlapping orchestration/registration. | #731 | pending | 
| F0284 | TODO | `rust/crates/phenix-plugin-context/src/state_integration.rs` | Refactor | Retain context projection and prompt policy, remove overlapping orchestration/registration. | #731 | pending | 
| F0285 | TODO | `rust/crates/phenix-plugin-context/src/state_service.rs` | Refactor | Retain context projection and prompt policy, remove overlapping orchestration/registration. | #731 | pending | 

### `rust/crates/phenix-plugin-debug`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0286 | TODO | `rust/crates/phenix-plugin-debug/Cargo.toml` | Keep | Optional inspection provider based on generic Core observability. | — | pending | 
| F0287 | TODO | `rust/crates/phenix-plugin-debug/src/component.rs` | Keep | Optional inspection provider based on generic Core observability. | — | pending | 
| F0288 | TODO | `rust/crates/phenix-plugin-debug/src/implementation.rs` | Keep | Optional inspection provider based on generic Core observability. | — | pending | 
| F0289 | TODO | `rust/crates/phenix-plugin-debug/src/lib.rs` | Keep | Optional inspection provider based on generic Core observability. | — | pending | 

### `rust/crates/phenix-plugin-efficiency-evaluation`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0290 | TODO | `rust/crates/phenix-plugin-efficiency-evaluation/Cargo.toml` | Keep | Optional benchmarking and evaluation implementation. | — | pending | 
| F0291 | TODO | `rust/crates/phenix-plugin-efficiency-evaluation/src/benchmark_outcomes.rs` | Keep | Optional benchmarking and evaluation implementation. | — | pending | 
| F0292 | TODO | `rust/crates/phenix-plugin-efficiency-evaluation/src/implementation.rs` | Keep | Optional benchmarking and evaluation implementation. | — | pending | 
| F0293 | TODO | `rust/crates/phenix-plugin-efficiency-evaluation/src/lib.rs` | Keep | Optional benchmarking and evaluation implementation. | — | pending | 

### `rust/crates/phenix-plugin-environment-local`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0294 | TODO | `rust/crates/phenix-plugin-environment-local/Cargo.toml` | Keep | Concrete local execution environment implementation. | — | pending | 
| F0295 | TODO | `rust/crates/phenix-plugin-environment-local/src/lib.rs` | Keep | Concrete local execution environment implementation. | — | pending | 

### `rust/crates/phenix-plugin-execution`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0296 | TODO | `rust/crates/phenix-plugin-execution/Cargo.toml` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | pending | 
| F0297 | TODO | `rust/crates/phenix-plugin-execution/src/agent_loop_regression.rs` | Move | Transfer behavioral assertions to resolved workflow integration tests before removing old loop. | #726, #731 | pending | 
| F0298 | TODO | `rust/crates/phenix-plugin-execution/src/attempt_integration.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | pending | 
| F0299 | TODO | `rust/crates/phenix-plugin-execution/src/attempt_service.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | pending | 
| F0300 | TODO | `rust/crates/phenix-plugin-execution/src/component.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | pending | 
| F0301 | TODO | `rust/crates/phenix-plugin-execution/src/configuration.rs` | Refactor | OrchestrationDefinition/Node must no longer compete with canonical workflow graph declarations. | #729, #731 | pending | 
| F0302 | TODO | `rust/crates/phenix-plugin-execution/src/configuration/packaged.rs` | Refactor | Align packaged orchestration/configuration with canonical graph and product profiles. | #729, #731 | pending | 
| F0303 | TODO | `rust/crates/phenix-plugin-execution/src/configuration_regression.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | pending | 
| F0304 | TODO | `rust/crates/phenix-plugin-execution/src/delegated_task_state.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | pending | 
| F0305 | TODO | `rust/crates/phenix-plugin-execution/src/generation_regression.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | pending | 
| F0306 | TODO | `rust/crates/phenix-plugin-execution/src/implementation.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | pending | 
| F0307 | TODO | `rust/crates/phenix-plugin-execution/src/lib.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | pending | 
| F0308 | TODO | `rust/crates/phenix-plugin-execution/src/resource_integration.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | pending | 
| F0309 | TODO | `rust/crates/phenix-plugin-execution/src/resource_service.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | pending | 
| F0310 | TODO | `rust/crates/phenix-plugin-execution/src/resource_transaction.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | pending | 
| F0311 | TODO | `rust/crates/phenix-plugin-execution/src/review.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | pending | 
| F0312 | TODO | `rust/crates/phenix-plugin-execution/src/root_reservation_integration.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | pending | 
| F0313 | TODO | `rust/crates/phenix-plugin-execution/src/step_transaction_service.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | pending | 
| F0314 | TODO | `rust/crates/phenix-plugin-execution/src/tool_schedule.rs` | Move | Consolidate concurrency planning with generic graph scheduling or tool-policy plugin; avoid a second scheduler. | #729, #731 | pending | 
| F0315 | TODO | `rust/crates/phenix-plugin-execution/tests/step_transaction.rs` | Refactor | Retain application execution state, review and domain policy; migrate competing graph/scheduling machinery. | #729, #731 | pending | 

### `rust/crates/phenix-plugin-frontend`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0316 | TODO | `rust/crates/phenix-plugin-frontend/Cargo.toml` | Keep | Replaceable frontend discovery and capability provider. | — | pending | 
| F0317 | TODO | `rust/crates/phenix-plugin-frontend/src/component.rs` | Keep | Replaceable frontend discovery and capability provider. | — | pending | 
| F0318 | TODO | `rust/crates/phenix-plugin-frontend/src/implementation.rs` | Keep | Replaceable frontend discovery and capability provider. | — | pending | 
| F0319 | TODO | `rust/crates/phenix-plugin-frontend/src/lib.rs` | Keep | Replaceable frontend discovery and capability provider. | — | pending | 

### `rust/crates/phenix-plugin-hooks`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0320 | TODO | `rust/crates/phenix-plugin-hooks/Cargo.toml` | Retire | Legacy hook dispatcher; use kernel Service Layers and Events with plugin-owned behavior. | #729, #731 + listener parity | pending | 
| F0321 | TODO | `rust/crates/phenix-plugin-hooks/src/component.rs` | Retire | Legacy hook dispatcher; use kernel Service Layers and Events with plugin-owned behavior. | #729, #731 + listener parity | pending | 
| F0322 | TODO | `rust/crates/phenix-plugin-hooks/src/implementation.rs` | Retire | Legacy hook dispatcher; use kernel Service Layers and Events with plugin-owned behavior. | #729, #731 + listener parity | pending | 
| F0323 | TODO | `rust/crates/phenix-plugin-hooks/src/lib.rs` | Retire | Legacy hook dispatcher; use kernel Service Layers and Events with plugin-owned behavior. | #729, #731 + listener parity | pending | 
| F0324 | TODO | `rust/crates/phenix-plugin-hooks/src/ownership_regression.rs` | Move | Retain layer/listener ownership regression under canonical Core/SDK tests before retiring hook crate. | #729, #731 + listener parity | pending | 

### `rust/crates/phenix-plugin-interactive-ui`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0325 | TODO | `rust/crates/phenix-plugin-interactive-ui/Cargo.toml` | Keep | Portable UI document vocabulary and optional frontend contribution kind. | — | pending | 
| F0326 | TODO | `rust/crates/phenix-plugin-interactive-ui/src/lib.rs` | Keep | Portable UI document vocabulary and optional frontend contribution kind. | — | pending | 

### `rust/crates/phenix-plugin-invocation-defaults`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0327 | TODO | `rust/crates/phenix-plugin-invocation-defaults/Cargo.toml` | Refactor | Separate invocations, clock, context recovery and routing policies into replaceable providers. | #731 | pending | 
| F0328 | TODO | `rust/crates/phenix-plugin-invocation-defaults/src/lib.rs` | Refactor | Separate invocations, clock, context recovery and routing policies into replaceable providers. | #731 | pending | 

### `rust/crates/phenix-plugin-jobs`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0329 | TODO | `rust/crates/phenix-plugin-jobs/Cargo.toml` | Keep | Persistent job domain service, not a second kernel event loop. | — | pending | 
| F0330 | TODO | `rust/crates/phenix-plugin-jobs/src/component.rs` | Keep | Persistent job domain service, not a second kernel event loop. | — | pending | 
| F0331 | TODO | `rust/crates/phenix-plugin-jobs/src/implementation.rs` | Keep | Persistent job domain service, not a second kernel event loop. | — | pending | 
| F0332 | TODO | `rust/crates/phenix-plugin-jobs/src/lib.rs` | Keep | Persistent job domain service, not a second kernel event loop. | — | pending | 

### `rust/crates/phenix-plugin-language`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0333 | TODO | `rust/crates/phenix-plugin-language/Cargo.toml` | Keep | Language/code intelligence provider. | — | pending | 
| F0334 | TODO | `rust/crates/phenix-plugin-language/src/component.rs` | Keep | Language/code intelligence provider. | — | pending | 
| F0335 | TODO | `rust/crates/phenix-plugin-language/src/implementation.rs` | Keep | Language/code intelligence provider. | — | pending | 
| F0336 | TODO | `rust/crates/phenix-plugin-language/src/lib.rs` | Keep | Language/code intelligence provider. | — | pending | 

### `rust/crates/phenix-plugin-memory`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0337 | TODO | `rust/crates/phenix-plugin-memory/Cargo.toml` | Keep | Memory service and persistence/invalidation policy outside Core. | — | pending | 
| F0338 | TODO | `rust/crates/phenix-plugin-memory/src/association_store.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | pending | 
| F0339 | TODO | `rust/crates/phenix-plugin-memory/src/component.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | pending | 
| F0340 | TODO | `rust/crates/phenix-plugin-memory/src/context_service_integration.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | pending | 
| F0341 | TODO | `rust/crates/phenix-plugin-memory/src/context_service_state.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | pending | 
| F0342 | TODO | `rust/crates/phenix-plugin-memory/src/embedding_integration.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | pending | 
| F0343 | TODO | `rust/crates/phenix-plugin-memory/src/error.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | pending | 
| F0344 | TODO | `rust/crates/phenix-plugin-memory/src/freshness.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | pending | 
| F0345 | TODO | `rust/crates/phenix-plugin-memory/src/freshness_integration.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | pending | 
| F0346 | TODO | `rust/crates/phenix-plugin-memory/src/implementation.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | pending | 
| F0347 | TODO | `rust/crates/phenix-plugin-memory/src/lib.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | pending | 
| F0348 | TODO | `rust/crates/phenix-plugin-memory/src/maintenance_integration.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | pending | 
| F0349 | TODO | `rust/crates/phenix-plugin-memory/src/package.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | pending | 
| F0350 | TODO | `rust/crates/phenix-plugin-memory/src/persistence.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | pending | 
| F0351 | TODO | `rust/crates/phenix-plugin-memory/src/provenance_integration.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | pending | 
| F0352 | TODO | `rust/crates/phenix-plugin-memory/src/reranking_integration.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | pending | 
| F0353 | TODO | `rust/crates/phenix-plugin-memory/src/restart_integration.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | pending | 
| F0354 | TODO | `rust/crates/phenix-plugin-memory/src/retrieval.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | pending | 
| F0355 | TODO | `rust/crates/phenix-plugin-memory/src/revalidation_failure_integration.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | pending | 
| F0356 | TODO | `rust/crates/phenix-plugin-memory/src/supersession_integration.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | pending | 
| F0357 | TODO | `rust/crates/phenix-plugin-memory/src/tests.rs` | Keep | Memory service and persistence/invalidation policy outside Core. | — | pending | 

### `rust/crates/phenix-plugin-models`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0358 | TODO | `rust/crates/phenix-plugin-models/Cargo.toml` | Keep | Model selection, routing and provider behavior stays application-side. | — | pending | 
| F0359 | TODO | `rust/crates/phenix-plugin-models/src/component.rs` | Keep | Model selection, routing and provider behavior stays application-side. | — | pending | 
| F0360 | TODO | `rust/crates/phenix-plugin-models/src/implementation_sdk.rs` | Keep | Model selection, routing and provider behavior stays application-side. | — | pending | 
| F0361 | TODO | `rust/crates/phenix-plugin-models/src/implementation_sdk/catalog.rs` | Keep | Model selection, routing and provider behavior stays application-side. | — | pending | 
| F0362 | TODO | `rust/crates/phenix-plugin-models/src/implementation_sdk/packaged.rs` | Keep | Model selection, routing and provider behavior stays application-side. | — | pending | 
| F0363 | TODO | `rust/crates/phenix-plugin-models/src/implementation_sdk/tests.rs` | Keep | Model selection, routing and provider behavior stays application-side. | — | pending | 
| F0364 | TODO | `rust/crates/phenix-plugin-models/src/lib.rs` | Keep | Model selection, routing and provider behavior stays application-side. | — | pending | 
| F0365 | TODO | `rust/crates/phenix-plugin-models/src/routing_service.rs` | Keep | Model selection, routing and provider behavior stays application-side. | — | pending | 
| F0366 | TODO | `rust/crates/phenix-plugin-models/src/routing_state.rs` | Keep | Model selection, routing and provider behavior stays application-side. | — | pending | 

### `rust/crates/phenix-plugin-openai-codex`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0367 | TODO | `rust/crates/phenix-plugin-openai-codex/Cargo.toml` | Keep | Specific model provider integration. | — | pending | 
| F0368 | TODO | `rust/crates/phenix-plugin-openai-codex/src/lib.rs` | Keep | Specific model provider integration. | — | pending | 

### `rust/crates/phenix-plugin-options`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0369 | TODO | `rust/crates/phenix-plugin-options/Cargo.toml` | Keep | Application options provider; avoid a second kernel resolver. | — | pending | 
| F0370 | TODO | `rust/crates/phenix-plugin-options/src/lib.rs` | Keep | Application options provider; avoid a second kernel resolver. | — | pending | 

### `rust/crates/phenix-plugin-planning`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0371 | TODO | `rust/crates/phenix-plugin-planning/Cargo.toml` | Keep | Optional planning policy implementation. | — | pending | 
| F0372 | TODO | `rust/crates/phenix-plugin-planning/src/component.rs` | Keep | Optional planning policy implementation. | — | pending | 
| F0373 | TODO | `rust/crates/phenix-plugin-planning/src/implementation.rs` | Keep | Optional planning policy implementation. | — | pending | 
| F0374 | TODO | `rust/crates/phenix-plugin-planning/src/lib.rs` | Keep | Optional planning policy implementation. | — | pending | 

### `rust/crates/phenix-plugin-providers`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0375 | TODO | `rust/crates/phenix-plugin-providers/Cargo.toml` | Keep | Provider presets and registration data; avoid Nix-authoritative semantics. | — | pending | 
| F0376 | TODO | `rust/crates/phenix-plugin-providers/README.md` | Keep | Provider presets and registration data; avoid Nix-authoritative semantics. | — | pending | 
| F0377 | TODO | `rust/crates/phenix-plugin-providers/src/lib.rs` | Keep | Provider presets and registration data; avoid Nix-authoritative semantics. | — | pending | 

### `rust/crates/phenix-plugin-repository-workers`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0378 | TODO | `rust/crates/phenix-plugin-repository-workers/Cargo.toml` | Keep | Optional repository-worker orchestration built on generic Core primitives. | — | pending | 
| F0379 | TODO | `rust/crates/phenix-plugin-repository-workers/src/component.rs` | Keep | Optional repository-worker orchestration built on generic Core primitives. | — | pending | 
| F0380 | TODO | `rust/crates/phenix-plugin-repository-workers/src/implementation.rs` | Keep | Optional repository-worker orchestration built on generic Core primitives. | — | pending | 
| F0381 | TODO | `rust/crates/phenix-plugin-repository-workers/src/lib.rs` | Keep | Optional repository-worker orchestration built on generic Core primitives. | — | pending | 

### `rust/crates/phenix-plugin-session-tree`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0382 | TODO | `rust/crates/phenix-plugin-session-tree/Cargo.toml` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | pending | 
| F0383 | TODO | `rust/crates/phenix-plugin-session-tree/src/component.rs` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | pending | 
| F0384 | TODO | `rust/crates/phenix-plugin-session-tree/src/disable_independently_regression.rs` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | pending | 
| F0385 | TODO | `rust/crates/phenix-plugin-session-tree/src/implementation.rs` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | pending | 
| F0386 | TODO | `rust/crates/phenix-plugin-session-tree/src/interface.rs` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | pending | 
| F0387 | TODO | `rust/crates/phenix-plugin-session-tree/src/lib.rs` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | pending | 
| F0388 | TODO | `rust/crates/phenix-plugin-session-tree/src/session_layering_regression.rs` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | pending | 
| F0389 | TODO | `rust/crates/phenix-plugin-session-tree/src/session_tree_atomicity_regression.rs` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | pending | 

### `rust/crates/phenix-plugin-sessions`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0390 | TODO | `rust/crates/phenix-plugin-sessions/Cargo.toml` | Keep | Session semantics and storage outside Core. | — | pending | 
| F0391 | TODO | `rust/crates/phenix-plugin-sessions/src/history_integration.rs` | Keep | Session semantics and storage outside Core. | — | pending | 
| F0392 | TODO | `rust/crates/phenix-plugin-sessions/src/implementation.rs` | Keep | Session semantics and storage outside Core. | — | pending | 
| F0393 | TODO | `rust/crates/phenix-plugin-sessions/src/lib.rs` | Keep | Session semantics and storage outside Core. | — | pending | 

### `rust/crates/phenix-plugin-step-runner`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0394 | TODO | `rust/crates/phenix-plugin-step-runner/Cargo.toml` | Refactor | Keep step execution; remove agent-loop progression once topology owns control flow. | #726, #729 | pending | 
| F0395 | TODO | `rust/crates/phenix-plugin-step-runner/src/lib.rs` | Refactor | Keep step execution; remove agent-loop progression once topology owns control flow. | #726, #729 | pending | 
| F0396 | TODO | `rust/crates/phenix-plugin-step-runner/src/runner.rs` | Refactor | Limit to one step invocation; remove any graph/run progression authority. | #726, #729 | pending | 
| F0397 | TODO | `rust/crates/phenix-plugin-step-runner/tests/invocation.rs` | Refactor | Keep step execution; remove agent-loop progression once topology owns control flow. | #726, #729 | pending | 
| F0398 | TODO | `rust/crates/phenix-plugin-step-runner/tests/planned_step.rs` | Refactor | Keep step execution; remove agent-loop progression once topology owns control flow. | #726, #729 | pending | 
| F0399 | TODO | `rust/crates/phenix-plugin-step-runner/tests/recovery_invocation.rs` | Refactor | Keep step execution; remove agent-loop progression once topology owns control flow. | #726, #729 | pending | 

### `rust/crates/phenix-plugin-workspace`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0400 | TODO | `rust/crates/phenix-plugin-workspace/Cargo.toml` | Keep | Workspace selection, policies and state remain domain-specific plugins. | — | pending | 
| F0401 | TODO | `rust/crates/phenix-plugin-workspace/src/component.rs` | Keep | Workspace selection, policies and state remain domain-specific plugins. | — | pending | 
| F0402 | TODO | `rust/crates/phenix-plugin-workspace/src/implementation.rs` | Keep | Workspace selection, policies and state remain domain-specific plugins. | — | pending | 
| F0403 | TODO | `rust/crates/phenix-plugin-workspace/src/lib.rs` | Keep | Workspace selection, policies and state remain domain-specific plugins. | — | pending | 

### `rust/crates/phenix-provider-sdk`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0404 | TODO | `rust/crates/phenix-provider-sdk/Cargo.toml` | Keep | Reusable provider integration helpers, outside Core. | — | pending | 
| F0405 | TODO | `rust/crates/phenix-provider-sdk/src/auth.rs` | Keep | Reusable provider integration helpers, outside Core. | — | pending | 
| F0406 | TODO | `rust/crates/phenix-provider-sdk/src/lib.rs` | Keep | Reusable provider integration helpers, outside Core. | — | pending | 
| F0407 | TODO | `rust/crates/phenix-provider-sdk/src/protocol.rs` | Keep | Reusable provider integration helpers, outside Core. | — | pending | 
| F0408 | TODO | `rust/crates/phenix-provider-sdk/src/runtime.rs` | Keep | Reusable provider integration helpers, outside Core. | — | pending | 
| F0409 | TODO | `rust/crates/phenix-provider-sdk/src/store.rs` | Keep | Reusable provider integration helpers, outside Core. | — | pending | 
| F0410 | TODO | `rust/crates/phenix-provider-sdk/src/types.rs` | Keep | Reusable provider integration helpers, outside Core. | — | pending | 

### `rust/crates/phenix-runtime`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0411 | TODO | `rust/crates/phenix-runtime/Cargo.toml` | Keep | Standalone generic kernel runtime with no AI plugin requirement. | — | pending | 
| F0412 | TODO | `rust/crates/phenix-runtime/src/lib.rs` | Keep | Standalone generic kernel runtime with no AI plugin requirement. | — | pending | 
| F0413 | TODO | `rust/crates/phenix-runtime/src/main.rs` | Keep | Standalone generic kernel runtime with no AI plugin requirement. | — | pending | 

### `rust/crates/phenix-sdk-macros`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0414 | TODO | `rust/crates/phenix-sdk-macros/Cargo.toml` | Keep | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | pending | 
| F0415 | TODO | `rust/crates/phenix-sdk-macros/src/component_attr.rs` | Keep | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | pending | 
| F0416 | TODO | `rust/crates/phenix-sdk-macros/src/component_runtime_attr.rs` | Keep | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | pending | 
| F0417 | TODO | `rust/crates/phenix-sdk-macros/src/expose_attr.rs` | Keep | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | pending | 
| F0418 | TODO | `rust/crates/phenix-sdk-macros/src/interface_attr.rs` | Keep | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | pending | 
| F0419 | TODO | `rust/crates/phenix-sdk-macros/src/lib.rs` | Keep | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | pending | 
| F0420 | TODO | `rust/crates/phenix-sdk-macros/src/plugin_attr.rs` | Keep | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | pending | 
| F0421 | TODO | `rust/crates/phenix-sdk-macros/src/plugin_attr_core.rs` | Keep | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | pending | 
| F0422 | TODO | `rust/crates/phenix-sdk-macros/src/plugin_attr_legacy.rs` | Keep | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | pending | 
| F0423 | TODO | `rust/crates/phenix-sdk-macros/src/resource_attr.rs` | Keep | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | pending | 

### `rust/crates/phenix-sdk`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0424 | TODO | `rust/crates/phenix-sdk/Cargo.toml` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | pending | 
| F0425 | TODO | `rust/crates/phenix-sdk/README.md` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | pending | 
| F0426 | TODO | `rust/crates/phenix-sdk/src/api.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | pending | 
| F0427 | TODO | `rust/crates/phenix-sdk/src/authoring.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | pending | 
| F0428 | TODO | `rust/crates/phenix-sdk/src/authoring/context.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | pending | 
| F0429 | TODO | `rust/crates/phenix-sdk/src/authoring/event_context.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | pending | 
| F0430 | TODO | `rust/crates/phenix-sdk/src/authoring/plugin.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | pending | 
| F0431 | TODO | `rust/crates/phenix-sdk/src/authoring/static_component.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | pending | 
| F0432 | TODO | `rust/crates/phenix-sdk/src/authoring/static_config.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | pending | 
| F0433 | TODO | `rust/crates/phenix-sdk/src/authoring/static_dispatch.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | pending | 
| F0434 | TODO | `rust/crates/phenix-sdk/src/authoring/static_graph_runtime.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | pending | 
| F0435 | TODO | `rust/crates/phenix-sdk/src/authoring/static_import.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | pending | 
| F0436 | TODO | `rust/crates/phenix-sdk/src/authoring/static_lifecycle.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | pending | 
| F0437 | TODO | `rust/crates/phenix-sdk/src/authoring/static_resource.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | pending | 
| F0438 | TODO | `rust/crates/phenix-sdk/src/contracts/agent_diagnostics.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0439 | TODO | `rust/crates/phenix-sdk/src/contracts/agent_loop.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0440 | TODO | `rust/crates/phenix-sdk/src/contracts/budget.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0441 | TODO | `rust/crates/phenix-sdk/src/contracts/context.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0442 | TODO | `rust/crates/phenix-sdk/src/contracts/context_admission.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0443 | TODO | `rust/crates/phenix-sdk/src/contracts/context_compaction.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0444 | TODO | `rust/crates/phenix-sdk/src/contracts/context_recovery_bootstrap.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0445 | TODO | `rust/crates/phenix-sdk/src/contracts/delegation.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0446 | TODO | `rust/crates/phenix-sdk/src/contracts/efficiency_evaluation.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0447 | TODO | `rust/crates/phenix-sdk/src/contracts/environment.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0448 | TODO | `rust/crates/phenix-sdk/src/contracts/execution.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0449 | TODO | `rust/crates/phenix-sdk/src/contracts/execution_resources.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0450 | TODO | `rust/crates/phenix-sdk/src/contracts/exploration.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0451 | TODO | `rust/crates/phenix-sdk/src/contracts/frontend.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0452 | TODO | `rust/crates/phenix-sdk/src/contracts/jobs.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0453 | TODO | `rust/crates/phenix-sdk/src/contracts/language.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0454 | TODO | `rust/crates/phenix-sdk/src/contracts/memory.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0455 | TODO | `rust/crates/phenix-sdk/src/contracts/memory_context.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0456 | TODO | `rust/crates/phenix-sdk/src/contracts/memory_freshness.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0457 | TODO | `rust/crates/phenix-sdk/src/contracts/mod.rs` | Refactor | Remove re-exports of relocated agent-domain contract definitions. | #731 | pending | 
| F0458 | TODO | `rust/crates/phenix-sdk/src/contracts/model_dispatch.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0459 | TODO | `rust/crates/phenix-sdk/src/contracts/models.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0460 | TODO | `rust/crates/phenix-sdk/src/contracts/options.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0461 | TODO | `rust/crates/phenix-sdk/src/contracts/planning.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0462 | TODO | `rust/crates/phenix-sdk/src/contracts/primitive_agent_export.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0463 | TODO | `rust/crates/phenix-sdk/src/contracts/sessions.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0464 | TODO | `rust/crates/phenix-sdk/src/contracts/skills.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0465 | TODO | `rust/crates/phenix-sdk/src/contracts/step_attempt.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0466 | TODO | `rust/crates/phenix-sdk/src/contracts/step_runner.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0467 | TODO | `rust/crates/phenix-sdk/src/contracts/step_transaction.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0468 | TODO | `rust/crates/phenix-sdk/src/contracts/tool_observation.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0469 | TODO | `rust/crates/phenix-sdk/src/contracts/usage.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0470 | TODO | `rust/crates/phenix-sdk/src/contracts/usage_policy.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0471 | TODO | `rust/crates/phenix-sdk/src/contracts/workspace.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | pending | 
| F0472 | TODO | `rust/crates/phenix-sdk/src/lib.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | pending | 
| F0473 | TODO | `rust/crates/phenix-sdk/src/providers.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | pending | 
| F0474 | TODO | `rust/crates/phenix-sdk/src/public_projection.rs` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | pending | 
| F0475 | TODO | `rust/crates/phenix-sdk/tests/fallible_provider_dispatch.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | pending | 
| F0476 | TODO | `rust/crates/phenix-sdk/tests/incompatible_schema.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | pending | 
| F0477 | TODO | `rust/crates/phenix-sdk/tests/plugin_attribute_graph.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | pending | 
| F0478 | TODO | `rust/crates/phenix-sdk/tests/plugin_attribute_only_gate.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | pending | 
| F0479 | TODO | `rust/crates/phenix-sdk/tests/plugin_component_authoring.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | pending | 
| F0480 | TODO | `rust/crates/phenix-sdk/tests/plugin_config_authoring.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | pending | 
| F0481 | TODO | `rust/crates/phenix-sdk/tests/plugin_dependency_authoring.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | pending | 
| F0482 | TODO | `rust/crates/phenix-sdk/tests/plugin_import_authoring.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | pending | 
| F0483 | TODO | `rust/crates/phenix-sdk/tests/plugin_layer_authority.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | pending | 
| F0484 | TODO | `rust/crates/phenix-sdk/tests/plugin_lifecycle_authoring.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | pending | 
| F0485 | TODO | `rust/crates/phenix-sdk/tests/plugin_manifest_authoring.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | pending | 
| F0486 | TODO | `rust/crates/phenix-sdk/tests/plugin_public_projection.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | pending | 
| F0487 | TODO | `rust/crates/phenix-sdk/tests/plugin_resource_authoring.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | pending | 
| F0488 | TODO | `rust/crates/phenix-sdk/tests/plugin_stateless_manifest_authoring.rs` | Keep | Preserve compile-fail and authoring contract coverage; update when old registration features disappear. | #728, #730, #731 | pending | 

### `rust/`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0489 | TODO | `rust/deny.toml` | Review | Unrecognized tracked artifact; verify purpose, ownership and consumer before removal. | consumer trace | pending | 

### `scripts/`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0490 | TODO | `scripts/check-plugin-architecture.sh` | Move | Move enforcement/check logic into Nix-supported dev tooling, retaining coverage before retiring standalone shell script. | Nix check replacement + CI proof | pending | 
| F0491 | IN PROGRESS | `scripts/check-rust-safety-policy.sh` | Move | Move enforcement/check logic into Nix-supported dev tooling, retaining coverage before retiring standalone shell script. | Nix check replacement + CI proof | Script `86183104` reads Cargo metadata to enforce `#![forbid(unsafe_code)]` across production targets; `modules/development.nix` invokes it. MOVE only after an equivalent Nix-owned rule, negative fixture and CI proof; current live consumer blocks deletion. | 
| F0492 | TODO | `scripts/check-spec-lifecycle-fixtures.sh` | Move | Move enforcement/check logic into Nix-supported dev tooling, retaining coverage before retiring standalone shell script. | Nix check replacement + CI proof | pending | 
| F0493 | TODO | `scripts/check-spec-lifecycle.sh` | Move | Move enforcement/check logic into Nix-supported dev tooling, retaining coverage before retiring standalone shell script. | Nix check replacement + CI proof | pending | 
| F0494 | TODO | `scripts/check-structural-boundaries.sh` | Move | Move enforcement/check logic into Nix-supported dev tooling, retaining coverage before retiring standalone shell script. | Nix check replacement + CI proof | pending | 

### `share/`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0495 | TODO | `share/phenix/interfaces/phenix.application@1.json` | Keep | Versioned language-neutral interface/schema asset. | — | pending | 

### `spec/`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0496 | TODO | `spec/acp-stdio.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0497 | TODO | `spec/adapter-acp.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0498 | TODO | `spec/agent-configurations.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0499 | TODO | `spec/agent-runtime-orchestration.md` | Refactor | Align agent-specific orchestration language with generic workflow graph and advanced plugin topology. | #726, #729 | pending | 
| F0500 | TODO | `spec/application-cli.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0501 | TODO | `spec/application-integration-terminology.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0502 | TODO | `spec/application-interface.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0503 | TODO | `spec/binding-lua.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0504 | TODO | `spec/client-provided-tools-implementation.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0505 | TODO | `spec/client-provided-tools.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0506 | TODO | `spec/client-sdk-acp.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0507 | TODO | `spec/composable-agent-runtime.md` | Refactor | Replace interim architecture mechanisms with normative usage-agnostic Core/plugin boundary. | #729, #730 | pending | 
| F0508 | TODO | `spec/configuration-frontends.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0509 | TODO | `spec/context-artifacts-pruning.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0510 | TODO | `spec/context-catalog.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0511 | TODO | `spec/context-compaction.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0512 | TODO | `spec/decisions-history.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0513 | TODO | `spec/environment-workspace-boundary.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0514 | TODO | `spec/execution-context-projection.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0515 | TODO | `spec/fallback-memory-recall.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0516 | TODO | `spec/followups/call-scope-endpoints.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | pending | 
| F0517 | TODO | `spec/followups/core-module-boundaries.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | pending | 
| F0518 | TODO | `spec/followups/memory-retrieval.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | pending | 
| F0519 | TODO | `spec/followups/observable-handles.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | pending | 
| F0520 | TODO | `spec/followups/plugin-durable-storage.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | pending | 
| F0521 | TODO | `spec/followups/resolved-dispatch-plan.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | pending | 
| F0522 | TODO | `spec/followups/token-cache-provider-integration.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | pending | 
| F0523 | TODO | `spec/followups/token-code-entity-lineage.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | pending | 
| F0524 | TODO | `spec/followups/token-efficiency-evaluation.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | pending | 
| F0525 | TODO | `spec/followups/token-isolated-exploration-runtime.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | pending | 
| F0526 | TODO | `spec/followups/token-lazy-tools-observations.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | pending | 
| F0527 | TODO | `spec/followups/token-memory-code-dependencies.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | pending | 
| F0528 | TODO | `spec/followups/token-primitive-agent-export.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | pending | 
| F0529 | TODO | `spec/followups/token-structured-code-actions.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | pending | 
| F0530 | TODO | `spec/frontend-services.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0531 | TODO | `spec/incremental-cargo-nix-artifacts.md` | Review (new) | Check whether experimental Crane benchmarks belong to #758; keep only maintained design guidance. | #758 review | pending | 
| F0532 | TODO | `spec/interactive-ui.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0533 | TODO | `spec/kernel-hooks.md` | Refactor | Keep retirement/migration invariants until legacy hooks are deleted, then consolidate with Layer/Event specs. | #731 | pending | 
| F0534 | TODO | `spec/kernel-runtime-rfc.md` | Keep (new) | Retain canonical kernel semantics; verify references point here instead of reproducing rules. | #736 merged | pending | 
| F0535 | TODO | `spec/language-intelligence.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0536 | TODO | `spec/language-service.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0537 | TODO | `spec/lifecycle-hooks.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0538 | TODO | `spec/microkernel-composition-roadmap.md` | Keep | Normative usage-agnostic kernel boundary and accepted dependency ordering. | — | pending | 
| F0539 | TODO | `spec/model-routing.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0540 | TODO | `spec/model-turn-protocol.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0541 | TODO | `spec/native-plugin-abi-and-guest-runtimes.md` | Keep (new) | Retain native adapter contract; verify no contradictory embedded/native guidance remains. | #734 merged | pending | 
| F0542 | TODO | `spec/objectives.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0543 | TODO | `spec/observability.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0544 | TODO | `spec/observable-values.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0545 | TODO | `spec/persistent-terminals-jobs.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0546 | TODO | `spec/plans.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0547 | TODO | `spec/plugin-architecture-enforcement.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0548 | TODO | `spec/plugin-artifact-readers.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0549 | TODO | `spec/plugin-authoring-macro.md` | Refactor | Document typed const/field contributions and kind-generic annotation syntax. | #728, #730 | pending | 
| F0550 | TODO | `spec/plugin-call-binding.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0551 | TODO | `spec/plugin-cli.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0552 | TODO | `spec/plugin-context.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0553 | TODO | `spec/plugin-contributions.md` | Refactor | Reconcile old contribution language against typed descriptor/kind template contracts. | #728, #730 | pending | 
| F0554 | TODO | `spec/plugin-durable-data.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0555 | TODO | `spec/plugin-embedded-runtime.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0556 | TODO | `spec/plugin-events.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0557 | TODO | `spec/plugin-host.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0558 | TODO | `spec/plugin-memory-freshness.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0559 | TODO | `spec/plugin-memory.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0560 | TODO | `spec/plugin-options.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0561 | TODO | `spec/plugin-persistence.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0562 | TODO | `spec/plugin-process-runtime-bridge.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0563 | TODO | `spec/plugin-resolution.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0564 | TODO | `spec/plugin-runtime-bridges.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0565 | TODO | `spec/plugin-sdk-context.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0566 | TODO | `spec/plugin-sdk.md` | Refactor | Remove deprecated first-party authoring shortcuts in favor of generic SDK contract. | #728, #730 | pending | 
| F0567 | TODO | `spec/plugin-service-layering.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0568 | TODO | `spec/plugin-sessions.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0569 | TODO | `spec/plugin-threading.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0570 | TODO | `spec/post-redesign-file-dispositions.md` | Keep | This frozen per-file expected disposition and proof of audit coverage; refresh after merges. | — | pending | 
| F0571 | TODO | `spec/process-confinement.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0572 | TODO | `spec/provider-protocol-sdk.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0573 | TODO | `spec/referenced-logging.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0574 | TODO | `spec/runtime-entry-triggers.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0575 | TODO | `spec/runtime-host-interfaces.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0576 | TODO | `spec/runtime-inspection.md` | Refactor | Add resolved graph/kind origin and provenance inspection after graph patch migration. | #729, #730 | pending | 
| F0577 | TODO | `spec/runtime-topology-generation.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0578 | TODO | `spec/rust-library-ownership-audit.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0579 | TODO | `spec/scheduled-work.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0580 | TODO | `spec/secrets.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0581 | TODO | `spec/selectable-generations.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0582 | TODO | `spec/skill-discovery.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0583 | TODO | `spec/spec-coverage-lifecycle.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0584 | TODO | `spec/token-efficiency.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0585 | TODO | `spec/transcript-streaming-and-admission.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0586 | TODO | `spec/transport-socket.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0587 | TODO | `spec/typed-structural-boundaries.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0588 | TODO | `spec/unified-semantic-code-query.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0589 | TODO | `spec/value-capability-sdk.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0590 | TODO | `spec/web-access.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0591 | TODO | `spec/worker-pr-runtime.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0592 | TODO | `spec/worker-profile-runtime.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0593 | TODO | `spec/worker-results-verification.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0594 | TODO | `spec/worker-task-dag.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0595 | TODO | `spec/workspace-execution.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 
| F0596 | TODO | `spec/workspace-persistence.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending | 

## Tracker-branch-only files

These four files are new on #764 and are not part of the 596-file main snapshot. They must also receive an explicit keep/archive/retire decision before this tracker closes.

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0597 | TODO | `spec/microkernel-migration-tracker.md` | Keep | Retain single authoritative cross-PR dependency state until closeout; archive final outcomes on main. | S6-03 | pending |
| F0598 | TODO | `spec/microkernel-tracker/repository-file-matrix.md` | Keep | Preserve exact path decisions and removing PR evidence; reconcile main and branch inventories. | S5-08 | pending |
| F0599 | TODO | `spec/microkernel-tracker/pr726-file-matrix.md` | Keep | Carry 58 path dispositions and extraction/removal proof after #726 lands. | S5-08 | pending |
| F0600 | TODO | `spec/microkernel-tracker/pr-task-matrices.md` | Keep | Link owner PR task comments and archive the final per-PR evidence index. | S6-03 | pending |

## Reconciliation gate

The owner PR records `git ls-files` totals and the list of paths added/removed since this snapshot. Every file retains a stable ID and a final decision with evidence. For #726-only paths not on main, use [its separate changed-file matrix](pr726-file-matrix.md). Track external Nix and client repositories separately when their change is needed for parity; do not claim they were audited here.
