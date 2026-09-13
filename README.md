# Phenix AI

This repository owns the generic Phenix runtime, conductor, internal client wire, independently packaged first-party plugins and protocol adapters, the canonical Neovim frontend, and the supported Harness product.

The canonical Neovim source lives under `clients/nvim`. The standalone `matthis-k/phenix-nvim` repository is a deterministic export target for plugin managers, not a second implementation.

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

`phenix-application-interface` owns the fixed, versioned application descriptor. Typed Rust declarations derive its `PhenixSchema` payloads. The descriptor covers editor operations, updates, callbacks, capability dependencies, and errors. It contains no runtime service topology or authority policy. The first Rust emitter consumes the serialized descriptor and produces a compiled regression client.

`phenix-binding-lua` projects the application/value-capability client surface into Lua. `clients/nvim` consumes that binding through `require("phenix")`; only its runtime adapter owns native client userdata.

`phenix-acp-stdio` provides the ACP stdio server and its channel boundary. The remaining configured-runtime bridge and `phenix-acp` executable are required before a packaged editor can spawn the real Phenix application.

### Rust boundaries

| Crate or package | Responsibility |
| --- | --- |
| `phenix-core` | Generic plugin host, trust boundaries, persistence enforcement, events, tasks |
| `phenix-client` | Internal conductor client/server wire |
| `phenix-application-interface` | Passive application contracts, descriptor emission, and Rust generation |
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

Project context and skills are context-plugin resources. Their metadata never expands execution authority. Script or workspace access still uses ordinary service authority.

## Packages

The flake exposes:

- `packages.<system>.phenix-core`;
- `packages.<system>.phenix-client`;
- `packages.<system>.phenix-application-interface`, including `bin/phenix-application-descriptor` and `share/phenix/interfaces/phenix.application@1.json`;
- `packages.<system>.phenix-binding-lua`;
- `packages.<system>.phenix-nvim`;
- `packages.<system>.phenix-nvim-export`;
- `packages.<system>.phenix-conductor`;
- `packages.<system>.phenix-harness`;
- `packages.<system>.phenix`;
- `phenixPlugins.<system>.*`;
- `wrappers.phenix.wrap`;
- `lib.mkPhenixPlugin`;
- `lib.mkPhenix`.

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
- Keep canonical Neovim behavior and packaging under `clients/nvim`; export mirrors from that source.
- Tests should assert behavior, protocol semantics, or cross-boundary integration rather than duplicated configuration facts.

## Development

For a fresh clone with repository hooks enabled immediately:

```sh
git clone -c core.hooksPath=.githooks https://github.com/matthis-k/phenix-conductor
cd phenix-conductor
```

The tracked hook invokes the repository's Nix maintenance app. It does not install hook files or configuration beneath `.git`. A normal clone also works; `nix develop` activates the same hook path for that repository and shell without mutating Git metadata.

```sh
nix develop
maintenance fix
maintenance all
```

Validation is separated into source, Rust, integration/system, realized product, Nix composition, Maintenance, and canonical frontend boundaries. Product validation exercises installed conductor and Harness compositions. `clients/nvim` has packaged headless checks in this repository; standalone mirrors validate export parity rather than owning behavior.

See `DEVELOPMENT.md` for focused validation commands and test-boundary guidance.