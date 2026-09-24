# Stable runtime and host interfaces

status: implemented
coverage:
  - rust/crates/phenix-core/src/plugin/context.rs
  - rust/crates/phenix-sdk/src/authoring/context.rs
  - rust/crates/phenix-core/src/provider_fallback_regression.rs
  - rust/crates/phenix-core/src/host_authority_regression.rs

## Purpose

Give Plugin code stable typed access to resolved runtime behavior while keeping replaceable implementations owned by ordinary Components and Plugins.

Core and passive SDK crates may own Interface vocabulary and typed handles. They do not thereby own the selected Provider implementation. Provider selection follows `plugin-resolution.md`.

## Plugin context

The canonical runtime view is `PluginContext`:

```text
PluginContext
  kernel   generic kernel mechanisms
  sdk      typed resolved Interface clients
  plugin   current Plugin identity, settings, and state
  call     current authority and Graph Generation
```

Plugin business logic should depend on this scoped view rather than mutable kernel internals.

## Kernel access

`KernelAccess` exposes generic kernel-owned mechanisms that are not replaceable product Providers, including:

- cancellation token access;
- Layer continuation;
- task scopes;
- Event dispatch;
- structural request decoding and response encoding;
- Plugin-owned durable persistence operations.

These operations remain authority- and generation-scoped through the underlying `PluginHost`.

## Interface clients

`SdkClient<I>` is a typed consumer-side handle for one imported `ComponentInterface`.

The client invokes through the caller Component's resolved Import. It does not perform live provider discovery and it does not hold a provider implementation pointer.

Typed calls lower requests to `PhenixValue`, use the kernel-mediated Import binding, and parse the response into the consumer's local type.

`SdkContract` lets passive SDK vocabulary name an Interface without depending on a provider implementation crate.

`SdkObject` combines stable provider-owned object identity with an `SdkClient`; provider state remains owned by the provider Plugin.

## Default SDK

The Rust SDK builds `PhenixSdk` from typed `SdkClient` values for common contracts such as sessions, models, tools, skills, context, options, and configuration.

Plugins may request additional SDK contracts through the same `SdkContract` and `SdkClient` mechanism.

An ergonomic expression such as a session or context SDK call is therefore a typed call through a resolved Component Import, not a hard-coded default implementation.

## Domain Interfaces

A Domain Interface is an ordinary resolved Interface used for replaceable Phenix behavior.

Domain Interface identity and structural schema are stable vocabulary. The active Provider is selected by the Graph Generation.

Default implementations remain ordinary Providers and can be replaced without changing consumer source when the replacement satisfies the same contract and policy.

## Host Interfaces and Host Capabilities

A Host Interface is a resolved Interface whose implementation provides controlled interaction with the environment.

A Host Capability is different: it is an authority-bearing kernel or environment handle exposed through `PluginHost` for privileged operations that cannot be modeled as an ordinary provider lookup.

A Provider of a Host Interface may internally require Host Capabilities, but exporting an Interface never grants those capabilities automatically.

## Structural boundary

Typed Interface clients use `PhenixValue` and `PhenixSchema` at the dynamic boundary. Provider and consumer Rust types may differ.

Structural mismatch is reported through the canonical kernel diagnostic path. See `typed-structural-boundaries.md`.

## Generation and authority

Interface clients are scoped to the calling Plugin Host and caller Component. Their provider binding is therefore the binding resolved for the current Graph Generation.

Calls inherit effective authority from the current host and cannot expand it.

Provider fallback, when explicitly enabled, uses the generation-pinned Provider Plan rather than mutating the Interface client.

## Invariants

- Typed runtime access does not hard-code provider implementations.
- `PluginContext` separates kernel mechanisms, SDK contracts, Plugin state, and call state.
- `SdkClient` invokes declared resolved Imports rather than searching live Plugins.
- Passive SDK crates may own stable Interface vocabulary without becoming runtime Plugins.
- Domain Interfaces and Host Interfaces use the same provider-resolution machinery.
- Host Interfaces and Host Capabilities remain distinct concepts.
- Provider replacement does not require consumer source changes.
- Interface calls remain authority- and Graph Generation-scoped.
