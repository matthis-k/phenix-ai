# Phenix AI

This repository owns the generic Phenix kernel, runtime process, internal client wire, independently packaged first-party plugins and protocol adapters, native client bindings, and the supported Harness product.

The canonical Neovim AI client lives in `matthis-k/phenix-ai.nvim`. The complete Neovim distribution lives in `matthis-k/phenix-nvim` and consumes that client. This repository owns frontend-neutral runtime behavior and contracts.

## Architecture

See [glossary.md](glossary.md) for the canonical architecture vocabulary.

```text
frontends / protocol adapters
            |
            v
      phenix-harness
      product policy
            |
            v
     phenix-runtime
 generic server process
            |
            v
       phenix-core
 generic runtime mechanisms
            |
            v
  selected plugin providers
```

`phenix-core` owns plugin identity and lifecycle, deterministic service resolution, authority attenuation, generic persistence, events, tasks, the embedded runtime bootstrap, plugin runtime adapter integration, and resource-only plugins. It does not own session, context, execution, planning, tool, model, frontend, or other first-party agent semantics.

`phenix-runtime` owns the generic server process and client transport. It hosts only configured plugins. A zero-plugin runtime has no first-party fallback behavior.

First-party `phenix-plugin-*` and `phenix-adapter-*` crates own independently selectable runtime behavior through the same core contracts available to alternate providers. A thin `phenix-plugin-catalog` collects embedded factories but owns no durable state or product policy.

`phenix-harness` owns the supported product assembly. It selects plugins, grants authority, chooses persistence, loads product configuration, and exposes the wrapped `phenix` product. Skill providers own their skill packages.

`phenix-client` owns the internal runtime client/server wire; it is not a public Client SDK. `phenix-adapter-acp` is the transport-independent ACP runtime plugin. It maps standard ACP and descriptor-backed `_phenix/...` extensions to the fixed application interface.

`phenix-application-interface` owns the fixed, versioned application descriptor. Typed Rust declarations derive its `PhenixSchema` payloads. The descriptor covers editor operations, updates, callbacks, capability dependencies, and errors. It contains no runtime service topology or authority policy. Generated client bindings consume this descriptor rather than duplicating application schemas.

`phenix-acp-stdio` provides the ACP stdio server and its channel boundary. The packaged `phenix` executable selects that transport with `--mode acp`. ACP and JSONL therefore serve the same resolved product graph instead of separate product assemblies.

### Rust boundaries

| Crate or package | Responsibility |
| --- | --- |
| `phenix-core` | Generic plugin host, trust boundaries, persistence enforcement, events, tasks |
| `phenix-client` | Internal runtime client/server wire |
| `phenix-application-interface` | Passive application contracts, descriptor emission, and client generation |
| `phenix-runtime` | Generic configured server and transport |
| `phenix-plugin-*` | Independently owned first-party services |
| `phenix-adapter-acp` | Stateless ACP adapter runtime plugin |
| `phenix-acp-stdio` | ACP stdio server and configured-runtime hand-off |
| `phenix-plugin-catalog` | Thin embedded-factory catalog |
| `phenix-harness` | Supported runtime + selected-plugin product assembly |
| `phenix-model-adapter-*` | Model execution adapters |

## Product composition

`phenix-full` is the supported full Harness composition and is also exposed as `phenix`. `phenix-basic` uses the smaller basic product composition. Both packages use the same executable and select the frontend with `--mode`; the mode does not change the plugin graph.

Basic includes the workspace and local-environment providers used by its public shell and workspace tools. Full additionally selects the Advanced agent, memory and planning providers. Both products select the same declarative agent topology and Basic node providers by default; the legacy agent loop is an explicit option.

Nix exposes independently packaged first-party runtime plugins, including adapters, through `phenixPlugins.<system>.*`. `wrappers.phenix.wrap` and `lib.mkPhenix` assemble a runtime with an explicit plugin selection. Omitting a plugin removes its service unless another selected provider supplies the same contract.

The resolved component graph is the canonical runtime composition for component imports and event listeners. A `ComponentExport` identifies the executable endpoint. It does not need a duplicate terminal `ServiceContribution`. Plugin service contributions remain available for ordinary service dispatch and explicit interposition layers. Embedded and bridged runtimes execute the same graph-selected component identity. Development reconciliation replaces kernel configuration, component graph, listener bindings, resources, and generation as one resolved runtime topology.

`GraphReconciler::manage` is the sole desired-state plugin load, build, replace, unload, and reconcile path. Runtime load requests carry either a concrete content-addressed artifact or a typed build plan; trusted host policy supplies management authorization, CAS access, and an isolated build executor out of band. Core computes canonical `sha256:<64 lowercase hex>` artifact revisions from exact build output before runtime resolution, and only concrete artifacts can enter a resolved graph or `KernelConfig`.

Plugin-owned durable state is canonical. Core enforces namespace ownership, migrations, transactions, and authority without interpreting first-party domain rows. Process-local handles, connections, caches, and provider generations are disposable and must not become durable identity.

### Product configuration and skills

Supported runtime configuration lives in `config/phenix/runtime.nix`. Static product skills live with the plugin that provides them. The default set is owned by `phenix-plugin-basic-skills`.

The Harness loads agent definitions, orchestration definitions, and routing profiles through plugin-owned services. Skill activation resolves through `phenix.skills@1` and enters model context through `phenix.context@1`. Product configuration does not become hidden runtime policy.

Packaged definitions and their ownership records commit atomically across the execution and model plugins. Routing ownership is explicit: packaged configuration and provider catalogs reject any profile ID already owned outside their manifest, even when the profile value matches. Later applications compare against the last owned values, update packaged definitions, and retire removed entries from catalogs. Retired entries remain addressable by ID for durable sessions and orchestration references; a session's current retired route remains in its own selection list. A retained ID uses its latest packaged definition. Model-feature publication follows the durable commit and is replayed on startup if interrupted. Provider plugins own credential state directly; routing does not mirror authentication state.

Selection metadata carries the default provider as a typed field across the application and ACP boundaries. Display descriptions do not determine routing behavior.

Project context and skills are context-plugin resources. Their metadata never expands execution authority. Script or workspace access still uses ordinary service authority.

## Packages

The flake exposes, among other public outputs:

- `packages.<system>.phenix-core`;
- `packages.<system>.phenix-client`;
- `packages.<system>.phenix-application-interface`, including `bin/phenix-application-descriptor` and `share/phenix/interfaces/phenix.application@1.json`;
- `packages.<system>.phenix-runtime`;
- `packages.<system>.phenix-harness`;
- `packages.<system>.phenix-basic`;
- `packages.<system>.phenix-full`;
- `packages.<system>.phenix`, an alias of the full product;
- `packages.<system>.phenix-binding-lua` for Lua clients;
- `phenixPlugins.<system>.*`;
- `wrappers.phenix.wrap`;
- `lib.mkPhenixPlugin`;
- `lib.mkPhenix`.

Neovim-specific packaging is intentionally absent. `phenix-ai.nvim` composes the generic Lua binding and launches the selected Phenix product with `--mode acp` when it uses ACP stdio.

## Protocol and provider boundaries

The runtime wire remains internal. Protocol adapters translate external protocols to configured runtime services without owning durable application state.

Model adapters translate execution requests into provider protocols. Provider conversation state is disposable. Durable Phenix state stays with the owning plugins.

Provider plugins own authentication and credential state. Model routing owns provider selection. The Harness application layer projects both through the product API. None of these concerns become privileged core APIs or ambient process authority.

## Design rules

- Keep one canonical typed API per semantic operation.
- Keep one durable owner per semantic domain.
- Parse external data at boundaries and keep invalid runtime states difficult to represent internally.
- Preserve typed failure modes across configuration, transport, protocol, plugin, and provider boundaries.
- Effective authority is the intersection of caller authority, provider maximum authority, and invocation restrictions.
- Apply the same authority attenuation to direct calls, plugin calls, retries, events, tasks, persistence, workspace operations, and bridged plugin calls.
- Resource-only plugins cannot execute code.
- Zero-plugin mode has no hidden first-party fallbacks.
- First-party plugins use the same contracts as alternate plugins.
- Do not add parallel frontend-to-agent protocols or duplicate orchestration registries.
- Keep frontend-specific behavior and packaging in frontend repositories.
- Tests should assert behavior, protocol semantics, or cross-boundary integration rather than duplicated configuration facts.

## Development

For a fresh clone with repository hooks enabled immediately:

```sh
git clone -c core.hooksPath=.githooks https://github.com/matthis-k/phenix-ai
cd phenix-ai
```

The tracked hook invokes the repository's Nix maintenance app. It does not install hook files or configuration beneath `.git`. A normal clone also works; `nix develop` activates the same hook path for that repository and shell without mutating Git metadata.

```sh
nix develop
maintenance fix
maintenance all
```

Validation is separated into source, Rust, integration/system, realized product, Nix composition, and Maintenance boundaries. Product validation exercises installed runtime and Harness compositions. Frontend behavior is tested in frontend repositories.

See `DEVELOPMENT.md` for focused validation commands and test-boundary guidance.
