# Owner PR task matrices

This file is an **index**, not a second implementation checklist. Each PR's linked GitHub conversation comment owns its checkboxes, evidence and current status. Edit that comment in place rather than appending contradictory progress comments or copying statuses here. The [central stage matrix](../microkernel-migration-tracker.md) records only the cross-PR gates, and the [main-tree](repository-file-matrix.md) and [#726](pr726-file-matrix.md) file matrices record file-level decisions.

## Existing owner task matrices

| Owner | Count | Area | Dependencies | Editable matrix |
| --- | ---: | --- | --- | --- |
| [#726](https://github.com/matthis-k/phenix-ai/pull/726) | 12 | Kernel workflow IR and executor | none for generic IR; #728 for portable entries | [Task matrix and proof ledger](https://github.com/matthis-k/phenix-ai/pull/726#issuecomment-6094025633) |
| [#728](https://github.com/matthis-k/phenix-ai/pull/728) | 7 | Typed contract/authoring descriptors | stable #726 selected Core interfaces | [Task matrix and proof ledger](https://github.com/matthis-k/phenix-ai/pull/728#issuecomment-6094025933) |
| [#760](https://github.com/matthis-k/phenix-ai/pull/760) | 6 | Selected contribution identity and generation | stacked on #728 | [Task matrix and proof ledger](https://github.com/matthis-k/phenix-ai/pull/760#issuecomment-6094026951) |
| [#725](https://github.com/matthis-k/phenix-ai/pull/725) | 7 | Durable prompt admission, claims and restart | stacked on #760 | [Task matrix and proof ledger](https://github.com/matthis-k/phenix-ai/pull/725#issuecomment-6094025363) |
| [#729](https://github.com/matthis-k/phenix-ai/pull/729) | 7 | IR patch compiler and conflict algebra | stable #726 IR and #728 descriptors | [Task matrix and proof ledger](https://github.com/matthis-k/phenix-ai/pull/729#issuecomment-6094026212) |
| [#730](https://github.com/matthis-k/phenix-ai/pull/730) | 7 | Plugin-defined kinds and templates | #728; #729 only for patch emitting kinds | [Task matrix and proof ledger](https://github.com/matthis-k/phenix-ai/pull/730#issuecomment-6094026435) |
| [#731](https://github.com/matthis-k/phenix-ai/pull/731) | 11 | Tool/skill migration, legacy retirement, non-agent parity | #729 and #730 | [Task matrix and proof ledger](https://github.com/matthis-k/phenix-ai/pull/731#issuecomment-6094026655) |
| [#763](https://github.com/matthis-k/phenix-ai/pull/763) | 5 | Combined full integration validation | #726 + #728 + #760 + #725 plus native/product canaries | [Task matrix and proof ledger](https://github.com/matthis-k/phenix-ai/pull/763#issuecomment-6094027226) |
| [#765](https://github.com/matthis-k/phenix-ai/pull/765) | 5 | Strict routing, MCP and macro compatibility deletion | main; independent of #726 | [Cleanup matrix](https://github.com/matthis-k/phenix-ai/pull/765#issuecomment-6094709270) |
| [#766](https://github.com/matthis-k/phenix-ai/pull/766) | 6 | Imperative agent crate retirement and product parity | stacked on #726 | [Cleanup matrix](https://github.com/matthis-k/phenix-ai/pull/766#issuecomment-6094709556) |
| [#767](https://github.com/matthis-k/phenix-ai/pull/767) | 5 | Hook dispatcher crate retirement and Event/Layer parity | stacked on #766 | [Cleanup matrix](https://github.com/matthis-k/phenix-ai/pull/767#issuecomment-6094709854) |
| [#768](https://github.com/matthis-k/phenix-ai/pull/768) | 5 | Retire application agent-service fallback; use selected pinned workflow only | stacked on #767 | [Application route matrix](https://github.com/matthis-k/phenix-ai/pull/768#issuecomment-6094811078) |
| [#769](https://github.com/matthis-k/phenix-ai/pull/769) | 6 | Standalone native ABI, intrinsic loader and ownership split | main; #726 must restack on #769 before closeout | [Native ABI owner matrix](https://github.com/matthis-k/phenix-ai/pull/769#issuecomment-6095306928) |

Total: **89 individually numbered owner tasks**. These tasks start unchecked unless the full stated acceptance proof is recorded. Existing implementation is not assumed wrong; it is not yet certified complete by this tracker.

## Planned owner PRs that must be opened before implementation

| Proposed owner | Parent gate | Exact initial files | First required evidence |
| --- | --- | --- | --- |
| Native Core adapter completion | #769 standalone ABI/loader, #726 canonical executor | `rust/crates/phenix-core/src/runtime/native_plugin.rs`; `rust/crates/phenix-core/src/plugin_pending.rs` | W016/W023 Core bridge moved to one selected invocation contract or removed after tested generic CallStart; #726 diff no longer owns native ABI/loader crate implementations |
| Native guest Lua adapter | Native loader stable | New guest adapter crate and fixtures only; separate existing `rust/crates/phenix-binding-lua/src` client bindings | Rust calls Lua guest, Lua guest calls Rust import, scoped identity and authority; editor client remains independent |
| Static catalog and profiles | #728, #730 and #731 selected consumer descriptors | `rust/crates/phenix-plugin-catalog/src/lib.rs`; `rust/crates/phenix-harness/src/{runtime_builder.rs,basic_suite.rs}`; `rust/crates/phenix-agent-configurations/src/lib.rs`; `modules/{development.nix,package-sets.nix,plugin-packaging.nix}` | Third-party provider installation works without editing product or central catalog |
| Core domain contract relocation | Service-only SDK and consumer compatibility | `rust/crates/phenix-core/src/agent.rs`; `rust/crates/phenix-sdk/src/contracts/`; `rust/crates/phenix-domain/src/` | Core non-agent build and external wire/schema ID compatibility |
| Step/tool scheduler consolidation | #726 scheduler and #731 semantics | `rust/crates/phenix-plugin-step-runner/src/runner.rs`; `rust/crates/phenix-plugin-execution/src/tool_schedule.rs` | Same exclusivity/cancel/limits, one graph control-flow owner |
| Echo model test-only migration | Real Basic/Full model selection | `rust/crates/phenix-plugin-basic-model/{Cargo.toml,src/lib.rs}`; `rust/crates/phenix-harness/src/basic_suite.rs`; package defaults | Deterministic fixtures retained, production profile has no unrequested echo provider |
| Final repository cleanup | All owner replacement/parity gates | Every remaining `Refactor/Move/Retire/Review` row in [main-tree checklist](repository-file-matrix.md), plus changed branch-only paths; `rust/Cargo.toml`, `rust/Cargo.lock`, `modules/`, `.github/workflows/`, `spec/` and `scripts/` | 100% file decisions with exact PR/test proof; no stale workspace, Nix or frontend references |

These are **planned PR assignments**, not claims that such PRs already exist. When one opens, replace the label with its PR number, create an editable numbered checkbox matrix on that PR, and add its comment link above. Do not put the implementation on tracking PR #764.

## Session handoff

At the start, inspect the owner PR and its task matrix comment; verify the current head and state. Select one unblocked task. At the end, edit the same matrix comment with completed tasks and links, then update the central stage gate and individual file rows if the evidence changes them. A separate dated comment may record a concise handoff but is not the authoritative status.

Format for task evidence: `[owner task ID] -> commit <SHA> -> test or CI URL -> replaced/deleted paths -> remaining gap`. A missing proof leaves the box unchecked. If a task grows beyond its file owner, open a new scoped PR and add a dependency before continuing.
