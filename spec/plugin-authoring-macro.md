# Rust plugin authoring

status: implemented
coverage:
  - rust/crates/phenix-sdk/tests/plugin_attribute_only_gate.rs
  - rust/crates/phenix-sdk/tests/plugin_component_authoring.rs
  - rust/crates/phenix-sdk/tests/plugin_attribute_graph.rs
  - rust/crates/phenix-sdk/src/authoring/static_dispatch.rs

## Purpose

Define the current Rust-native authoring surface for Phenix runtime plugins.

Plugin authors write ordinary Rust state and behavior plus semantic annotations. Generated SDK plumbing lowers those declarations into the canonical Core contribution and runtime interfaces. Authors do not maintain a second manifest tree, factory registry, dispatch ladder, listener registry, resource-registration list, or hook runtime.

The central rule is:

> Plugins provide capabilities. The kernel composes capabilities into the runtime.

## Ownership model

A Plugin is the independently activatable identity, lifecycle, authority, hosting, and durable-ownership boundary.

A Component is a Plugin-owned runtime composition unit. Components import and export typed Interfaces and may provide Layers, Listeners, and public values.

An Interface is a stable semantic contract. Provider and consumer Rust types may differ when their structural schemas are compatible.

A concrete Plugin dependency selects another Plugin implementation. An Interface import requests a capability and is resolved independently of the provider implementation.

## Authoring surface

The canonical Rust authoring surface is attribute-driven:

```text
#[phenix_sdk::plugin]
#[phenix_sdk::component]
#[phenix_sdk::interface(...)]
#[phenix_sdk::resource(...)]
```

Plugin fields declare owned relationships such as:

```text
#[phenix(dep)]
#[phenix(component)]
#[phenix(resource)]
#[phenix(config)]
```

Component and Plugin methods declare behavior such as:

```text
#[phenix(export(...))]
#[phenix(layer(...))]
#[phenix(listen(...))]
#[phenix(value(...))]
#[phenix(start)]
#[phenix(stop)]
```

Annotations carry semantic data that cannot be inferred safely, including public visibility, terminal participation, authority, layer priority, stable IDs, and event identity.

Simple module plugins and stateful struct plugins lower into the same Core model.

## Generated lowering

Generated authoring code derives the Core-facing representation from the annotated Rust definition:

- Plugin identity and maximum authority;
- concrete Plugin dependencies;
- Components and their stable ownership;
- typed Imports and Exports;
- terminal service participation and Layers;
- Events and Listeners;
- public callables and values;
- Plugin Resources and configuration metadata;
- lifecycle callbacks;
- runtime dispatch adapters.

The generated representation is data for the canonical resolver. It does not register itself into a live runtime.

`rust/crates/phenix-sdk/tests/plugin_attribute_only_gate.rs` is the adoption gate: an attribute-only Plugin must build its graph and manifests and activate generated runtime behavior without parallel wiring.

## Typed boundaries

`PhenixValue` is the canonical dynamic boundary representation. Once a semantic target is known, the receiving side parses into its own invariant-bearing Rust type.

Structural matching uses the shared SDK wrappers:

```text
T           projected matching by default
Project<T>  explicit projected matching
Exact<T>    exact matching
```

Do not introduce parallel exact-call APIs or provider implementation dependencies merely to share request and response types. See `typed-structural-boundaries.md`.

## Runtime semantics

Authoring syntax does not own runtime topology.

The kernel:

- resolves concrete dependency closure and Interface providers;
- validates structural compatibility and authority;
- creates one immutable Graph Generation;
- owns lifecycle, dispatch, Events, Layers, cancellation, persistence coordination, and reconciliation;
- rebuilds and commits runtime topology when composition changes.

Hooks are authoring shorthand over canonical Layer or Event/Listener mechanisms, not a second execution system.

Async Rust methods may be adapted behind the synchronous canonical Plugin API. Executor-specific types do not become Core ABI. See `plugin-threading.md`.

## Identity

Stable externally visible identity is explicit. Plugin-owned nested identities may be derived deterministically when renaming them cannot break an external contract.

Identity precedence is:

1. an explicit stable ID;
2. a type-provided canonical ID;
3. deterministic parent plus item-name derivation for Plugin-owned local identities.

Cross-Plugin Interface identity never derives from a provider implementation's local field or method name.

## Invariants

- One Rust authoring model lowers into one canonical Plugin model.
- Static Plugin authors do not maintain parallel runtime wiring.
- Plugin dependencies and Interface imports remain distinct.
- Provider replacement does not require consumer source changes.
- Runtime topology belongs to the resolver and Graph Generation, not to authoring macros.
- Layers provide synchronous interposition; Events describe facts that already occurred.
- Plugin Resources and lifecycle callbacks are generated from declarations.
- Dynamic values cross Phenix boundaries as `PhenixValue`, then become local typed values.
- Runtime and executor choices do not change Plugin semantics.

## Related contracts

- `plugin-contributions.md` owns the Core contribution vocabulary.
- `plugin-resolution.md` owns provider resolution.
- `plugin-host.md` owns executable Plugin access to kernel capabilities.
- `plugin-events.md` and `plugin-service-layering.md` own Event and Layer semantics.
- `plugin-persistence.md` owns durable schema and Store behavior.
- `plugin-runtime-bridges.md` owns plugin-runtime-adapter and live Plugin-management semantics.
