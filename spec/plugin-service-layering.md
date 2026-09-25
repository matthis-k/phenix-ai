# Layered service invocation

status: implemented
coverage:
  - rust/crates/phenix-core/src/service_layer_dispatch_regression.rs
  - rust/crates/phenix-sdk/tests/plugin_attribute_only_gate.rs
  - rust/crates/phenix-core/src/runtime_topology_generation_regression.rs

## Purpose

Allow Plugins to interpose on an operation without replacing the complete terminal implementation.

Layers are the canonical kernel-owned lowering for synchronous hooks. A hook that must run before an operation, wrap it, deny it, or inspect its result is a Layer on the semantic service boundary.

A resolved invocation chain contains zero or more ordered Layers and one Terminal Provider. The resolver constructs the chain before activation and pins it to one Graph Generation.

This document extends `plugin-resolution.md` and `plugin-contributions.md`.

## Roles

**Terminal Provider.** The resolved implementation that completes the service when no Layer handles or denies it first.

**Layer.** Ordered synchronous interposition around the remainder of the resolved chain.

**Continuation.** A kernel-issued one-shot capability that advances one Layer to the next chain position.

A Layer and an Event Listener are different mechanisms. Layers can affect an operation before it completes. Listeners observe facts that already exist.

## Resolution

Layer membership and order are graph semantics.

The resolver selects the terminal provider and enabled Layers before activation. Registration order is not semantic ordering. Policy and declared priority determine the resolved chain.

A topology or policy change creates a new Graph Generation. Already-started calls remain pinned to their original generation.

## Layer behavior

A Layer may:

- handle the request without delegation;
- delegate once and transform the returned value;
- deny with a typed kernel denial;
- fail.

Delegation uses the kernel continuation associated with the current invocation. The Layer does not rediscover the service or call the terminal implementation directly.

## One-shot continuation

A continuation is bound to:

- the current service invocation;
- the next position in the resolved chain;
- the starting Graph Generation;
- the current effective authority.

It may be consumed once. A second use fails.

Invoking the same service recursively instead of using the continuation is rejected as same-service re-entry.

## Authority

Each step receives authority attenuated by caller authority, Plugin and Component limits, and the resolved contribution requirements.

A Layer cannot request more authority through delegation than it received. The continuation preserves attenuation into the remainder of the chain.

## Failure and fallback

Layer failure and terminal execution failure are execution failures. They do not cause live provider search.

Provider fallback is a separate generation-pinned resolution mechanism defined by `plugin-resolution.md`. An already-started failing terminal does not silently switch to a lower-priority provider.

## Authoring

Rust component methods annotated with `#[phenix(layer(...))]` lower into ordinary Layer contributions and generated runtime dispatch.

The authoring surface does not create a second Layer registry or execution engine. Generated Layers enter the same Core chain used by manually constructed test fixtures and runtime-managed Plugins.

## Provenance and cancellation

Layer invocation runs under the same generation, authority, cancellation, and provenance machinery as terminal service invocation.

Cancellation is checked at the canonical Plugin call boundary. Late results from a cancelled call are rejected. See `plugin-threading.md`.

## Invariants

- Every resolved service chain has at most one active terminal path for an invocation.
- Layer order is resolved before activation.
- Continuations are one-shot and invocation-scoped.
- Same-service recursive re-entry cannot replace continuation use.
- Delegation cannot expand authority.
- Layer failure does not trigger provider search.
- Layers and Event Listeners remain distinct mechanisms.
- Generated Rust Layers use the canonical Core runtime path.
