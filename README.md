# Phenix AI

This repository owns the generic Phenix runtime, conductor, internal client wire, independently packaged first-party plugins and protocol adapters, native client bindings, and the supported Harness product.

The canonical Neovim AI client lives in `matthis-k/phenix-ai.nvim`. The complete Neovim distribution lives in `matthis-k/phenix-nvim` and consumes that client. This repository owns frontend-neutral runtime behavior and contracts.

## Architecture

```text
frontends / protocol adapters
            |
            v
      phenix-harness
      product policy
            |
            v
     phenix-conductor
 generic server process
            |
            v
       phenix-core
 generic runtime mechanisms
            |
            v
  selected plugin providers
```

`phenix-core` owns plugin identity and lifecycle, deterministic service resolution, authority attenuation, generic persistence, events, tasks, the embedded runtime bootstrap, runtime-provider integration, and resource-only plugins. It does not own session, context, execution, planning, tool, model, frontend, or other first-party agent semantics.

`phenix-conductor` owns the generic server process and client transport. It hosts only configured plugins. A zero-plugin conductor has no first-party fallback behavior.

First-party `phenix-plugin-*` and `phenix-adapter-*` crates own independently selectable runtime behavior through the same core contracts available to alternate providers. A thin `phenix-plugin-catalog` collects embedded factories but owns no durable state or product policy.

`phenix-harness` owns the supported product assembly. It selects plugins, grants authority, chooses persistence, loads product configuration and skills, and exposes the wrapped `phenix` product.

`phenix-client` owns the internal conductor client/server wire; it is not a public Client SDK. `phenix-adapter-acp` is the transport-independent ACP runtime plugin. It maps standard ACP and descriptor-backed `_phenix/...` extensions to the fixed application interface.

`phenix-application-interface` owns the fixed, versioned application descriptor. Typed Rust declarations derive its `PhenixSchema` payloads. The descriptor covers editor operations, updates, callbacks, capability dependencies, and errors. It contains no runtime service topology or authority policy. Generated client bindings consume this descriptor rather than duplicating application schemas.

`phenix-acp-stdio` provides the ACP stdio server and its channel boundary. The packaged `phenix-acp` executable composes that transport with the supported Harness application worker so external clients can spawn a complete runtime through one stable executable boundary.

### Rust boundaries

| Crate or package | Responsibility |
| --- | --- |
| `phenix-core` | Generic plugin host, trust boundaries, persistence enforcement, events, tasks |
| `phenix-client` | Internal conductor client/server wire |
| `phenix-application-interface` | Passive application contracts, descriptor emission, and client generation |
| `phenix-conductor` | Generic configured server and transport |
| `phenix-plugin-*` | Independently owned first-party services |
| `phenix-adapter-acp` | Stateless ACP adapter runtime plugin |
| `phenix-acp-stdio` | ACP stdio server and configured-runtime hand-off |
| `phenix-plugin-catalog` | Thin embedded-factory catalog |
| `phenix-harness` | Supported conductor + selected-plugin product assembly |
| `phenix-backend-*` | Provider/backend adapters |

## Product composition

The normal `phenix` package is the supported Harness composition. It is built through the same public package interfaces available to users.

Nix exposes independently packaged first-party runtime plugins, including adapters, through `phenixPlugins.<system>.*`. `wrappers.phenix.wrap` and `lib.mkPhenix` assemble a conductor with an explicit plugin selection. Omitting a plugin removes its service unless another selected provider supplies the same contract.

The resolved component graph is the canonical runtime composition for component imports and event listeners. A `ComponentExport` identifies the executable endpoint. It does not need a duplicate terminal `ServiceContribution`. Plugin service contributions remain available for ordinary service dispatch and explicit interposition layers. Embedded and bridged runtimes execute the same graph-selected component identity. Development reconciliation replaces kernel configuration, component graph, listener bindings, resources, and generation as one resolved runtime topology.

`GraphReconciler::manage` is the sole desired-state plugin load, build, replace, unload, and reconcile path. Runtime load requests carry either a concrete content-addressed artifact or a typed build plan; trusted host policy supplies management authorization, CAS access, and an isolated build executor out of band. Core computes canonical `sha256:<64 lowercase hex>` artifact revisions from exact build output before runtime resolution, and only concrete artifacts can enter a resolved graph or `KernelConfig`.

Plugin-owned durable state is canonical. Core enforces namespace ownership, migrations, transactions, and authority without interpreting first-party domain rows. Process-local handles, connections, caches, and provider generations are disposable and must not become durable identity.

### Product configuration and skills

Supported runtime configuration lives in `config/phenix/runtime.nix`. Skills and product resources live under `config/phenix/skills/`.

The Harness packages these resources and loads agent definitions, orchestration definitions, and routing profiles through plugin-owned services. Product configuration does not become hidden conductor policy.

Packaged definitions and their ownership records commit atomically across the execution and model plugins. The first application adopts matching preexisting definitions and recognizes generated model profiles by their content-derived IDs. Conflicting foreign records are rejected before any definitions change. Later applications compare against the last owned values, update packaged definitions, and retire removed entries from catalogs. Retired entries remain addressable by ID for durable sessions and orchestration references; a session's current retired route remains in its own selection list. A retained ID uses its latest packaged definition. Authentication and capability publication follow the durable commit and are replayed on startup if interrupted.

Selection metadata carries the default provider as a typed field across the application and ACP boundaries. Display descriptions do not determine routing behavior.

Project context and skills are context-plugin resources. Their metadata never expands execution authority. Script or workspace access still uses ordinary service authority.

## Packages

The flake exposes, among other public outputs:

- `packages.<system>.phenix-core`;
- `packages.<system>.phenix-client`;
- `packages.<system>.phenix-application-interface`, including `bin/phenix-application-descriptor` and `share/phenix/interfaces/phenix.application@1.json`;
- `packages.<system>.phenix-conductor`;
- `packages.<system>.phenix-harness`;
- `packages.<system>.phenix`;
- `packages.<system>.phenix-acp` for external ACP clients;
- `packages.<system>.phenix-binding-lua` for Lua clients;
- `phenixPlugins.<system>.*`;
- `wrappers.phenix.wrap`;
- `lib.mkPhenixPlugin`;
- `lib.mkPhenix`.

Neovim-specific packaging is intentionally absent. `phenix-ai.nvim` composes the generic Lua binding and `phenix-acp` runtime with its own Neovim source.

## Protocol and provider boundaries

The conductor wire remains internal. Protocol adapters translate external protocols to configured runtime services without owning durable application state.

Backend adapters translate execution requests into provider protocols. Provider conversation state is disposable. Durable Phenix state stays with the owning plugins.

Authentication and provider selection are plugin and Harness concerns. They must not become privileged core APIs or ambient process authority.

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

Validation is separated into source, Rust, integration/system, realized product, Nix composition, and Maintenance boundaries. Product validation exercises installed conductor and Harness compositions. Frontend behavior is tested in frontend repositories.

See `DEVELOPMENT.md` for focused validation commands and test-boundary guidance.
