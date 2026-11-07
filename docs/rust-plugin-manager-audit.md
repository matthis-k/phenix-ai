# Actual Phenix plugin requirements — implementation inventory

**Inspected:** `matthis-k/phenix-ai` main at `28cf5cd` and draft PR #1242
(`phenix-plugin` extraction), plus deployed plugin crate sources, SDK/macros,
artifact verification, resolver, kernel runtime and historical plugin audit.
This is a source audit, **not** proof that every plugin ships in every product.

## Existing machinery and real consumers

| Mechanism | Source / actual use | Decision |
|---|---|---|
| Rust `.so` (`cdylib`) plugins, versioned native entrypoint, create/init/invoke/poll/cancel/stop | `phenix-macros/src/state.rs`, `phenix-contract/src/abi.rs`, native loader, `phenix-kernel/src/{runtime,pending}.rs`; Basic/model/provider plugins | Retain behind a private driver/adapter; do not duplicate in a plugin manager. |
| Artifact + schema verification **before** `dlopen` | `phenix-artifact`, `resolve::Prepared`, `Native::load` | Preserve trusted host selection, digest and provenance. |
| Plugin-owned Rust `State`, init/stop hooks | SDK `#[phenix_sdk::plugin]`, Basic Agent/Session/Memory, providers | Default to private per-instance state, not global host cells. |
| `define`/`provide`/`require` typed operations, One/Optional/Many imports | SDK macro, `phenix-contract` metadata, `resolve.rs` | Bind contributions at composition; provider selection is not a graph runtime primitive. |
| `Value`, `Portable`, `ValueSchema`, canonical ABI bytes | `phenix-contract` and typed `phenix-sdk::live` | Keep wire representation separate from ordinary Rust types. |
| Scoped authority/resources and authenticated host callbacks | `phenix-kernel/src/{runtime,state_effects}.rs` | Kernel issues/attenuates; registration never grants capability. |
| Pending invocation/cancellation/wakeup | `phenix-kernel/src/pending.rs`, model HTTP | Node implementation may be pending; one execution mechanism in graph. |
| Optional declared graph metadata and v2 declarations | SDK graph macro and offline compiler; graph-v2 prototype separate | Future graph bridge consumes one `GraphPatch` contribution, compiles with existing planner. |
| Selected generation identity, promotion and retirement | `phenix-kernel/src/generation.rs` | Retain completed code and policy via leases across activation. |
| Model/provider catalogs, authentication, storage, sessions, frontends, tools | 28 `phenix-plugin-*` crates in Oct 24 audit; current tree includes more fixture and adapters | These are **domain plugins**, not generic plugin-manager responsibilities. |

## Gaps requiring the new plugin primitive

1. No simple host-side generic typed `load`/`unload`/`reload` for independently
   configured plugin instances contributing arbitrary types.
2. No sealed compile-time-typed configuration key across crate consumers.
3. No contribution handle whose owner/code lifetime remains pinned after
   a plugin is logically unloaded or a generation retires.
4. No clear distinction between *catalog* (reusable factories), *plugin set*
   (kernel instance) and *generation* (selected immutable graph/resources).
5. Existing SDK `#[plugin]` is tailored to M1 operation contracts and ABI;
   there is not yet a graph-independent registration API for arbitrary Rust
   services, factories, modules, graph patches and resources.
6. Instance-level reload must not reuse prior plugin private state implicitly;
   persistent state needs explicit resources/migration.

## Prototype constraints / deliberate omissions

- Only Rust plugins: local typed Rust API. No Lua/JS/Python plugin protocols.
  An ordinary Rust plugin may internally run Lua or another runtime.
- The first prototype does **not** load a new `.so` and obtain typed Rust
  contributions from it. Dynamic Rust binaries require a small stable ABI
  shim plus a host-side typed adapter. Reusing existing M1 ABI is preferable.
- No universal plugin supertrait or global service locator.
- No plugin-specific graph node kinds, second scheduler, or duplicative patch
  resolution. Graph contributions are ordinary registered Rust types.
- No filesystem scanning, dependency solver, distributed placement, hot binary
  swapping, automatic state migration or domain configuration codecs in V0.
- `reload` is **staged registration**, not atomic graph generation activation:
  the kernel must compile/validate the resulting graph before promoting the
  generation. A later kernel composition adapter owns that final transaction.

## Type/boundary strategy

- **Factory edge:** `PluginKey<Config>` and `Fn(Config)->Result<Instance>`;
  configuration mismatch cannot compile at ordinary Rust call sites.
- **Erased interior:** catalog maps string identities to private erased
  constructors, no generics on the shared runtime.
- **Contribution edge:** `Contribution` trait and `ContributionHandle<C>`;
  a handle retains its exact plugin revision via `Arc<Loaded>`.
- **Kernel boundary:** host constructs `Registrar` using selected identity.
  Registered identifiers have zero authority until kernel grants and graph
  compiler validate them.
- **Native boundary:** ABI-safe opaque buffers/handles; never let raw Rust
  trait objects escape across independently compiled shared-library ABI.

## Safety-critical tests

- Two plugin sets from one catalog have isolated plugin instance state.
- Distinct contribution kinds share registration without graph dependencies.
- Duplicate factory/contribution names reject deterministically.
- Typed config and typed retrieval; foreign keys fail closed.
- Construction/registration failures leave old revision unchanged.
- Unload removes availability without dropping pinned contribution code/state.
- No old generation can accidentally observe newly registered actions.

## Concrete next slice

1. Compile/test this first Rust-only catalog/set prototype.
2. Bridge a `GraphPatch` contribution to the existing graph prototype's
   `PatchPlanner`, with contributor provenance and authority validated by kernel.
3. Reuse verified native packages + ABI 3 loader to expose host-owned graph
   action handles with explicit library leases (no Rust trait-object ABI).
4. Build/test atomic kernel generation composition; only then replace the
   current M1 invocation/graph paths and remove obsolete glue.