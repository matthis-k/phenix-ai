# Plugin contributions

status: implemented
coverage:
  - rust/crates/phenix-sdk/tests/plugin_attribute_only_gate.rs
  - rust/crates/phenix-sdk/tests/plugin_component_authoring.rs
  - rust/crates/phenix-core/src/runtime_topology_generation_regression.rs

## Purpose

Define the Core contribution data produced by Plugin authoring and runtime package loading.

Contribution descriptors are resolver input. They are not a second author-facing declaration model and they do not mutate the active runtime directly.

This document extends `plugin-authoring-macro.md`.

## Canonical contribution vocabulary

A Plugin may contribute generic kernel-owned metadata for:

- Plugin identity, version, execution runtime, and maximum authority;
- concrete Plugin dependencies;
- Components;
- typed Interface Imports and Exports;
- terminal service participation;
- Layers;
- Events and Listeners;
- Plugin Resources and durable schemas;
- configuration metadata;
- lifecycle callbacks;
- public callables and values;
- Plugin Runtime Adapter requirements.

Every contribution has stable Plugin ownership. Graph Generation provenance is assigned by resolution and activation rather than by the authoring surface.

Product meanings such as sessions, memory, models, tools, and artifacts are expressed through neutral Interface contracts and Plugin-owned data. Core contribution types do not encode those product domains.

## Static Plugin lowering

For Rust-native static Plugins, macros and SDK authoring code derive contribution descriptors from annotated structs, modules, fields, and methods.

Authors do not manually maintain `PluginManifest`, `ComponentManifest`, registration tables, factories, dispatch ladders, or Plugin Resource registration lists for declarations that the authoring surface can derive.

The generated descriptor remains inspectable before executable Plugin behavior runs.

## Components

Components are the normal composition unit for runtime behavior.

A Component contribution contains its Plugin ownership plus its Imports, Exports, service participation, Layers, Listeners, and authority requirements. Interface structural compatibility is validated during graph resolution.

An Export and terminal service participation are distinct semantics even when one annotated method generates both contributions.

## Dependencies

Concrete Plugin dependencies and Interface Imports remain separate contribution kinds.

A concrete dependency selects and recursively includes a specific Plugin definition. An Interface Import requests a capability and leaves provider selection to the resolver.

Required and optional import semantics are explicit in the contribution type. Missing required imports fail graph construction. Optional imports do not trigger hidden provider search at invocation time.

## Resources and configuration

Plugin Resources contribute stable resource identity, schema metadata, ownership, and required backend features. The kernel uses these declarations for persistence planning.

Configuration contributions describe typed Plugin-owned configuration semantics. Configuration frontends lower user syntax into canonical configuration contributions; they do not register live providers or mutate topology.

## Runtime packages

A dynamically managed Plugin candidate supplies the same semantic contribution model as a static Plugin. Execution runtime and artifact revision are packaging and execution metadata, not alternate component semantics.

Plugin Runtime Adapters translate an artifact into the canonical executable Plugin interface after the candidate's inspectable contributions have been validated.

## Resolution boundary

Contribution data is immutable resolver input for a candidate Graph Generation.

The resolver owns:

- dependency closure;
- provider binding;
- structural compatibility;
- Layer ordering;
- authority attenuation;
- plugin-runtime-adapter dependency resolution;
- resource and persistence planning;
- generation identity.

A Plugin cannot alter its already-resolved contribution semantics from inside `start`, `invoke`, a Listener, or a Layer.

## Invariants

- One canonical contribution model feeds one resolver.
- Static authoring derives descriptors instead of requiring parallel manual wiring.
- Runtime-managed Plugins normalize to the same contribution vocabulary.
- Components retain explicit Plugin ownership.
- Concrete dependencies do not masquerade as Interface Imports.
- Product-domain registries do not belong in Core.
- Executable Plugin code cannot expand authority or change its resolved contract after activation begins.
