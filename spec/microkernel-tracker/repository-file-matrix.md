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
| F0002 | IN PROGRESS | `.github/workflows/ci.yml` | Refactor | Keep required CI workflow, audit duplicated shards, maintenance and tests after topology migration. | #731 + CI baseline | KEEP generated workflow. `ci.yml:1` names phenix-flake-ci as author; `:35-380` runs dependency-derived PR check selection, and source jobs invoke Nix maintenance. Audit affected-check skip safety and generated parity, then run CI on exact head. Edit Nix declarations, not generated YAML. #766 `a0a2c0f` and #767 `ba91b3c` remove obsolete Cargo package IDs from generated shard impact metadata after failed source sync; reverify source job on new heads. |
| F0003 | IN PROGRESS | `.github/workflows/dependency-security.yml` | Refactor | Keep required CI workflow, audit duplicated shards, maintenance and tests after topology migration. | #731 + CI baseline | KEEP security CI. This workflow runs `cargo deny check licenses sources` on PRs, main and its weekly schedule. Decide whether it belongs in flake-ci maintenance and retain scheduled security coverage. Check exact-head workflow run before certification. |
| F0004 | IN PROGRESS | `.github/workflows/shared-binary-cache.yml` | Refactor | Keep required CI workflow, audit duplicated shards, maintenance and tests after topology migration. | #731 + CI baseline | KEEP post-CI binary cache publishing. Workflow triggers after main CI success or manual dispatch; publishes Nix outputs to cache. Validate untrusted PR isolation, no duplicate store uploads and main success path before simplification. |
| F0005 | IN PROGRESS | `.github/workflows/sync-maintenance.yml` | Refactor | Keep required CI workflow, audit duplicated shards, maintenance and tests after topology migration. | #731 + CI baseline | KEEP migration compatibility check. Workflow notes legacy `Maintenance autofix` check name. PR path is read-only diff validation; manual path publishes patch artifact. After flake-ci status parity, decide whether to remove this workflow or preserve its required check context. |
| F0006 | IN PROGRESS | `.github/workflows/worker-executor.yml` | Refactor | Keep required CI workflow, audit duplicated shards, maintenance and tests after topology migration. | #731 + CI baseline | KEEP security guard. `issue_comment` workflow rejects forbidden worker-comment transport, with issue/PR write scope. Review whether that channel remains enabled and verify an unauthorized comment cannot trigger worker execution before retiring. |

### `root files`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0007 | PROVEN | `.gitignore` | Keep | Repository generated-content hygiene. | — | KEEP. Direct source review on main `d488c405`, blob `b5e60517`: excludes Nix `result*`, `/rust/target/`, `.direnv`, cache and log files. Git-only hygiene; no runtime replacement or test needed. |
| F0008 | IN PROGRESS | `AGENTS.md` | Keep | Repo operator/developer or architecture guidance; update stale references after migration. | #731 + docs sync | KEEP candidate. `AGENTS.md` blob `87a32bd5` names README, DEVELOPMENT and runtime.nix authorities; lines 22-27 require pre-commit checks and state that GitHub API commits bypass local hooks. Keep agent change discipline, reconcile static product authority after #731, and verify referenced Nix checks. |
| F0009 | IN PROGRESS | `DEVELOPMENT.md` | Keep | Repo operator/developer or architecture guidance; update stale references after migration. | #731 + docs sync | KEEP candidate. `DEVELOPMENT.md` blob `3b6b1a2c` maps maintenance checks to source/rust/unit/doc/integration/system/product, and documents descriptor fixture regeneration. Compare commands with the generated `modules/development.nix` tasks and run exact-head source/descriptor checks. |
| F0010 | PROVEN | `LICENSE` | Keep | Repository licensing. | — | KEEP. Direct source review on main `d488c405`, blob `831ecf00`: MIT grant, 2026 copyright, attribution and redistribution notice remain part of repository distribution. No runtime substitute or CI test applies. |
| F0011 | IN PROGRESS | `README.md` | Keep | Repo operator/developer or architecture guidance; update stale references after migration. | #731 + docs sync | REFACTOR candidate. `README.md` blob `4992ae83` defines Core, Runtime, Harness and catalog roles; the embedded-factory catalog and legacy agent-loop option describe transitional products. Rewrite those claims only after #731 selected artifacts and #766/#767 retirements reach shipped products; verify referenced CLI/package exports. |

### `config/`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0012 | IN PROGRESS | `config/phenix/NOTICE.md` | Refactor | Retain portable product config; remove Nix-only feature selection and generated semantics. | #731 | NOTICE says runtime.nix came from older phenix-harness and static skills moved to basic-skills. Candidate KEEP as provenance/licensing note. Check that packaged license and current consolidated paths still match; remove stale migration wording if false. |
| F0013 | IN PROGRESS | `config/phenix/runtime.nix` | Refactor | Retain portable product config; remove Nix-only feature selection and generated semantics. | #731 | `runtime.nix:1-13` declares transitional text-only callable contract, later defines agent routing and fallback target. Candidate REFACTOR into portable product config after #731; verify schema/callable identity and real provider selection before removing legacy settings. |

### `root files`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0014 | TODO | `flake.lock` | Keep | Canonical dependency/package metadata; synchronize when moving or retiring crates. | — | pending |
| F0015 | IN PROGRESS | `flake.nix` | Keep | Canonical dependency/package metadata; synchronize when moving or retiring crates. | — | KEEP candidate. `flake.nix` blob `9a6e15b8` delegates outputs to flake-parts and imports `rust-artifacts`, `harness-product`, `plugin-packaging`, `package-sets`, Lua integration, development and Stitch modules. This is the actual build composition entry. Check `nix flake check`/show and exports on exact integration head. |
| F0016 | TODO | `glossary.md` | Keep | Repo operator/developer or architecture guidance; update stale references after migration. | #731 + docs sync | pending |

### `modules/`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0017 | IN PROGRESS | `modules/cargo-source.nix` | Review (new) | Inspect source-dependency pin and its Nix consumers; retain only if not duplicating package derivations. | CI source derivation parity | Live consumer paths: `modules/rust-artifacts.nix`, `modules/package-sets.nix`, `modules/harness-product.nix`, `modules/lua-binding-integration.nix`. Source `9941dcf2` derives local Rust dependency closure from Cargo manifests. Candidate KEEP; compare Nix closure and cached rebuild behavior before certification. |
| F0018 | IN PROGRESS | `modules/commit-candidate.nix` | Keep | Retain Nix deployment/build capability; no runtime semantic authority. | — | Imported by `modules/development.nix`. Source `c138bc02` defines staged-only `maintenance commit`, with success, unrelated-edit and untracked-file fixture cases. Candidate KEEP; verify current-head Nix test target and hook interaction before certification. |
| F0019 | IN PROGRESS | `modules/development.nix` | Keep | Retain Nix deployment/build capability; no runtime semantic authority. | — | Owns maintenance registration, generated-hook parity check and safety-policy check used by the dev/CI entry. Review production shell/check dependency closure and execute exact-head Nix checks before KEEP proof. |
| F0020 | IN PROGRESS | `modules/flake-module.nix` | Keep | Retain Nix deployment/build capability; no runtime semantic authority. | — | Source `9451a73c` exports `phenixWrapped` for phenix, kernel, runtime, harness and Stitch packages; imported as default flake module. Candidate KEEP; verify these exports by flake evaluation and consumer checks. |
| F0021 | IN PROGRESS | `modules/harness-product.nix` | Refactor | Thin portable product config wrapper; remove duplicated product selection. | #731 | `harness-product.nix:5` uses dependency-selected Cargo source; `:38` builds runtime config; `:53-140` defines real product and Lua smoke checks. KEEP packaging/smoke, move product policy and duplicated selection after #731. Nix product check proof pending. |
| F0022 | IN PROGRESS | `modules/lua-binding-integration.nix` | Keep | Retain Nix deployment/build capability; no runtime semantic authority. | — | KEEP candidate. `modules/lua-binding-integration.nix` blob `1b33e240` builds the `phenix-acp-stdio` callback fixture and exercises `require("phenix")`, observable callbacks, typed application callable operations and client tools under LuaJIT. This is client ABI parity, not the future guest-Lua adapter. Run `phenix-binding-lua-observable-callback` before PROVEN. |
| F0023 | IN PROGRESS | `modules/package-sets.nix` | Refactor | Remove hardcoded plugin-name identity map and migration aliases after portable profile selection. | #730, #731 | `package-sets.nix:142-161` validates hardcoded plugin crate roles/aliases, and `:201-219` exports workspace crates/checks. Replace plugin identity mapping with manifest-driven selection while retaining packaging derivations; verify checked source closures. #765 marks hardcoded plugin crate aliases for artifact-derived packaging after #730/#731. Annotated at #765 head `cf700548`; CI pending. |
| F0024 | IN PROGRESS | `modules/plugin-packaging.nix` | Refactor | Nix packages deployment artifacts, not tool/skill resolution or provider selection authority. | #730, #731 | `plugin-packaging.nix:54-142` forms embedded/packaged plugin wrappers; `:199/204` names Basic/Full; `:371-429` validates wrappers. KEEP deployment mechanics, move product selection to typed artifacts, preserve wrapper and product smoke tests. |
| F0025 | IN PROGRESS | `modules/rust-artifacts.nix` | Keep | Retain Nix deployment/build capability; no runtime semantic authority. | — | KEEP candidate. `modules/rust-artifacts.nix` blob `b2dd7297` uses Cargo dependency skeleton plus selected Harness source and reuses release artifacts; it removes Cargo lock sentinel files before reuse to prevent inode-lock deadlocks. Test incremental source closure and product derivation on exact head before retaining as proven. |
| F0026 | IN PROGRESS | `modules/stitch.nix` | Review | Establish its live package consumer and unique responsibility before retaining/removing. | consumer + CI references | Live consumer confirmed: `flake.nix` imports `modules/stitch.nix`; it exports `stitch`/`stitch-mcp` packages/apps. `modules/development.nix` invokes its `stitch-runtime-smoke` check. Source `ea7d94c4`. Candidate KEEP, not dead code; pending exact-head Nix smoke and MCP package check. |

### `rust/`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0027 | TODO | `rust/Cargo.lock` | Keep | Canonical dependency/package metadata; synchronize when moving or retiring crates. | — | pending |
| F0028 | IN PROGRESS | `rust/Cargo.toml` | Keep | Canonical dependency/package metadata; synchronize when moving or retiring crates. | — | REFACTOR candidate. `rust/Cargo.toml` blob `a47449bb` owns the resolver-3 workspace and still lists `phenix-plugin-basic-agent` and `phenix-plugin-hooks`. #766/#767 remove those members on their stacked branches. Regenerate Cargo metadata and lockfile, then check all Nix impact/package declarations before PROVEN. |

### `rust/crates/phenix-acp-stdio`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0029 | IN PROGRESS | `rust/crates/phenix-acp-stdio/Cargo.toml` | Keep | ACP stdio application and transport boundary; independent from Core. | — | Main `a9964d80`: application-role stdio crate imports adapter, application contracts, Core and domain. KEEP package; require packaged startup and typed routing. |
| F0030 | IN PROGRESS | `rust/crates/phenix-acp-stdio/examples/observable_callback_fixture.rs` | Keep | ACP stdio application and transport boundary; independent from Core. | — | Main `9b7f6ab5`: executable SDK/ACP callback fixture exercises Lua client-tool round trip. KEEP; run Lua observable-callback and package smoke. |
| F0031 | IN PROGRESS | `rust/crates/phenix-acp-stdio/src/client_tools.rs` | Keep | ACP stdio application and transport boundary; independent from Core. | — | Main `42a84b8b`: merges runtime/client model-tool descriptors and dispatches client tool calls. REFACTOR after #731 moves AI tool DTOs from Core; prove duplicate IDs, permission and reconnect. |
| F0032 | IN PROGRESS | `rust/crates/phenix-acp-stdio/src/lib.rs` | Keep | ACP stdio application and transport boundary; independent from Core. | — | Main `b33c2264`: unsafe-free module entry exports transport/client-tool functions used by fixture. KEEP; ACP package/callback smoke required. |
| F0033 | IN PROGRESS | `rust/crates/phenix-acp-stdio/src/transport.rs` | Keep | ACP stdio application and transport boundary; independent from Core. | — | Main `eebf0ad6`: 2,026-line stdio, ChannelTransport and SDK callback host; protocol mapping is in separate adapter. KEEP transport; inspect overlap with canonical admission and test end-to-end ACP. |
| F0034 | IN PROGRESS | `rust/crates/phenix-acp-stdio/tests/client_callback_retirement.rs` | Keep | ACP stdio application and transport boundary; independent from Core. | — | Main `45b81037`: disconnected callback listener retires owning generation in named regression. KEEP; rerun ACP shard and real disconnect. |
| F0035 | IN PROGRESS | `rust/crates/phenix-acp-stdio/tests/client_tool_reconnect.rs` | Keep | ACP stdio application and transport boundary; independent from Core. | — | Main `6274e300`: reconnect regression requires fresh callable generation before readmission. KEEP; add live client reconnect proof. |
| F0036 | IN PROGRESS | `rust/crates/phenix-acp-stdio/tests/nested_client_callable_input.rs` | Keep | ACP stdio application and transport boundary; independent from Core. | — | Main `8af8e49c`: nested client callable input admission has direct regression. KEEP; test ACP/reference scope on integrated product. |

### `rust/crates/phenix-adapter-acp`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0037 | IN PROGRESS | `rust/crates/phenix-adapter-acp/Cargo.toml` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Main `0131b4ac`: runtime-plugin adapter depends on typed app interface, Core and SDK, not Harness. KEEP; prove artifact selection without a static roster. |
| F0038 | IN PROGRESS | `rust/crates/phenix-adapter-acp/src/callbacks.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Main `7586aaaf`: permission request/response ACP mapping with focused valid/invalid choice tests. KEEP; test real permission exchange. |
| F0039 | IN PROGRESS | `rust/crates/phenix-adapter-acp/src/dispatch.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Main `b6157e6f`: standard session, prompt, cancellation and config dispatch uses typed ApplicationTransport and rejects extra workspace/MCP. KEEP; verify product-selected generation and cancellation. |
| F0040 | IN PROGRESS | `rust/crates/phenix-adapter-acp/src/elicitation.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Main `64d49332`: ACP form elicitation translates supported schema, rejects unsupported shapes. KEEP; round-trip through frontend. |
| F0041 | IN PROGRESS | `rust/crates/phenix-adapter-acp/src/errors.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Main `6b0d2600`: error translator retains application error class, including cancellation/malformed input tests. KEEP; transport wire error proof needed. |
| F0042 | IN PROGRESS | `rust/crates/phenix-adapter-acp/src/extension_callbacks.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Main `5a4be414`: descriptor-checked extension callbacks reject wrong capability/contract/shape. KEEP; test async response, stale generation. |
| F0043 | IN PROGRESS | `rust/crates/phenix-adapter-acp/src/extension_dispatch.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Main `a4e45d4f`: static match table for typed extension operations; regression covers unknown, malformed, capability and client-tool paths. REFACTOR toward descriptor-selected dispatch; retain rejection semantics. |
| F0044 | IN PROGRESS | `rust/crates/phenix-adapter-acp/src/extensions.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Main `ad44a9cb`: extension catalog derives operations/events/callbacks from app descriptor and filters ACP standard calls. KEEP; test dynamic descriptor advertisement. |
| F0045 | IN PROGRESS | `rust/crates/phenix-adapter-acp/src/lib.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Main `d739b49b`: module exports adapter plugin manifest/factory, identity regression in runtime_plugin test. KEEP; test selected activation after #728/#760. |
| F0046 | IN PROGRESS | `rust/crates/phenix-adapter-acp/src/updates.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Main `94b64cec`: session/execution update translators preserve correlation and descriptor extension fallback with unit tests. KEEP; full streaming sequence/usage proof needed. |
| F0047 | IN PROGRESS | `rust/crates/phenix-adapter-acp/tests/application_dispatch.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Main `1a64b309`: application dispatch test checks prompt/session/config, rejects unsupported inputs and preserves resume identity. KEEP; rerun after #768. |
| F0048 | IN PROGRESS | `rust/crates/phenix-adapter-acp/tests/capabilities.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Main `4f8c4542`: capability negotiation test filters unsupported optional methods. KEEP; verify selected dynamic plugin descriptors. |
| F0049 | IN PROGRESS | `rust/crates/phenix-adapter-acp/tests/disconnect.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Main `0f18a0f8`: dropping adapter does not close durable session in regression. KEEP; add #725 fenced restart/reconnect. |
| F0050 | IN PROGRESS | `rust/crates/phenix-adapter-acp/tests/runtime_plugin.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Main `24d52103`: adapter manifest/factory plugin identity fixture. KEEP; selected-provider canary after #728/#760. |
| F0051 | IN PROGRESS | `rust/crates/phenix-adapter-acp/tests/session_lifecycle.rs` | Keep | Replaceable ACP protocol adapter, not kernel logic. | — | Main `e4c22937`: standard ACP load/close/cancel use canonical session operations. KEEP; test in-flight native pending cancellation. |

### `rust/crates/phenix-agent-configurations`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0052 | IN PROGRESS | `rust/crates/phenix-agent-configurations/Cargo.toml` | Refactor | Retain pure product-profile declarations; drop old plugin identity selections when replacements land. | #731 | Main `a4bedc4f`: assembly-role crate depends only on Core; matching lib currently selects legacy hooks. REFACTOR profile assembly with #731; verify no factory-owned fallback. |
| F0053 | IN PROGRESS | `rust/crates/phenix-agent-configurations/src/lib.rs` | Refactor | Advanced defaults currently select legacy phenix.hooks despite spec/kernel-hooks.md retirement contract; replace profile selection. | #731 + hook parity | `agent-configurations/src/lib.rs:43` includes `phenix.hooks` in Advanced defaults; profile expansion and manifest functions at :68/:82/:100-119. Remove deprecated hook default only after Layer/Event behavior parity and selectable profile proof. #765 marks hardcoded default expansion for typed profile replacement, retaining default parity. Annotated at #765 head `cf700548`; CI pending. |

### `rust/crates/phenix-application-interface`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0054 | IN PROGRESS | `rust/crates/phenix-application-interface/Cargo.toml` | Keep | Portable frontend and application contracts. | — | Main `d162f507`: passive library owns fixed typed application descriptor/operations without runtime state. KEEP; verify descriptor snapshot generation in source CI. |
| F0055 | IN PROGRESS | `rust/crates/phenix-application-interface/fixtures/application.rs` | Keep | Portable frontend and application contracts. | — | Main `1bc2ed73`: 1,867-line generated `phenix.application@1` fixture. KEEP generated API conformance fixture; require exact regeneration match, not hand maintenance. |
| F0056 | IN PROGRESS | `rust/crates/phenix-application-interface/src/bin/phenix-application-descriptor.rs` | Keep | Portable frontend and application contracts. | — | Main `67667399`: descriptor CLI writes the generated application interface to stdout/file. KEEP developer tooling; check Nix source/descriptor output parity. |
| F0057 | IN PROGRESS | `rust/crates/phenix-application-interface/src/catalog.rs` | Keep | Portable frontend and application contracts. | — | Main `617c05d0`: `application_descriptor` and `operations!` enumerate typed application operations and capabilities. REFACTOR only when truly selected extensibility needs it; preserve fixed external IDs and generation snapshot. |
| F0058 | IN PROGRESS | `rust/crates/phenix-application-interface/src/client.rs` | Keep | Portable frontend and application contracts. | — | Main `eef3fa20`: `ApplicationTransport` and `ApplicationClient` negotiate typed operations and retain structured failure classes. KEEP; verify capability rejection and independent transport replacement. |
| F0059 | IN PROGRESS | `rust/crates/phenix-application-interface/src/descriptor.rs` | Keep | Portable frontend and application contracts. | — | Main `2007e911`: serializable `ApplicationDescriptor` holds versioned operations, callbacks, events and ordering. KEEP schema contract; test unknown-field rejection and deterministic JSON snapshot. |
| F0060 | IN PROGRESS | `rust/crates/phenix-application-interface/src/generate.rs` | Keep | Portable frontend and application contracts. | — | Main `e3f45a84`: deterministic Rust descriptor emitter validates names, types and capability references. KEEP; run fixture regeneration and invalid-descriptor tests. |
| F0061 | IN PROGRESS | `rust/crates/phenix-application-interface/src/lib.rs` | Keep | Portable frontend and application contracts. | — | Main `71498ae4`: thin unsafe-free application library entry exports catalog/client/descriptor/types, fixed `phenix.application@1` ID. KEEP; confirm downstream ACP/Lua consumers. |
| F0062 | IN PROGRESS | `rust/crates/phenix-application-interface/src/tests.rs` | Keep | Portable frontend and application contracts. | — | Main `b0205a36`: eight tests cover generated parity, typed invocation, optional capability rejection and errors. KEEP; run generated descriptor and client smoke on exact integration head. |
| F0063 | IN PROGRESS | `rust/crates/phenix-application-interface/src/types/discovery.rs` | Keep | Portable frontend and application contracts. | — | Main `fa1b3d70`: discovery payloads declare typed capabilities and authentication descriptors. KEEP portable DTOs; verify ACP client generation and schema IDs. |
| F0064 | IN PROGRESS | `rust/crates/phenix-application-interface/src/types/interaction.rs` | Keep | Portable frontend and application contracts. | — | Main `30c2ff68`: typed permission, elicitation and review DTOs; normalization explicitly rejects compound/invalid schemas, with tests. KEEP; require Lua/ACP round-trip. |
| F0065 | IN PROGRESS | `rust/crates/phenix-application-interface/src/types/log.rs` | Keep | Portable frontend and application contracts. | — | Main `8af01e56`: typed log query cursor/limit/session/execution records. KEEP payload; verify transcript/diagnostic client mapping. |
| F0066 | IN PROGRESS | `rust/crates/phenix-application-interface/src/types/mod.rs` | Keep | Portable frontend and application contracts. | — | Main `92956e52`: application-owned DTO macro and structural error classification; imports ModelId/SkillId from Core. REFACTOR if #731 relocates domain IDs; retain wire/schema identity. |
| F0067 | IN PROGRESS | `rust/crates/phenix-application-interface/src/types/observable.rs` | Keep | Portable frontend and application contracts. | — | Main `2729ff9e`: observable paths, query and update payloads use typed references. KEEP; validate subscription/update callback ordering with Lua frontend. |
| F0068 | IN PROGRESS | `rust/crates/phenix-application-interface/src/types/session.rs` | Keep | Portable frontend and application contracts. | — | Main `da969b6e`: session create/resume/status DTOs with projection round-trip regression. KEEP; verify #725 durable identity and ACP reconnect. |

### `rust/crates/phenix-binding-generator`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0069 | IN PROGRESS | `rust/crates/phenix-binding-generator/Cargo.toml` | Keep | Generate typed foreign-language bindings from declared interfaces. | — | Main `f9a858f9`: descriptor-only passive Lua binding generator crate. KEEP independent codegen package; validate fixture regeneration. |
| F0070 | IN PROGRESS | `rust/crates/phenix-binding-generator/src/lib.rs` | Keep | Generate typed foreign-language bindings from declared interfaces. | — | Main `823cbddd`: Lua code generator reads fixed descriptor, not plugin/runtime imports; deterministic test exists. KEEP; run generated Lua binding diff and snapshot check. |

### `rust/crates/phenix-binding-lua`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0071 | IN PROGRESS | `rust/crates/phenix-binding-lua/Cargo.toml` | Keep | Lua/Neovim integration must remain behind the client boundary. | — | Main `e77e23b1`: passive Lua client-binding crate, distinct from proposed Lua native guest adapter. KEEP; verify linked client package and no runtime plugin ownership. |
| F0072 | IN PROGRESS | `rust/crates/phenix-binding-lua/src/facade.rs` | Keep | Lua/Neovim integration must remain behind the client boundary. | — | Main `27823800`: Lua facade owns polling, repair/replay, sessions, callback and elicitation mapping. KEEP client behavior; inspect duplicated session repair vs backend and test long-lived restart. |
| F0073 | IN PROGRESS | `rust/crates/phenix-binding-lua/src/lib.rs` | Keep | Lua/Neovim integration must remain behind the client boundary. | — | Main `c14b6217`: LuaJIT/Lua 5.1 ACP client runs network worker off Lua host and exposes polling. KEEP client boundary; verify worker close/drop lifecycle and reconnect regression. |
| F0074 | IN PROGRESS | `rust/crates/phenix-binding-lua/src/tools.rs` | Keep | Lua/Neovim integration must remain behind the client boundary. | — | Main `eeb5f16e`: Lua tool registration and facade module connect to typed Add/RemoveClientTool operations. KEEP client-tool bridge; ensure handler lifetime and admission tests. |
| F0075 | IN PROGRESS | `rust/crates/phenix-binding-lua/src/tools_regression.rs` | Keep | Lua/Neovim integration must remain behind the client boundary. | — | Main `ce145519`: four regressions cover Lua handler cleanup, duplicate release and client-tool facade. KEEP tests; run LuaJIT package fixture. |

### `rust/crates/phenix-client-acp`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0076 | IN PROGRESS | `rust/crates/phenix-client-acp/Cargo.toml` | Keep | Typed ACP client implementation. | — | Main `40076ece`: typed ACP client package with generated interface build. KEEP client transport; prove ACP/Lua product negotiation and streaming. |
| F0077 | IN PROGRESS | `rust/crates/phenix-client-acp/build.rs` | Keep | Typed ACP client implementation. | — | Main `a4cba51a`: build script emits descriptor-backed ACP extension client and checks hash. KEEP generator; validate reproducible generated Rust and source CI. |
| F0078 | IN PROGRESS | `rust/crates/phenix-client-acp/src/lib.rs` | Keep | Typed ACP client implementation. | — | Main `f32e961b`: ACP connection negotiates extensions, standard messages and typed application API. KEEP transport client; test callback lifetime, reconnect and streaming with #768 product. |

### `rust/crates/phenix-client`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0079 | IN PROGRESS | `rust/crates/phenix-client/Cargo.toml` | Keep | Transport-neutral application client DTOs. | — | Main `b1f6cb87`: transport-neutral legacy client DTO library depends on domain types only. REVIEW against fixed application interface, no deletion until external consumer and wire-ID audit. |
| F0080 | IN PROGRESS | `rust/crates/phenix-client/src/lib.rs` | Keep | Transport-neutral application client DTOs. | — | Main `47ee4b46`: 706-line `ServiceRequest`/frontend-envelope protocol alongside newer typed application operations. REVIEW overlap/consumers explicitly; plan migration or keep separate transport with parity. |

### `rust/crates/phenix-contract`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0081 | IN PROGRESS | `rust/crates/phenix-contract/Cargo.toml` | Keep | Versioned portable types and canonical typed contributions. | — | Main `e1d400f4`: passive zero-domain contract library; portable shared dependency. KEEP; verify absence of runtime/plugin dependencies in Cargo graph. |
| F0082 | IN PROGRESS | `rust/crates/phenix-contract/src/contract.rs` | Keep | Versioned portable types and canonical typed contributions. | — | Main `0c883713`: shared ContractId/Type/PhenixValue/reference authority structures with schema compatibility. KEEP foundational type model; test exact/project codecs and wire IDs. |
| F0083 | IN PROGRESS | `rust/crates/phenix-contract/src/contract_wire.rs` | Keep | Versioned portable types and canonical typed contributions. | — | Main `262f3cf9`: explicit serde serialization of ContractValue reparses schema, with unit regression. KEEP typed wire encoding; verify cross-language data fidelity. |
| F0084 | IN PROGRESS | `rust/crates/phenix-contract/src/identity.rs` | Keep | Versioned portable types and canonical typed contributions. | — | Main `5172b71d`: macro-backed versioned identifiers, `InterfaceId` and `GenerationId` with ambiguous text/version rejection tests. KEEP canonical IDs; run identifier suite. |
| F0085 | IN PROGRESS | `rust/crates/phenix-contract/src/infallible_value.rs` | Keep | Versioned portable types and canonical typed contributions. | — | Main `9769e857`: Infallible codec maps to `Type::Never`, with regression. KEEP structural type edge case; run contract tests. |
| F0086 | IN PROGRESS | `rust/crates/phenix-contract/src/interface.rs` | Keep | Versioned portable types and canonical typed contributions. | — | Main `0b7b3355`: `InterfaceSchema` has typed request/response/error with directional provider matching. KEEP compatibility checks; verify service-only provider canary #728. |
| F0087 | IN PROGRESS | `rust/crates/phenix-contract/src/lib.rs` | Keep | Versioned portable types and canonical typed contributions. | — | Main `485e7b7a`: unsafe-free wire-stable contract barrel, exports shared schema/value/identity modules. KEEP core-independent contract package; verify dependency direction after migration. |
| F0088 | IN PROGRESS | `rust/crates/phenix-contract/src/std_value.rs` | Keep | Versioned portable types and canonical typed contributions. | — | Main `f53873c9`: standard Rust value codecs include fixed-width checks, JSON lowering, deterministic maps/sets. KEEP; run boundary tests and Lua conversion. |
| F0089 | IN PROGRESS | `rust/crates/phenix-contract/src/structural_value.rs` | Keep | Versioned portable types and canonical typed contributions. | — | Main `bb7d23c1`: `PhenixValue`/schema self-codecs preserve identity and structural type. KEEP; run round-trip tests. |

### `rust/crates/phenix-core`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0090 | IN PROGRESS | `rust/crates/phenix-core/Cargo.toml` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Main `2c6e9f96`: generic Core crate root remains canonical runtime package, tests explicitly controlled in Cargo metadata. KEEP package; audit AI-only imports and test feature closure after #731. |
| F0091 | IN PROGRESS | `rust/crates/phenix-core/src/agent.rs` | Move | AI-only inference/tool/skill/context contracts belong in application contract package, not Core. | #731 + references | `core/src/agent.rs` defines model inference, tools, skills, context requests/responses and four versioned services (`phenix.models.inference@1`, etc.). MOVE domain types without changing serialized schemas, ID strings or SDK consumers; require contract compatibility tests. |
| F0092 | IN PROGRESS | `rust/crates/phenix-core/src/artifact.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Main `5c6ac825`: content-hash ArtifactRevision codec with noncanonical hash rejection tests. KEEP artifact identity; ensure selected generation uses it without second revision format. |
| F0093 | IN PROGRESS | `rust/crates/phenix-core/src/authority.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Main `e5eddbb5`: Authority is permission-set attenuation with tests forbidding privilege regain. KEEP generic authority algebra; prove nested #726 imports and plugin callbacks do not elevate. |
| F0094 | IN PROGRESS | `rust/crates/phenix-core/src/callable.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Main `e6e2ff33`: callable registry checks typed inputs/outputs and retires reference owner generations, with six tests. KEEP generic callable registry; verify #760 callbacks use same reference lifetime. |
| F0095 | IN PROGRESS | `rust/crates/phenix-core/src/component_endpoint_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Main `9cd65fdf`: test resolves exact component endpoint/import and dispatches to selected provider. KEEP canary; run #726 selected native and service-only paths. |
| F0096 | IN PROGRESS | `rust/crates/phenix-core/src/component_reentrancy_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Main `cd1921c4`: tests same-plugin endpoint reentry/cycle rejection but contains `legacy_kernel` fixture. REFACTOR legacy-only fixture after #766 while retaining generic reentrancy coverage. |
| F0097 | IN PROGRESS | `rust/crates/phenix-core/src/composition/activation.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Main `de1a1ae5`: activation validates resolved generation against manifest and layer policy before graph installation, with rejection tests. KEEP admission; prove failed #760 promotion retains active graph. |
| F0098 | IN PROGRESS | `rust/crates/phenix-core/src/composition/component.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Main `59ab7cda`: component graph compiles typed imports, provider plans, DAG constraints and authority. KEEP graph compiler; check for duplicated per-invocation selection after #726 consolidation. |
| F0099 | IN PROGRESS | `rust/crates/phenix-core/src/composition/component_invocation.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Main `18046cb2`: typed `ResolvedImportHandle::invoke_value` delegates to bound graph endpoint with mismatch rejection regression. KEEP single typed path; compare #726 root invoke wrappers for duplicate resolver. |
| F0100 | IN PROGRESS | `rust/crates/phenix-core/src/composition/inspection.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Main `9a8a9b48`: immutable generation inspection exposes component graph, import authority and layer policies without activation. KEEP introspection; check generation and selected descriptor equality. |
| F0101 | IN PROGRESS | `rust/crates/phenix-core/src/composition/manifest.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Main `19be4cf7`: manifests declare artifact, plugin runtime, components, imports, listeners, triggers and services as data. KEEP canonical declarations; #728 contribution envelope must not duplicate schema authority. |
| F0102 | IN PROGRESS | `rust/crates/phenix-core/src/composition/mod.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Main `35b4c13f`: composition module wires activation/component/inspection/manifest/provider resolution. KEEP module layout; check no stale modules after #726 moves. |
| F0103 | IN PROGRESS | `rust/crates/phenix-core/src/composition/provider_resolution.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Main `44c982fd`: provider policy defines explicit bindings, enable/disable, priorities and opt-in fallback, with tests. KEEP policy; verify no implicit fallback executor or alternate model route. |
| F0104 | IN PROGRESS | `rust/crates/phenix-core/src/composition/registry.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Main `3fb6bf80`: KernelConfig owns service/layer dispatch plans and plugin runtime bindings; `resolve_chain` and `resolve_bound_chain` coexist. SIMPLIFY candidate if #726 invokes overlapping resolution per call; keep one cached plan. |
| F0105 | IN PROGRESS | `rust/crates/phenix-core/src/composition/resolver.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Main `6e1910df`: 2,133-line ResolvedGeneration constructs configuration, graph, resources, policies and dispatch topology with many `resolve_with_*` wrappers. SIMPLIFY entry API after #728/#760, preserve canonical generation identity and non-agent bootstrap. |
| F0106 | IN PROGRESS | `rust/crates/phenix-core/src/configuration/mod.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Main `566206ca`: configuration module only exports resolution. KEEP thin module; no duplicate policy here. |
| F0107 | IN PROGRESS | `rust/crates/phenix-core/src/configuration/resolution.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Main `6e1044eb`: configuration resolver merges typed namespaced contributions by precedence, validates declared sources and attenuates frontend authority. KEEP generic config; verify third-party and Nix/Lua equivalence. |
| F0108 | IN PROGRESS | `rust/crates/phenix-core/src/configuration_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Main `688b4b94`: six regressions cover deterministic config merge, source conflicts, authority and equivalent Nix/Lua generation. KEEP; run branch/integration suite after topology selection. |
| F0109 | IN PROGRESS | `rust/crates/phenix-core/src/content_reference.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Main `26c3ccf6`: content-addressed file store verifies hashes and safe relative paths. KEEP generic content refs; test path traversal and durable restart. |
| F0110 | IN PROGRESS | `rust/crates/phenix-core/src/events.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Main `a7380d8a`: EventBus owns generation-scoped subscriptions, admission, backpressure and delivery status. KEEP generic event engine; #767 must show hook retirement preserves dispatch/authority and cancellation. |
| F0111 | IN PROGRESS | `rust/crates/phenix-core/src/host_authority_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Main `fb20c0d3`: delegation/retry test verifies denied permission cannot be regained. KEEP host authority regression; combine with #726 nested/guest provider test. |
| F0112 | IN PROGRESS | `rust/crates/phenix-core/src/invalid_candidate_activation_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Main `2fe9bbc1`: two regressions assert invalid candidate and removed live provider leave active generation intact. KEEP promotion canary; rerun on #760 with owner-forged contribution. |
| F0113 | IN PROGRESS | `rust/crates/phenix-core/src/invocation.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Main `74833518`: typed InvocationOutcome/Failure maps generic Core errors; test `legacy_success_transport_stays_bare` exposes old transport compatibility. REFACTOR legacy outcome path only after #731/ACP clients migrate; retain structured failures. |
| F0114 | IN PROGRESS | `rust/crates/phenix-core/src/layer_regression.rs` | Keep | Preserve generic authority, dispatch, generation, event, persistence and parity regressions. | — | Main `a083aeb8`: layer tests show direct terminal equivalence and failure short-circuit. KEEP layer canary; rerun with workflow nested import/pending callback. |
| F0115 | IN PROGRESS | `rust/crates/phenix-core/src/lib.rs` | Refactor | Retain generic Core public API, remove AI-specific reexports after domain contracts migrate. | #731 | Main `078d0532`: Core exports `agent` module alongside generic registry, invocation, events, persistence and tasks. MOVE AI contract exports via #731; preserve generic API and downstream client wire IDs. |
| F0116 | IN PROGRESS | `rust/crates/phenix-core/src/logging.rs` | Keep | Generic authority, lifecycle, composition, graph, invocation, reconciliation and inspection. | — | Main `1260c981`: structured logger supports log sinks, detail modes, paged readers and content-ref storage. KEEP generic diagnostics; verify ACP/Lua transcript reading and structured event categories. |
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
| F0168 | IN PROGRESS | `rust/crates/phenix-core/src/sdk.rs` | Review | Generic SDK observable machinery may remain in Core; separate public SDK integration from kernel authority. | #728 + API ownership review | `core/src/sdk.rs` implements SDK namespace/value and observable resolution, ownership validation and callable registration. Investigate whether public SDK metadata can move without moving Core-owned capability/authority enforcement. Proof: no alternate capability registry. |
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
| F0196 | IN PROGRESS | `rust/crates/phenix-harness/src/application.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | Harness application owns request/session/product transport. #768 `1784d1c` now deletes the alternate `AgentLoopInterface` service import, service/provider selection, live dispatch branch and its foreign-service-only fixture. Only selected pinned `agent.turn` topology runs, with pre-admission missing-plan rejection. The existing 4-profile declarative test covers substitution, provider cancellation, normal result and durable replay; streaming, prompt receipts, side effects, current-head tests and final portable EntryBinding are still open. #766 `3daa7b4` and #767 `ed19dba` had already removed test-only legacy provider selection. |
| F0197 | TODO | `rust/crates/phenix-harness/src/authority.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | pending |
| F0198 | IN PROGRESS | `rust/crates/phenix-harness/src/basic_suite.rs` | Move | Move hardcoded echo/basic product setup to data-driven reference profiles and fixtures. | #731 | `harness/basic_suite.rs:14-29` hand-adds session, echo model, tools, skills and context factories. Candidate MOVE to selected product descriptors; retain deterministic Basic fixture but remove echo from production default after independent real-model test. #765 marks manual Basic echo selection for fixture migration after real Basic tests. Annotated at #765 head `cf700548`; CI pending. |
| F0199 | TODO | `rust/crates/phenix-harness/src/bin/phenix-acp-fixture.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | pending |
| F0200 | TODO | `rust/crates/phenix-harness/src/context_recovery.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | pending |
| F0201 | TODO | `rust/crates/phenix-harness/src/exact_selected_suite_tests.rs` | Refactor | Keep product parity assertions but eliminate fixtures coupled to old hardcoded plugin registration. | #731 | pending |
| F0202 | TODO | `rust/crates/phenix-harness/src/lib.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | pending |
| F0203 | IN PROGRESS | `rust/crates/phenix-harness/src/main.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | #766 [CI 38031696729](https://github.com/matthis-k/phenix-ai/actions/runs/38031696729) proved a CLI test still required a deleted imperative loop's application tool adapter. #766 `ff60387`, #767 `8285aad` and #768 `3ddb504` now verify that disabling the application adapter yields a generic Core graph with **no** selected agent workflow. Missing entry fails at application prompt preflight; no implicit fallback. Await exact-head source, binary tests and packaged CLI proof. |
| F0204 | TODO | `rust/crates/phenix-harness/src/model_surface_fixture.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | pending |
| F0205 | TODO | `rust/crates/phenix-harness/src/persistence.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | pending |
| F0206 | TODO | `rust/crates/phenix-harness/src/runtime.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | pending |
| F0207 | IN PROGRESS | `rust/crates/phenix-harness/src/runtime_builder.rs` | Refactor | Manual add_selected/plugin-factory roster should lower from selected manifests/contributions. | #728, #730, #731 | `harness/runtime_builder.rs:131-177` calls `add_embedded` for first-party plugin roster, including old agent loop, hook and model services; `:215+` selects by hardcoded IDs. Move to selected descriptor discovery with external provider canary. #765 marks manual embedded factory roster for selected descriptor replacement in #731. Annotated at #765 head `cf700548`; CI pending. |
| F0208 | TODO | `rust/crates/phenix-harness/src/runtime_config.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | pending |
| F0209 | IN PROGRESS | `rust/crates/phenix-harness/src/tests.rs` | Refactor | Retain product entry point; shed manual plugin registration and redundant runtime decisions. | #730, #731 | #766 commit `01f5a74` and stacked #767 `9722784` remove fixture selection exclusions for deleted `phenix.agent-loop`, keeping declarative node override and Basic profile tests. [Previous #766 CI](https://github.com/matthis-k/phenix-ai/actions/runs/38029939148) failed those fixtures; current-head rerun and full parity L766-03/04 pending. |
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
| F0220 | IN PROGRESS | `rust/crates/phenix-model-adapter-acp/src/mcp_bridge.rs` | Keep | ACP model-provider adapter implementation. | — | BREAKING #765 (`d2d64a42`): removed old MCP 2025 protocol downgrade and response formatting; discovery and requests use 2026-07-28 only. Prior format tests deleted. Source/Clippy rerun required. |
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
| F0233 | IN PROGRESS | `rust/crates/phenix-plugin-api/Cargo.toml` | Refactor | Retain application-facing API, but use tool/skill kind-provider contracts instead of duplicated catalogs. | #730, #731 | KEEP/REFACTOR candidate. `phenix-plugin-api/Cargo.toml` blob `0f4ed80d` marks a runtime plugin, links application-interface/Core/SDK and uses Basic Skills, Context, Execution, Options and Sessions only as dev dependencies. Check portable third-party authoring and no hidden product fallback after #731. |
| F0234 | IN PROGRESS | `rust/crates/phenix-plugin-api/src/lib.rs` | Refactor | Retain application-facing API, but use tool/skill kind-provider contracts instead of duplicated catalogs. | #730, #731 | REFACTOR candidate. `phenix-plugin-api/src/lib.rs` blob `45331e3b` implements a `phenix.api` SDK facade over selected Session/Options/Execution/Skills services and publishes versioned SDK exports. It is first-party bridge behavior, not generic Core mechanism. Keep service identity stable while moving authoring to typed artifact contributions; test provider swaps. |
| F0235 | IN PROGRESS | `rust/crates/phenix-plugin-api/src/tests.rs` | Refactor | Retain application-facing API, but use tool/skill kind-provider contracts instead of duplicated catalogs. | #730, #731 | KEEP candidate. `phenix-plugin-api/src/tests.rs` blob `3b685abc` builds an actual Core selected graph from Session/Options/Execution/Basic Skills factories. The skills test checks provenance `phenix.skills@1:`. Preserve this semantics canary while replacing hardcoded embedded registration; run SDK tests at exact selected-artifact head. |

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
| F0240 | IN PROGRESS | `rust/crates/phenix-plugin-basic-agent/Cargo.toml` | Refactor | Retain only if new topology/node provider crate reuses package; remove obsolete loop package afterward. | #726, #731 | DELETED on stacked #766 (`1b17c81a`) along with entire imperative `phenix-plugin-basic-agent` crate. Workspace, lock, Nix package, catalog and Harness references removed; merge after #726 and exact-head CI. |
| F0241 | IN PROGRESS | `rust/crates/phenix-plugin-basic-agent/src/agent_loop.rs` | Retire | Replace imperative run progression with topology plugin and independently provided nodes; preserve parity tests. | #726, #731 | DELETED on stacked #766 (`1b17c81a`), not left as old executable. Default Harness now selects agent-topology and basic-agent-nodes. Verify turn/tool and cancellation; no fallback executor or compatibility alias. |
| F0242 | IN PROGRESS | `rust/crates/phenix-plugin-basic-agent/src/component_regression.rs` | Move | Rehome valuable agent-loop behavior proofs in the topology/node integration suite. | #726, #731 | DELETED with old agent crate in #766 (`1b17c81a`). The replacement topology and Core workflow tests own the new execution contract; review distinct regression assertions before closing acceptance. |
| F0243 | IN PROGRESS | `rust/crates/phenix-plugin-basic-agent/src/lib.rs` | Refactor | Preserve agent-loop semantics in topology/node plugins; remove imperative progression after parity. | #726, #731 | DELETED with old agent crate in #766 (`1b17c81a`). SDK active agent command/result types retained because new nodes use them, not as legacy adapter. |

### `rust/crates/phenix-plugin-basic-context`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0244 | TODO | `rust/crates/phenix-plugin-basic-context/Cargo.toml` | Keep | Minimal replaceable context provider useful as a baseline and fixture. | — | pending |
| F0245 | TODO | `rust/crates/phenix-plugin-basic-context/src/lib.rs` | Keep | Minimal replaceable context provider useful as a baseline and fixture. | — | pending |

### `rust/crates/phenix-plugin-basic-model`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0246 | IN PROGRESS | `rust/crates/phenix-plugin-basic-model/Cargo.toml` | Retire | Delete production crate manifest after echo implementation and references move to fixtures. | #731 + migrate echo fixture | Echo model package remains in production dependency graph through basic-agent reexports and harness Basic suite. Retire crate only after W035, #731 and fixture relocation update Cargo/Nix and downstream imports. |
| F0247 | IN PROGRESS | `rust/crates/phenix-plugin-basic-model/src/lib.rs` | Move | Preserve deterministic echo implementation as test-support fixture, not a production plugin. | #731 | `basic-model/src/lib.rs:13-35` is deterministic echo provider, `:41-55` public factory/manifest. MOVE to test fixture owned by consumer, replace production default with real model selection; preserve deterministic tests. #765 marks deterministic echo provider for fixture-only relocation after real-model product tests. Annotated at #765 head `cf700548`; CI pending. |

### `rust/crates/phenix-plugin-basic-skills`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0248 | IN PROGRESS | `rust/crates/phenix-plugin-basic-skills/Cargo.toml` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | KEEP/REFACTOR candidate. `phenix-plugin-basic-skills/Cargo.toml` blob `e1500806` defines a separately packaged runtime plugin depending on Core, SDK and serde_json. Keep its behavior, replace compile-time selection only after portable artifact registration, and verify Basic/Full skill discovery. |
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
| F0265 | IN PROGRESS | `rust/crates/phenix-plugin-basic-skills/src/lib.rs` | Refactor | Become a standard plugin-defined skill kind/template consumer/provider. | #730, #731 | REFACTOR candidate. `phenix-plugin-basic-skills/src/lib.rs` blob `e73b38a2` embeds 13 named skill files with `include_bytes!`, requires the `write` skill and exports `phenix.skills@1` with durable skill IDs. Preserve resource behavior and provider independence; test listing, enablement, persistence and third-party skill replacement after #731. |

### `rust/crates/phenix-plugin-basic-tools`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0266 | TODO | `rust/crates/phenix-plugin-basic-tools/Cargo.toml` | Refactor | Become a standard plugin-defined tool kind/template consumer/provider. | #730, #731 | pending |
| F0267 | TODO | `rust/crates/phenix-plugin-basic-tools/src/lib.rs` | Refactor | Become a standard plugin-defined tool kind/template consumer/provider. | #730, #731 | pending |

### `rust/crates/phenix-plugin-catalog`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0268 | IN PROGRESS | `rust/crates/phenix-plugin-catalog/Cargo.toml` | Retire | Delete static roster crate manifest once first-party discovery/fixtures move. | #728, #730, #731 | Static catalog Cargo package remains imported by Harness and Basic setup. Retire manifest only after dynamic selection replaces all exports, durable schema projection, workspace/Cargo/Nix registration and test consumers. |
| F0269 | IN PROGRESS | `rust/crates/phenix-plugin-catalog/src/lib.rs` | Retire | Remove manually maintained first-party plugin re-export/registration catalog once dynamic discovery works. | #730, #731 | `catalog/src/lib.rs:7-212` reexports and registers a large manual first-party provider roster; `:237` does special durable schema projection. RETIRE after #728/#730 descriptors supply both roles and #731 proves unknown third-party installation. #765 marks static roster for descriptor-based discovery and consumer replacement in #731. Annotated at #765 head `cf700548`; CI pending. |
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
| F0297 | IN PROGRESS | `rust/crates/phenix-plugin-execution/src/agent_loop_regression.rs` | Move | Transfer behavioral assertions to resolved workflow integration tests before removing old loop. | #726, #731 | DELETED in #766 (`1b17c81a`); this test module exercised the removed imperative runner. New topology and workflow tests remain. Audit lost unique scenarios against #726 regression suite. |
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
| F0314 | IN PROGRESS | `rust/crates/phenix-plugin-execution/src/tool_schedule.rs` | Move | Consolidate concurrency planning with generic graph scheduling or tool-policy plugin; avoid a second scheduler. | #729, #731 | `execution/tool_schedule.rs:4-41` defines tool concurrency policy (ParallelSafe or Exclusive) and batch planner; tests at :89/:108 protect no arbitrary cap and exclusive-by-default. Move policy to plugin as needed; do not drop exclusivity in graph scheduler. #765 marks old tool batch driver; preserve Exclusive/ParallelSafe, no-default-limit behavior under Core plans. Annotated at #765 head `cf700548`; CI pending. |
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
| F0320 | IN PROGRESS | `rust/crates/phenix-plugin-hooks/Cargo.toml` | Retire | Legacy hook dispatcher; use kernel Service Layers and Events with plugin-owned behavior. | #729, #731 + listener parity | DELETED in #767 (`ba91b3c`): hook crate manifest and Cargo dependency removed. No replacement compatibility package. Event/Layer checks and exact-head CI pending. |
| F0321 | IN PROGRESS | `rust/crates/phenix-plugin-hooks/src/component.rs` | Retire | Legacy hook dispatcher; use kernel Service Layers and Events with plugin-owned behavior. | #729, #731 + listener parity | DELETED in #767 (`ba91b3c`): HookInterface component manifest removed; Core events and service layers remain generic contracts. |
| F0322 | IN PROGRESS | `rust/crates/phenix-plugin-hooks/src/implementation.rs` | Retire | Legacy hook dispatcher; use kernel Service Layers and Events with plugin-owned behavior. | #729, #731 + listener parity | DELETED in #767 (`ba91b3c`): legacy hook command/action dispatcher removed, not migrated into a parallel dispatcher. |
| F0323 | IN PROGRESS | `rust/crates/phenix-plugin-hooks/src/lib.rs` | Retire | Legacy hook dispatcher; use kernel Service Layers and Events with plugin-owned behavior. | #729, #731 + listener parity | DELETED in #767 (`ba91b3c`): crate root and reexports removed from product catalog. |
| F0324 | IN PROGRESS | `rust/crates/phenix-plugin-hooks/src/ownership_regression.rs` | Retire | Retain layer/listener ownership regression under canonical Core/SDK tests before retiring hook crate. | #729, #731 + listener parity | DELETED in #767 (`ba91b3c`): old dispatcher-only ownership regression. Verify equivalent generic service Layer and Event owner tests; no old Hook protocol fixture. |

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
| F0360 | IN PROGRESS | `rust/crates/phenix-plugin-models/src/implementation_sdk.rs` | Keep | Model selection, routing and provider behavior stays application-side. | — | BREAKING #765 (`c13199ad4`, later formatting at `d2d64a42`) removed `normalize_legacy_profile`. Persistence reads now strictly decode canonical routing records; no schema migration reader or compatibility fallback. |
| F0361 | IN PROGRESS | `rust/crates/phenix-plugin-models/src/implementation_sdk/catalog.rs` | Keep | Model selection, routing and provider behavior stays application-side. | — | BREAKING #765: removed provider catalog manifest normalization; owner and profile records decode without historical coercion. CI on `d2d64a42` pending. |
| F0362 | IN PROGRESS | `rust/crates/phenix-plugin-models/src/implementation_sdk/packaged.rs` | Keep | Model selection, routing and provider behavior stays application-side. | — | BREAKING #765: removed packaged manifest normalization and automatic ownership adoption of pre-existing matching profiles. Foreign IDs now reject even on initial packaging. |
| F0363 | IN PROGRESS | `rust/crates/phenix-plugin-models/src/implementation_sdk/tests.rs` | Keep | Model selection, routing and provider behavior stays application-side. | — | BREAKING #765: removed old raw-option and legacy adoption tests; replaced adoption test with explicit foreign-ownership rejection. No compatibility fixtures retained. |
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
| F0382 | IN PROGRESS | `rust/crates/phenix-plugin-session-tree/Cargo.toml` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | KEEP candidate. Manifest declares independent `runtime-plugin`; session-tree consumes phenix-plugin-sessions via typed imports. Retain independently selectable package until integration proves its behavior can be replaced without collapsing session ownership. |
| F0383 | IN PROGRESS | `rust/crates/phenix-plugin-session-tree/src/component.rs` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | `session-tree/component.rs:16-40` exports its own component, importing flat `SessionInterface` and mutations. Tests :69/:86 check missing flat session fails and selected import binds correctly. Independent plugin boundary appears intentional; pending exact-head CI. |
| F0384 | IN PROGRESS | `rust/crates/phenix-plugin-session-tree/src/disable_independently_regression.rs` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | `disable_independently_regression.rs:12` proves flat sessions remain when optional tree plugin is omitted. Keep this proof as a non-agent replaceability gate, even if tree package later moves. Exact-head integration result pending. |
| F0385 | IN PROGRESS | `rust/crates/phenix-plugin-session-tree/src/implementation.rs` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | `implementation.rs:13` has versioned `phenix.session-tree@1`; manifest :65-83 supplies its own service and a Layer on flat sessions. Distinct hierarchical behavior and durable mutations support separate plugin ownership; verify Layer/event semantics. |
| F0386 | IN PROGRESS | `rust/crates/phenix-plugin-session-tree/src/interface.rs` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | `interface.rs:4-13` defines independent `SessionTreeInterface` and schema for `phenix.session-tree@1`. KEEP candidate; preserve ID/payload compatibility if plugin is renamed or consolidated. |
| F0387 | IN PROGRESS | `rust/crates/phenix-plugin-session-tree/src/lib.rs` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | `lib.rs` reexports component, implementation and interface plus tests. No independent orchestration engine; retain module root as long as tree service remains replaceable and Cargo dependencies resolve. |
| F0388 | IN PROGRESS | `rust/crates/phenix-plugin-session-tree/src/session_layering_regression.rs` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | `session_layering_regression.rs:63/97/122` tests delegation, disabled optional tree Layer and required Layer fail-closed. Preserve these exact semantics when retiring legacy hook mechanisms; verify current test suite. |
| F0389 | IN PROGRESS | `rust/crates/phenix-plugin-session-tree/src/session_tree_atomicity_regression.rs` | Review | Separate only if fork/tree behavior requires independently replaceable contracts beyond sessions. | #731 + consumer map | `session_tree_atomicity_regression.rs:220/263/308+` checks rejected reparent/cycle and transactional lineage behavior. This is unique durable data safety coverage; preserve test under any moved implementation. |

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
| F0396 | IN PROGRESS | `rust/crates/phenix-plugin-step-runner/src/runner.rs` | Refactor | Limit to one step invocation; remove any graph/run progression authority. | #726, #729 | `step-runner/runner.rs:245+` handles delegated workers/tasks, reservations, retry and step execution. Audit whether these are domain tool semantics or a second control-flow owner; transfer only graph progression after equivalent delegated-worker tests. #765 marks delegated worker/policy owner; move duplicated graph progression only after cancellation, budget and no-replay tests. Annotated at #765 head `cf700548`; CI pending. |
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
| F0420 | IN PROGRESS | `rust/crates/phenix-sdk-macros/src/plugin_attr.rs` | Keep | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | #765 eliminates the eight-line forwarding shim and installs the unchanged old implementation directly at this path; KEEP after macro compilation and authoring tests. Annotated at #765 head `cf700548`; CI pending. |
| F0421 | TODO | `rust/crates/phenix-sdk-macros/src/plugin_attr_core.rs` | Keep | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | pending |
| F0422 | IN PROGRESS | `rust/crates/phenix-sdk-macros/src/plugin_attr_legacy.rs` | Retire | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | DELETED in #765 (`cf700548`): `plugin_attr_legacy.rs` implementation moved into canonical `plugin_attr.rs`, with eight-line forwarding shim removed. Mark REMOVED after merge and macro CI. |
| F0423 | TODO | `rust/crates/phenix-sdk-macros/src/resource_attr.rs` | Keep | Generic static annotation lowering; no hardcoded tool/skill kind registration. | — | pending |

### `rust/crates/phenix-sdk`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0424 | TODO | `rust/crates/phenix-sdk/Cargo.toml` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | pending |
| F0425 | IN PROGRESS | `rust/crates/phenix-sdk/README.md` | Refactor | General Rust plugin authoring API; migrate AI domain contracts to application-owned contract crates. | #728, #730, #731 | KEEP/REFACTOR candidate. `phenix-sdk/README.md` blob `57ab593f` documents typed provider endpoint/protocol/auth declarations and provider-private credential operations. Check that examples compile against selected portable contributions; update only stale authoring paths after #728/#731. |
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
| F0438 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/agent_diagnostics.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `89739d51` defines versioned `phenix.agent.diagnostic` event, not a generic Core event mechanism. Put event vocabulary in agent-owned contract package; preserve event name/version and existing subscribers, then test emitted payloads. |
| F0439 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/agent_loop.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | SDK agent_loop contract carries agent turn/tool state and service schema. MOVE to plugin-owned domain contract with wire ID stability, test old JSON/tool requests and implement replacement imports before retiring. #765 marks legacy agent-domain contracts for later migration with stable wire identity. Annotated at #765 head `cf700548`; CI pending. |
| F0440 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/budget.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `81b5753d` owns `RootBudgetLedger` reserve/settle/release policy and remaining-budget calculations. Preserve typed reservation limits and budget regressions in an execution/model policy plugin; Core should only enforce granted authority, not agent token budgets. |
| F0441 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/context.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `b8127a57` defines `phenix.context@1`, `phenix.context-recovery@1`, projection types and `ContextInterface`. Move service schema to a context-owned contract crate; preserve IDs and serialized request/response shapes for SDK, client and recovery consumers. |
| F0442 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/context_admission.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `c0a712a8` implements `ContextAdmissionRequest::admit` and retention/cache-placement policy. Keep admission distinct from Core resource persistence; relocate with context plugin and test budget reservation and candidate ordering at equal inputs. |
| F0443 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/context_compaction.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `8552fc71` defines checkpoint/compaction decisions and `choose_cache_aware_compaction`, with validation rules. Keep in context/memory policy package, not Core; preserve deterministic choice and checkpoint replay tests. |
| F0444 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/context_recovery_bootstrap.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `ed8bca3a` defines `recovery_cold_gate`, deadline policy and classification checks. Relocate into context recovery plugin contracts. Validate durable restart and stale-decision rejection before retiring SDK re-exports. |
| F0445 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/delegation.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `a40fa60a` defines delegation task/result envelopes and resource/deadline validation, with dependencies on model routing. Relocate with execution/delegation plugin; verify authority and budget preservation across worker result return. |
| F0446 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/efficiency_evaluation.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `d2c1956b` defines `phenix.efficiency-evaluation@1` and `phenix.efficiency-outcome-evidence@1`, plus typed task/charge evidence. This is a large first-party evaluation contract, not Core machinery; retain IDs and tests under its plugin owner before removing SDK compatibility exports. |
| F0447 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/environment.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `5fcd290a` defines `phenix.environment@1`, filesystem policy and process-stream recovery commands. Keep the stable Environment wire identity while assigning its types to environment provider contracts; test local/remote implementations and permissions. |
| F0448 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/execution.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `dbcb439b` defines `phenix.execution@1` and `phenix.execution.inspect@1`, worker/record schemas and `ExecutionAuthority`. Own as an execution plugin contract; test selected provider/service replacement, inspection and authority attenuation. |
| F0449 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/execution_resources.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `26a3299f` defines `phenix.execution.resources@1` for delegated reservation and resource commands. Keep external contract IDs and ownership in execution plugin; validate resource leases and cancellation on client-reachable operations. |
| F0450 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/exploration.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `22074323` implements exploration admission, resource command lowering and opportunity assessment. This is execution/delegation policy; relocate alongside its plugin and keep negative limits/deadline tests. |
| F0451 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/frontend.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `cfd988bc` defines `phenix.frontend-services@1`, live frontend provider descriptors and request/response schemas. Preserve client operation IDs while placing contracts with frontend plugin; verify ACP/Lua descriptor mapping. |
| F0452 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/jobs.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `ce2326f7` defines `phenix.jobs@1`, job command and runtime resource status types. Distinguish generic Core task mechanics from job product policy; move schema to Jobs plugin and retain restart/status/cancel conformance. |
| F0453 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/language.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `cb283aec` defines `phenix.language@1`, typed document identity, provider epoch and code facet revisions. Move language operations/schema to language plugin; preserve provider-epoch stale-result rejection and client wire serialization. |
| F0454 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/memory.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `9ecc47e1` defines `phenix.memory@1`, embed/rank/compact/expand endpoints and summarization callables. Own these service contracts under memory plugin; keep all published IDs and retrieval/replay tests during SDK relocation. |
| F0455 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/memory_context.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `917b8278` defines `memory.context@1`, association evidence and `resolve_recall`. This is memory policy with deterministic recall ordering; relocate to memory/context owner and retain provenance/evidence rejection tests. |
| F0456 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/memory_freshness.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `53adf842` defines `memory.validate`, dependency revisions and stale-reference outcomes. Move code-memory policy with memory plugin; preserve serialized revision identity and stale/fresh decision canaries. |
| F0457 | TODO | `rust/crates/phenix-sdk/src/contracts/mod.rs` | Refactor | Remove re-exports of relocated agent-domain contract definitions. | #731 | pending |
| F0458 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/model_dispatch.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `048ade7b` defines `phenix.models.dispatch@1` and typed prepared dispatch/failure results. Put model routing contracts with models plugin, retain versioned wire ID and provider substitution regression. |
| F0459 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/models.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | BREAKING #765: `ModelTarget.options` requires typed `PhenixValue` JSON; removed untagged raw-JSON option deserializer and old-format acceptance test. Move generic model domain contract only with #731. |
| F0460 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/options.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `4fc29959` defines `phenix.options@1`, validated OptionKey/Subject IDs and precedence/layer policy. Keep contract with options plugin, test precedence and durable scope consistency before SDK re-export retirement. |
| F0461 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/planning.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `539775c1` defines `phenix.planning@1`, objective/plan/history command types and state. Own planning policy in planning plugin, keep serialized IDs and plan lifecycle tests. |
| F0462 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/primitive_agent_export.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `4c80b327` contains 1,528 lines of continuation export packet, recipient, evidence, budget and delta policy. Place export semantics with context/delegation plugin; preserve schema version 1 and negative size/freshness validation. |
| F0463 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/sessions.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `e714f096` publishes `phenix.sessions@1` and `phenix.sessions.mutation@1` with history, journal and completion payload types. Move to sessions plugin-owned contract crate, preserve ACP/Lua bindings and durable replay. |
| F0464 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/skills.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `d04071d9` is an 18-line typed interface wrapper for `phenix.skills@1` imported from Core. Keep service wire identity, re-export from skill plugin contracts; this file may vanish once SDK contract ownership migrates. |
| F0465 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/step_attempt.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `abeeaaf7` defines `phenix.execution.attempts@1` and step attempt transition rules. Keep with execution attempt plugin, validate failure/terminal transitions after relocation. |
| F0466 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/step_runner.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `2d545faf` publishes invocation/default/helper/clock, `phenix.step-runner@1` and delegated worker contracts. Split owned invocation vs step scheduling in plugin contract packages; preserve IDs and tests before deleting duplicated runners. |
| F0467 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/step_transaction.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `37b4bf1f` defines `phenix.execution.step-transaction@1` transaction commands. Move with execution step transaction plugin, preserve atomicity and failure semantics. |
| F0468 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/tool_observation.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `13ee7009` owns tool observation provenance, invalidation, validation and reuse. Keep agent tool/context policy out of Core; retain exact-source rejection and side-effect observation tests. |
| F0469 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/usage.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `b4ef0161` owns budgets, model feature limits and usage ledger aggregation. Move to model/execution policy contracts; retain accounting correctness and serialization tests. |
| F0470 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/usage_policy.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `fa2141ba` defines usage, reasoning/tool/skill/retry budgets and planning. Move to model policy plugin, preserve `tools.deferred_schemas` feature and limit-denial tests. |
| F0471 | IN PROGRESS | `rust/crates/phenix-sdk/src/contracts/workspace.rs` | Move | Domain-specific application contracts should live with owning agent/tool/skill/provider contract packages; SDK stays general. | #730, #731 | MOVE candidate. Blob `2ce6545b` defines `phenix.workspace@1`, file commit receipts/version conflicts and write atomicity. Place workspace protocol with workspace plugin; preserve optimistic concurrency, path and permission tests. |
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
| F0489 | IN PROGRESS | `rust/deny.toml` | Review | Unrecognized tracked artifact; verify purpose, ownership and consumer before removal. | consumer trace | `rust/deny.toml` sets cargo-deny graph/advisory/license/source policy. `.github/workflows/dependency-security.yml` runs `cargo deny check licenses sources`. Live security-consumer proof; KEEP pending workflow execution and policy review. |

### `scripts/`

| ID | State | Exact path | Provisional decision | Explicit check | Prerequisite | Decision / current-head proof |
| --- | --- | --- | --- | --- | --- | --- |
| F0490 | IN PROGRESS | `scripts/check-plugin-architecture.sh` | Move | Move enforcement/check logic into Nix-supported dev tooling, retaining coverage before retiring standalone shell script. | Nix check replacement + CI proof | Nix `modules/development.nix` calls `scripts/check-plugin-architecture.sh`. Move policy to phenix-flake-ci maintenance leaf only after matching success and negative fixture results; keep script until exact check parity. |
| F0491 | IN PROGRESS | `scripts/check-rust-safety-policy.sh` | Move | Move enforcement/check logic into Nix-supported dev tooling, retaining coverage before retiring standalone shell script. | Nix check replacement + CI proof | Script `86183104` reads Cargo metadata to enforce `#![forbid(unsafe_code)]` across production targets; `modules/development.nix` invokes it. MOVE only after an equivalent Nix-owned rule, negative fixture and CI proof; current live consumer blocks deletion. #765 marks shell runner for move into flake-ci maintenance after equivalent negative and target-discovery checks. Annotated at #765 head `cf700548`; CI pending. |
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
| F0516 | IN PROGRESS | `spec/followups/call-scope-endpoints.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Status `planned`; explicit CallScope and InvocationStack proposal with all 9 acceptance boxes still unchecked. Cross-check against #726 `RootExecutionHandle`, tasks and canonical dispatch before merging invariants into kernel RFC; archive only after test parity. |
| F0517 | IN PROGRESS | `spec/followups/core-module-boundaries.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Status `planned`; proposes contract crate/import ownership and semantic Core modules. `phenix-contract` already exists on main, so plan is partly stale. Diff every remaining unchecked target against current source and kernel RFC before archive. |
| F0518 | IN PROGRESS | `spec/followups/memory-retrieval.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Status `partial`; lexical retrieval and rank interfaces marked implemented, but Tantivy benchmark decision, deterministic index rebuild, failure policy and write-amplification measurements remain unchecked. Keep open and assign memory owner. |
| F0519 | IN PROGRESS | `spec/followups/observable-handles.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Status `implemented`, parent #607; all acceptance boxes are checked for stable observable handles, counter exhaustion and non-adopted generic crates. Candidate ARCHIVE after confirming Core tests/source; fold enduring decisions into `spec/observable-values.md`. |
| F0520 | IN PROGRESS | `spec/followups/plugin-durable-storage.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Status `planned`, parent #520; all durable collection migration and write-amplification checks remain unchecked. Keep open with session/execution/jobs/planning/memory/artifacts paths listed under its migration inventory; do not delete on design status. |
| F0521 | IN PROGRESS | `spec/followups/resolved-dispatch-plan.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Status `active`; states generation dispatch hot path migrated but exact-head validation pending. Main `composition/registry.rs` has `ResolvedDispatchTopology`; align open resolver deletion and Layer continuation tests with #726 canonical plan before archiving. |
| F0522 | IN PROGRESS | `spec/followups/token-cache-provider-integration.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Status `implementation-in-progress`; most cache control/usage fixtures checked, provider-native compaction explicitly split to #611. Confirm remaining fixture/CI claims and consolidate with `spec/token-efficiency.md` after #611 gate. |
| F0523 | IN PROGRESS | `spec/followups/token-code-entity-lineage.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Status `implemented`; language code entity lineage and revision map are marked complete with restart/rename/DocumentSymbol tests. Candidate ARCHIVE after verifying current language-plugin coverage and moving stable identity rules to canonical spec. |
| F0524 | IN PROGRESS | `spec/followups/token-efficiency-evaluation.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Status `implemented`; efficiency evaluation service and outcome provider marked complete. Candidate ARCHIVE after current `phenix-plugin-efficiency-evaluation` contracts/tests and missing latency metric decision are checked. |
| F0525 | IN PROGRESS | `spec/followups/token-isolated-exploration-runtime.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Status `implemented`; delegated exploration admission, worker lineage and context result readmission marked complete. Candidate ARCHIVE after current planning, execution and step-runner integration proofs. |
| F0526 | IN PROGRESS | `spec/followups/token-lazy-tools-observations.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Status `implemented`; lazy tool catalog, artifact recovery and compact observation semantics marked complete. Candidate ARCHIVE after Basic topology/node implementation preserves exact tool/result and schema loading behavior. |
| F0527 | IN PROGRESS | `spec/followups/token-memory-code-dependencies.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Status `implementation-in-progress` though listed acceptance boxes are checked; code-dependency freshness remains linked to #594 facet wiring. Verify the source implementation and decide whether status should become archived history. |
| F0528 | IN PROGRESS | `spec/followups/token-primitive-agent-export.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Status `implementation-in-progress` while export, delta, recipient resolver and budget checks are marked complete. Verify remote/primitive-agent integration or record that contract-only export is the accepted scope before archiving. |
| F0529 | IN PROGRESS | `spec/followups/token-structured-code-actions.md` | Review | Classify proposal as implemented/obsolete/open; consolidate accepted invariants into canonical spec, then archive or remove. | #731 + spec reconciliation | Status `implemented`; semantic source read/edit, stale-revision rejection and workspace batch receipt are marked complete. Candidate ARCHIVE after checking unchanged public contract and the noted open artifact payload retention. |
| F0530 | TODO | `spec/frontend-services.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending |
| F0531 | IN PROGRESS | `spec/incremental-cargo-nix-artifacts.md` | Review (new) | Check whether experimental Crane benchmarks belong to #758; keep only maintained design guidance. | #758 review | `spec/incremental-cargo-nix-artifacts.md` has `status: partial`, references prior #749/#754 and explicitly separates source closure from compiled artifact reuse. Keep as performance research until referenced PR/test claims are refreshed; don't misclassify as completed CI capability. |
| F0532 | TODO | `spec/interactive-ui.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending |
| F0533 | IN PROGRESS | `spec/kernel-hooks.md` | Refactor | Keep retirement/migration invariants until legacy hooks are deleted, then consolidate with Layer/Event specs. | #731 | #767 commit `68aba30` replaces obsolete legacy dispatcher/migration narrative with canonical Service Layer and Event/Listener ownership and records the dispatcher as deleted. Verify the live Core tests and exact-head source spec lifecycle check before PROVEN. |
| F0534 | TODO | `spec/kernel-runtime-rfc.md` | Keep (new) | Retain canonical kernel semantics; verify references point here instead of reproducing rules. | #736 merged | pending |
| F0535 | TODO | `spec/language-intelligence.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending |
| F0536 | TODO | `spec/language-service.md` | Keep | Retain normative specification until checked against implementation; consolidate stale/contradictory clauses after migration. | #731 + spec lifecycle audit | pending |
| F0537 | IN PROGRESS | `spec/lifecycle-hooks.md` | Retire | Deleted redundant migration-only spec after hook crate removal; preserve distinct contract rules in kernel-hooks/plugin-events. | #767 + S5-02 | DELETED in #767 `bbfbe50`, stacked on #766. The guide only repeated Layer versus Event lowering already specified by `spec/kernel-hooks.md`. Main baseline classification was provisional; keep row until deletion is merged and docs/link/CI checks pass. |
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
| F0556 | IN PROGRESS | `spec/plugin-events.md` | Keep | Canonical event semantics and listener ownership. | #731 + spec lifecycle audit | #767 `9a7079a` removes deleted `phenix-plugin-hooks/src/ownership_regression.rs` from coverage and removes false compatibility claim. Existing Core `events.rs`, delivery/provenance, and Layer regression coverage remain. Current-head source architecture and event tests pending. |
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
