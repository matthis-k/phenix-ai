# Plugin host

status: implemented
coverage:
  - rust/crates/phenix-core/src/runtime/tests.rs
  - rust/crates/phenix-core/src/runtime_provider_host_regression.rs
  - rust/crates/phenix-core/src/third_party_component_regression.rs
  - rust/crates/phenix-core/src/host_authority_regression.rs
  - rust/crates/phenix-core/src/runtime_component_parity_regression.rs
  - rust/crates/phenix-core/src/tasks.rs


Terminology follows [the architecture glossary](glossary.md).

## Purpose

Define `PluginHost`, the one kernel-owned execution boundary through which executable Plugin code receives Host Capabilities and returns Plugin results.

Plugin runtime choice changes transport and isolation. It does not change Plugin identity, Interface semantics, authority, lifecycle, Graph resolution, persistence ownership, or provenance.

This document extends `spec/plugin-authoring-macro.md`, `spec/plugin-contributions.md`, `spec/plugin-resolution.md`, and `spec/plugin-runtime-bridges.md`.

## Roles

**Plugin authoring API.** Lets an author declare Plugin state, behavior, dependencies, Plugin Resources, and semantic metadata.

**Core descriptor.** The generated or runtime-derived resolver input. Static Plugin authors do not maintain this descriptor by hand.

**PluginHost.** The kernel-owned execution boundary for one executable Plugin instance.

**Plugin runtime adapter.** A Plugin that maps another Plugin runtime onto the canonical Plugin API.

**Host Capability.** An authority-bearing handle for a kernel- or environment-owned operation such as filesystem, process, network, clock, credentials, terminal, or frontend callback access.

These roles are distinct. A Plugin runtime adapter translates execution. It does not gain composition authority. A Host Capability grants one bounded operation. It does not expose mutable kernel internals.

## Kernel ownership

The kernel owns:

- Plugin instance identity and Generation binding;
- Plugin lifecycle state and transitions;
- authority grants and attenuation;
- Host Capability construction;
- Interface dispatch through the Resolved Graph;
- Event delivery;
- controller and task scheduling;
- cancellation and live-call tracking;
- normalized Host Capability and Plugin runtime adapter failures;
- provenance.

A Plugin never receives mutable access to the kernel runtime, raw registries, Persistence Provider handles, or database connections.

## Execution runtimes

The canonical model is:

```text
Plugin API
  |
  +-- embedded runtime
  |     `-- generated or generic Rust PluginInstance adapter
  |
  +-- Plugin runtime adapter plugin
  |     `-- guest plugin in WASM, TypeScript, Python, process, remote, ...
  |
  `-- resource-only plugin
        `-- no executable instance
```

Core initially supplies only `embedded`. Other Plugin runtimes are supplied by ordinary Plugin runtime adapter Plugins as defined in `spec/plugin-runtime-bridges.md`.

A Plugin runtime adapter may use a process Transport internally. An "external plugin" is therefore an execution arrangement, not a second semantic Plugin model.

Rust dynamic libraries are not an implicit Plugin runtime.

## Lifecycle

Executable Plugin instances move through kernel-owned lifecycle states:

```text
registered
active
stopped
```

Activation stages candidate instances and commits them as `active` with the resolved Generation. Stop transitions a plugin to `stopped`. Each active Plugin instance belongs to one Generation and Artifact Revision. Runtime-local process handles belong to that generation and are never Durable State.

Resource-only Plugins have no executable lifecycle callbacks. Their declarations still participate in Graph construction and Plugin Resource activation.

## Host capabilities

Executable Plugins receive only Host Capabilities granted by the resolved authority policy.

Examples include:

- invoking an imported Interface through kernel dispatch;
- reading or mutating an authorized Plugin Resource;
- emitting an Event;
- inspecting permitted runtime metadata;
- requesting a bounded controller or task operation;
- filesystem access;
- process execution;
- network access;
- clock access;
- credential access;
- terminal or frontend callbacks.

Product-domain operations such as sessions, models, tools, skills, context, memory, orchestration, or repository behavior are not special `PluginHost` methods. Plugins use the corresponding neutral Interface contracts through ordinary Imports.

Every Host Capability operation rechecks Effective Authority. Holding one Host Capability does not imply ambient access to another.

## Invocation

The Resolved Graph chooses the Provider and Layer chain before execution.

Invocation follows this shape:

```text
pin Generation
  -> establish Effective Authority
  -> create live-call scope
  -> enter resolved Layer/Provider chain
  -> adapt through Plugin runtime adapter when required
  -> normalize result or error to PhenixValue
  -> record provenance
  -> close live-call scope
```

Provider code cannot execute before Interface compatibility, Provider Binding, and authority checks complete.

Callbacks from a Plugin re-enter ordinary kernel dispatch and authority enforcement.

## Runtime providers

A Plugin runtime adapter receives authority for its own bridge implementation and a separate attenuated guest `PluginHost` for the guest Plugin.

Plugin runtime adapter authority never becomes guest Plugin authority.

The Plugin runtime adapter may translate:

- Plugin Artifacts;
- lifecycle calls;
- `PhenixValue` to native values and back;
- guest Host Capability handles;
- private concurrency and error models.

It may not redefine Plugin identity, Interfaces, authority, persistence ownership, Graph semantics, lifecycle semantics, or provenance.

## Cancellation

Each in-flight executable call has a kernel-owned live-call scope.

Cancellation may:

- prevent undispatched work from starting;
- signal an embedded worker through an explicit cancellation handle;
- send correlated cancellation to a Plugin runtime adapter;
- terminate a guest process or runtime instance when policy permits hard cancellation;
- reject late results from cancelled scopes.

Userspace Interfaces may define additional domain cancellation semantics. `PluginHost` guarantees only the generic runtime behavior.

## Errors

Kernel-facing execution failures distinguish at least:

- Provider unavailable;
- authority denied;
- invalid structural request or response;
- Provider execution failure;
- Plugin runtime adapter or protocol failure;
- cancelled;
- Host Capability denied;
- guest crashed or disconnected.

Userspace Interfaces may define richer typed domain failures.

## First-party and third-party equality

A first-party runtime Plugin receives no private Host Capability, implicit authority, Provider priority, persistence privilege, or lifecycle path unavailable to a compatible third-party implementation.

If a first-party implementation requires a capability that an equivalent third-party Plugin cannot request through the canonical Plugin API, the architecture is incomplete.

## Invariants

- `PluginHost` is the one executable Plugin boundary.
- Plugin runtime choice changes transport and isolation, not Plugin semantics.
- Static authoring does not require hand-written factories or Host registration.
- Resource-only Plugins need no fake executable instance.
- Host Capabilities remain generic and authority-bearing.
- Product-domain behavior uses ordinary neutral Interfaces rather than private Host methods.
- Plugin callbacks cannot bypass Graph resolution, authority, or Plugin Resource ownership.
- Plugin runtime adapter authority is separate from guest Plugin authority.
- Runtime-local handles are Generation-bound and never reconstructed as Durable State.
- First-party Plugins receive no private privileged path.

## Required regressions

- embedded and bridged Providers expose the same logical Interface semantics;
- Provider code cannot execute before authority enforcement;
- an ungranted Host Capability is rejected;
- a Plugin cannot obtain mutable runtime, registry, Persistence Provider, or database handles;
- an alternate third-party Provider can request every supported Host Capability needed by an equivalent first-party Provider;
- product concepts are absent from the generic Host API;
- callbacks re-enter normal Graph and authority checks;
- cancellation and crash/disconnect always close live-call scopes;
- Plugin runtime adapter authority cannot leak into guest Plugin authority;
- a resource-only Plugin activates without an executable runtime;
- static Rust Plugin authors do not maintain manual `PluginFactory` wiring.
