# Phenix product-aligned folder and crate structure

> **Status:** target contribution guidance, not a completed relocation.  
> **Inspected code:** `main` at `dbfe3476f50e7e84cfc2defd1d7bfc9c16cfcfa8`, 2026-10-30.  
> **Scope:** crate organization, product selection, shared contracts and test isolation; graph semantics remain governed by the current accepted specifications and PRs.

## Boundary rules

- **Product = selected plugins + configuration + typed wiring + authority grants.** Kernel is implicit. Product compositions are thin declarations, not application orchestrators or a different kernel.
- **Contracts always shared.** Keep canonical `phenix-contract` and a small number of reusable contract-set crates, flat under `crates/`: `phenix-basic-contracts`, `phenix-advanced-contracts`, `phenix-provider-contracts`. Put several domain interfaces in each crate as modules. Existing environment/workspace contract sets remain until a dependency-based consolidation is justified. **No extra `contracts/` directory or crate per interface.**
- **Implementations organized by domain.** Prefer modules in product-focused crates, not a crate for each handler, strategy, type, decorator, state field or graph node. Separate crates remain justified by distinct state ownership/lifecycle, restricted dependencies, standalone deployability, plugin-loading identity or test isolation.
- **Frontends are adapters/observers.** They attach at public typed input/observation boundaries. Core agent/model/tool/session machinery cannot call or know ACP/JSONL/terminal/Neovim implementations.
- **Keep fixtures outside shipping selections.** Test compositions replace implementations, not shared interfaces. Keep graph-v2 migration staged until exact-head parity.

## Desired tree

```text
crates/                               # flat foundational + shared contracts
  phenix-kernel/
  phenix-contract/
  phenix-sdk/
  phenix-macros/
  phenix-artifact/
  phenix-basic-contracts/             # src/{agent,model,tool,session,journal,storage,...}.rs
  phenix-advanced-contracts/          # src/{compaction,evidence,preparation,project,...}.rs
  phenix-provider-contracts/
  phenix-environment-contracts/       # keep if cross-product use justifies
  phenix-workspace-contracts/         # keep if cross-product use justifies

plugins/
  shared/                             # model, provider, protocols, credentials,
                                      # storage and environment implementations
  basic/                              # product-focused agent/session/tools/
                                      # memory/journal modules and frontend adapters
  advanced/                           # model preparation, compaction, evidence,
                                      # project/workspace modules
products/                             # thin plugin selections and grants:
  basic.nix                            # proposed
  basic-offline.nix                    # proposed
  advanced.nix                         # proposed
testing/                              # test-only crates and declarations:
  testkit/                             # reusable assertion/conformance machinery
  fixtures/                            # scripted/history models, loopback provider,
                                      # mock tool/storage
  products/                            # selected fixtures, negative/positive tests
tests/                                # existing integration tests until migrated
examples/                             # SDK examples, not shipping plugins
```

The `plugins/`, `products/`, and `testing/` directories are **created as documented destinations** in this PR. The listed child Cargo crates and `*.nix` declarations are **not created** here. Existing shipping code, Cargo members, Nix file sets and products remain in place. Do not add empty crates or fake compatibility shims for a diagram.

## Concrete migration mapping

| Current `crates/*` group | Destination / boundary |
| --- | --- |
| `phenix-kernel`, `phenix-contract`, `phenix-sdk`, `phenix-macros`, `phenix-artifact` | Stay foundational, flat `crates/`. |
| `phenix-basic-contracts`, `phenix-advanced-contracts`, `phenix-provider-contracts`, environment/workspace contracts | Keep shared and flat; contract modules grouped by domain and dependency scope. |
| `phenix-plugin-basic-agent`, `phenix-plugin-basic-tools`, `phenix-plugin-basic-memory`, `phenix-plugin-event-journal` | `plugins/basic/`: combine into a product-focused crate only when public reactive port boundaries support it. |
| `phenix-plugin-basic-session`, `phenix-basic-runtime` | `plugins/basic/`: preserve session lifecycle ownership and product-host/domain separation. Do not merge automatically. |
| `phenix-plugin-basic-frontends` | `plugins/basic/`: ingress adapters and detached, authorized observers; never called explicitly by agent machinery. |
| `phenix-plugin-model-http`, `phenix-plugin-storage-sqlite`, `phenix-plugin-environment` | `plugins/shared/`: distinct replaceable model, storage, environment implementations. |
| `phenix-provider-*`, `phenix-plugin-provider-*` | `plugins/shared/`: shared protocols, auth and discovery strategies; provider-specific registration data and only genuinely exceptional logic. |
| `phenix-plugin-advanced-*`, `phenix-plugin-compaction-checkpoints`, `phenix-plugin-evidence-*`, `phenix-plugin-project-storage-sqlite`, `phenix-plugin-workspace-selection`, `phenix-p2-*` | `plugins/advanced/` or `testing/products/` according to whether the component actually ships; keep evidence/project/compaction state owners explicit. |
| `phenix-plugin-model-scripted`, `phenix-plugin-model-history-test`, `phenix-plugin-model-meter-fixture`, `phenix-plugin-provider-loopback-fixture`, `phenix-plugin-tool-fixture`, `phenix-plugin-storage-fixture` | `testing/fixtures/`; explicitly test-only. |
| `phenix-conformance`, `phenix-http-selected-product`, P2 `*-product` harnesses, root `tests/` | `testing/testkit/`, `testing/products/`, or existing integration tests by ownership. |
 
## Product selection, not duplication

| Selection | Selected domains | What must remain shared |
| --- | --- | --- |
| **Basic** | Agent, Session, Tools, Memory, Journal, selected Model/Provider/Storage, frontend ingress/observations | Basic/Provider contracts, kernel scheduling/authority |
| **Basic offline** | Basic with local/scripted implementation selected instead of network provider | Same interface contracts; no `is_test` branches in domain code |
| **Advanced** | Basic + preparation, model adapters, compaction, evidence and project/workspace capabilities | Basic state owner and contracts; additional Advanced contract set |
| **Test products** | Controlled subsets of implementations and fixtures with explicit grants | Same public contracts and failure semantics |

This is a proposed selection matrix, **not** a claim that corresponding Nix files or complete graph-v2 capabilities are already integrated.

## Migration procedure / acceptance

1. First inventory plugin identities, public interfaces, current Cargo dependencies, Nix source selections, generated CI and current product evidence. Paths must not silently become plugin identities.
2. Consolidate contract *interfaces* by domain in existing contract sets; do not introduce a second ABI schema or let contracts depend on implementations.
3. Establish public typed graph/subgraph exports, observer subscriptions and authorized extension boundaries in the owning implementation PR before moving any consumers relying on private graph node IDs. No second scheduler or event bus.
4. Migrate one cohesive domain group at a time; retain independently owned durable state and tested receipt/recovery behavior. Remove old paths once parity passes, with no in-repo compatibility layer.
5. Update Cargo workspace membership, path dependencies and lockfile, Nix source filters, product selection, CI jobs and fixture references **together** with physical moves.
6. Verify exact-head Basic user-input → model → tool → result, auth, restart/recovery, ACP/JSONL/terminal, cancellation/observer disconnect, denial/error cases and the selected offline/advanced/test compositions. Run required flake gates. Record verified and unverified work separately.

For plugin integration rules, read [plugin do's and don'ts](plugin-dos-and-donts.md). For PR reporting and spec drift, read [CONTRIBUTING.md](../CONTRIBUTING.md).
