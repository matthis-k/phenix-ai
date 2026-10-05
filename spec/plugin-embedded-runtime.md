# Embedded Rust plugin runtime

status: implemented

## Purpose

Run trusted Rust plugins inside the normal Phenix process without IPC or a dynamic Rust ABI.

Requires `spec/plugin-host.md` and `spec/plugin-threading.md`.

## Hosting model

Embedded executable plugins are Rust crates linked into the assembled Phenix product at build time.

The product links plugin factories, not one global mutable plugin instance:

```text
statically linked PluginFactory
  -> configuration activates plugin
  -> PluginRuntime generation
  -> blocking calls on host-managed worker threads
```

The kernel crate does not depend on concrete plugin crates. Product assembly depends on the kernel and selected embedded plugin crates.

First-party status and runtime choice are independent. A trusted custom distribution may embed an additional plugin. A first-party plugin may instead use a plugin-runtime-adapter bridge when it needs independent distribution or enforceable OS isolation.

## Rust boundary

The embedded adapter may use ordinary synchronous Rust traits and concrete Rust values internally.

A representative shape is:

```rust
pub trait PluginFactory: Send + Sync + 'static {
    fn manifest(&self) -> &PluginManifest;

    fn instantiate(
        &self,
        activation: PluginActivation,
    ) -> Result<Box<dyn PluginRuntime>, PluginError>;
}
```

The exact trait may change during implementation. The required semantics are:

- the product owns a deterministic catalog of linked factories;
- configuration selects an available factory by canonical `PluginId`;
- activation creates a new runtime generation bound to the pinned configuration;
- runtime calls receive only permission-bound kernel services through `PluginHostHandle`;
- executable plugin work runs through the blocking worker model from `spec/plugin-threading.md`.

The kernel never passes `&mut ConductorRuntime`, raw registries, store connections, or other mutable kernel internals to plugin code.

## Availability and activation

Embedding and activation are separate.

- **Available** means the factory exists in the built product.
- **Enabled** means the pinned Harness configuration activates that plugin.

Disabling an embedded plugin removes its contributions from new compiled configuration revisions. It does not remove code from the executable and does not require relinking.

Changing the implementation bytes of an embedded plugin changes the assembled Phenix product and therefore requires a rebuild.

Configuration may activate the same linked implementation under different immutable configuration revisions. Each activation gets its own runtime generation and settings/grants derived from that revision.

## Registration

Factory manifest inspection and contribution validation are side-effect free.

Configuration compilation may inspect embedded manifests, schemas, and contribution declarations. It must not start network requests, filesystem watchers, provider sessions, subprocesses, or other live behavior.

Live runtime activation happens only after the complete configuration revision validates.

## Calls

Embedded invocation follows the same kernel path as bridged invocation:

```text
resolve provider
  -> establish effective permission
  -> create live call scope
  -> dispatch embedded runtime on worker thread
  -> plugin uses PluginHostHandle as needed
  -> normalize events/result/error
  -> attach provenance
  -> close live call scope
```

No serialization or IPC is required for embedded calls.

A blocking embedded call must not hold broad kernel mutable-state locks or persistence transactions while waiting on external I/O.

## Trust boundary

Embedded native code is trusted from an OS-isolation perspective.

Kernel permission checks still define Phenix semantics and must run for host operations, provider resolution, durable namespaces, and canonical state mutation. They do not sandbox arbitrary native code in the same address space.

A plugin that requires enforceable filesystem, network, secret, IPC, crash, or memory isolation uses a suitable plugin-runtime-adapter bridge, such as a process-backed bridge.

## No dynamic Rust ABI

Phenix does not load Rust plugins through `.so`, `.dylib`, or `.dll` libraries using the Rust ABI.

Embedded Rust plugins are linked at build time. Independently distributed executable plugins use a plugin-runtime-adapter bridge with a versioned guest/runtime boundary.

A stable C ABI or another in-process binary plugin ABI is out of scope until a concrete requirement justifies its ownership, unsafe boundary, versioning, and lifecycle cost.

## Resource-only plugins

A plugin that contributes only static skills, schemas, templates, context, or other data does not need an executable runtime.

Resource packages register through their manifest and configured store path. They do not create a fake embedded factory or subprocess.

## Invariants

- Embedded Rust plugins are statically linked into the assembled product.
- The kernel crate never depends on concrete plugin crates.
- The product owns the deterministic embedded factory catalog.
- Embedding does not imply enablement or authority.
- Configuration activation creates runtime generations; linked code is not hot-loaded or unloaded.
- Embedded calls use direct synchronous Rust calls through `PluginHost` and host-managed worker threads.
- Embedded plugin code never receives mutable kernel internals.
- Manifest inspection is side-effect free.
- Rust dynamic libraries are not a supported plugin format.
- Plugins needing enforceable isolation use a suitable plugin-runtime-adapter bridge.

## Required regressions

- the normal Harness links the configured first-party embedded factories;
- the kernel package builds without depending on concrete Phenix Plugin Suite crates;
- disabling an embedded plugin removes its contributions without rebuilding the executable;
- two configuration revisions can activate the same embedded factory with different settings and distinct runtime generations;
- manifest inspection performs no live plugin side effects;
- embedded invocation passes through provider resolution, permission enforcement, live-call tracking, and provenance;
- embedded plugin code cannot obtain a mutable kernel/store/registry handle through the plugin API;
- an embedded blocking provider runs on a worker thread and does not stop unrelated kernel progress;
- no Rust dynamic-library loader is required by the normal Harness.

## PR boundary

This slice defines the embedded factory catalog, activation semantics, direct adapter, and build-time dependency direction. Other executable runtimes are supplied through the plugin-runtime-adapter model in `spec/plugin-runtime-bridges.md`; package and wrapper composition belong to `spec/plugin-nix-packaging.md`.
