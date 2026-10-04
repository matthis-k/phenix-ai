# Architecture glossary

status: canonical

## Purpose

This file defines the canonical Phenix architecture vocabulary. New specs, public APIs, and architecture discussion should use these terms consistently.

Existing identifiers may retain older names for compatibility. Those names are migration aliases, not separate concepts.

## Root terms

### Kernel

The **Kernel** is the live execution engine.

It owns generic mechanisms: resolution, dispatch, Plugin lifecycle, Generation residency, Authority enforcement, events, tasks, persistence coordination, invocation state, and reconciliation execution.

`phenix-core::Kernel` implements this concept. **Core** names the implementation layer; **Kernel** names the live engine.

### Plugin

A **Plugin** is an independently identifiable ownership and lifecycle unit.

A Plugin declares what it contains, requires, provides, owns, and may execute. Plugins own domain behavior and state. The Kernel owns generic composition and execution mechanisms.

### Interface

An **Interface** is a stable, versioned, typed contract between Components.

Consumers depend on an Interface, not on a concrete implementation. `InterfaceId`, `InterfaceSchema`, and `ComponentInterface` implement this concept.

Use **contract** as ordinary English or for an explicitly named external contract. Do not introduce a second Plugin-composition concept beside Interface. The current `ContractId` is an alias of `InterfaceId`.

### Configuration

**Configuration** is declarative input that determines what Phenix composes and how ambiguous choices are resolved.

Configuration may select Plugins, select or prefer Interface Providers, set policy, contribute Plugin-owned values, or otherwise constrain resolution. Configuration does not directly mutate live topology.

### Generation

A **Generation** is one immutable resolved result of Plugin declarations plus Configuration.

A Generation fixes the selected Plugin set, Component declarations, Interface bindings, executable topology, resources, policy, and Authority ceiling used by work admitted to that Generation.

Changing a semantic input that changes those results produces another Generation.

### Execution

An **Execution** is one bounded unit of semantic work.

An Execution runs against exactly one Generation and under one Authority. Nested work does not silently switch Generation. Another Generation may be selected only when a new Root Execution is admitted.

### Authority

**Authority** is the set of Permissions available to an Execution or invocation.

Authority may stay equal or attenuate across boundaries. It cannot expand downstream.

The current `Authority` type implements this as set intersection.

## Composition

### Component

A **Component** is a Plugin-owned composition unit. It declares Interface Requirements and Provisions and groups their executable behavior.

### Requirement

A **Requirement** declares that a Component needs an Interface.

The current resolver representation is `ComponentImport`. Requirement is the author-facing term because the consumer depends on the Interface, not on a specific Provider.

### Provision

A **Provision** declares that a Component implements an Interface.

The current resolver representation is `ComponentExport`.

### Provider

A **Provider** is a Component whose Provision can satisfy a specific Interface Requirement.

Provider is relational. Prefer qualified forms such as **Environment Provider**, **model provider**, or **persistence provider** when the Interface is not obvious.

Do not use bare Provider for Plugin execution hosting.

### Interface binding

An **Interface binding** is the resolved association between one Requirement and one Provider.

A consumer executes through the Interface binding and remains implementation-independent.

### Provider selection

**Provider selection** is Configuration that constrains or prefers which eligible Provider satisfies an Interface.

A user or Product may select a concrete Provider while the consuming Plugin remains implementation-agnostic at execution time.

### Concrete Plugin dependency

A **concrete Plugin dependency** requires a particular Plugin identity.

It differs from an Interface Requirement:

```text
Interface Requirement       implementation-independent
Concrete Plugin dependency  implementation-specific
```

Both are valid.

### Composition

A **Composition** is the complete unresolved set of Plugin declarations and Configuration considered for one Generation.

### Resolution

**Resolution** deterministically validates a Composition and produces a Generation.

It owns dependency closure, Interface compatibility, Provider selection, Interface bindings, layer ordering, resource validation, and Authority constraints.

### Product

A **Product** is a named reusable Configuration preset intended to produce a supported Composition.

`basic` and `full` are Products. A Product is not a Runtime or Generation.

## Generation lifecycle

### Runtime graph

The **Runtime graph** is the inspectable graph representation of one Generation: Components, Interface bindings, service chains, Listeners, Triggers, and ownership.

It is a view of a Generation, not a synonym for Runtime.

### Candidate Generation

A **Candidate Generation** is a fully resolved Generation being considered for activation or residency.

### Resident Generation

A **Resident Generation** has executable Plugin state loaded in the Kernel and may be selected for new Root Executions.

### Default Generation

The **Default Generation** is the Resident Generation used when a new Root Execution does not explicitly select another one.

Prefer this over **active Generation** when other Resident Generations may also execute explicit roots.

### Reconciliation

**Reconciliation** computes and applies the transition from one Generation to another.

### Promotion

**Promotion** makes a Resident Generation the Default Generation.

### Retirement

**Retirement** removes a Generation from future selection and releases its executable state when no retained work requires it.

## Execution

### Root Execution

A **Root Execution** starts a causal execution tree. It captures Generation selection, initial Authority constraints, and any host-pinned Interface bindings.

### Child Execution

A **Child Execution** is derived from another Execution. Its Authority may be attenuated. Ordinary nested work remains in the Root Execution's Generation.

### Attempt

An **Attempt** is one concrete planned and dispatched attempt to perform work within an Execution.

### Session

A **Session** is durable conversation or application state that may contain activity from multiple Executions. It is not permanently bound to a Generation.

### Worker Task

A **Worker Task** is durable delegated-work state that may create or bind to a Child Execution.

### Kernel Task

A **Kernel Task** is process-local asynchronous work owned by Kernel execution infrastructure.

Use the qualified terms when both task concepts could apply.

## Invocation

### Callable

A **Callable** is typed behavior that can be invoked. It is not necessarily model-visible.

### Endpoint

An **Endpoint** is a resolved executable target for an Interface or Service invocation.

### Entrypoint

An **Entrypoint** is an Endpoint that may start a new Execution through a Trigger.

### Trigger

A **Trigger** declares a condition that starts a new Execution at an Entrypoint.

### Tool

A **Tool** is a Callable deliberately projected to a model.

Tool discovery and Tool invocation should derive from the same declaration and resolved Endpoint.

### Service

A **Service** is a resolved invocation chain with zero or more Layers and one terminal implementation.

An Interface is a typed dependency contract. A Service is invocation topology.

### Layer

A **Layer** is synchronous interposition within an existing Service invocation.

### Continuation

A **Continuation** is the one-use invocation-scoped capability used by a Layer to execute the remaining Service chain.

### Event

An **Event** is a fact emitted after something occurred.

### Listener

A **Listener** reacts to an Event. It cannot change the operation that already completed.

### Hook

**Hook** is authoring shorthand only. A synchronous Hook lowers to a Layer. An observational Hook lowers to an Event plus Listener. There is no separate Hook runtime.

## Runtime and Plugin hosting

### Runtime

A **Runtime** is one live Phenix instance around a Kernel.

A Runtime has one Default Generation and may keep other Resident Generations.

Do not use Runtime as a synonym for Kernel, Generation, Plugin runtime, or Runtime graph.

### Plugin runtime

A **Plugin runtime** is the mechanism used to host executable Plugin code.

### Plugin runtime adapter

A **Plugin runtime adapter** maps a non-embedded Plugin runtime onto the canonical Plugin API expected by the Kernel.

The current code name `PluginRuntimeProvider` is a compatibility name.

### Plugin artifact

A **Plugin artifact** is immutable content-addressed input used to instantiate executable Plugin behavior.

## Security

### Permission

A **Permission** is one atomic named right contained in Authority.

The current implementation name `CapabilityId` is retained for compatibility. Model features, application features, and capability references are separate concepts.

### Authority ceiling

An **Authority ceiling** is the maximum Authority a Composition, Generation, Plugin, or execution boundary may grant.

### Effective Authority

**Effective Authority** is the Authority remaining after all applicable ceilings and restrictions are intersected.

### Feature

A **Feature** is supported optional behavior, such as a model or Application protocol feature. A Feature is not Authority.

### Capability reference

A **Capability reference** is an authority-bearing or unforgeable reference to behavior or state.

Avoid bare **capability** where Permission, Feature, or capability reference is more precise.

## Environment and Workspace

### Environment

An **Environment** is the replaceable Interface that defines the filesystem and process world visible to operations.

Local, SSH, container, and sandbox implementations are Environment Providers.

### Workspace

A **Workspace** is project semantics inside one Environment. It owns roots, project-relative identity, versions, conflict checks, search, and patch semantics. It does not choose the Environment implementation.

## Application boundary

### Application

An **Application** is user-facing software that consumes Phenix.

### Application interface

The **Application interface** is the protocol-neutral contract exposed to Applications.

### Protocol

A **Protocol** defines interoperable message semantics between an Application-side client and a Phenix-side adapter.

### Protocol adapter

A **Protocol adapter** translates an external Protocol into canonical Phenix application operations and events.

### Client SDK

A **Client SDK** is reusable Application-side code implementing a Protocol or Application interface.

### Language binding

A **Language binding** exposes a Client SDK through another programming language.

Always qualify it when Interface binding may also be in scope.

### Transport

A **Transport** moves Protocol bytes or messages. It does not define Protocol semantics.

## Current implementation names

The following names predate this glossary. They are compatibility names for the listed canonical concepts.

| Existing name | Canonical concept |
| --- | --- |
| `GraphGenerationId` | Generation identity |
| `RuntimeGeneration` | Generation topology held by the Kernel |
| `ResolvedHarness` | resolved Generation / resolved Composition |
| `PhenixHarness` | supported Product host |
| `HarnessBuilder` | Product builder |
| `RuntimeId` | Plugin runtime identity |
| `PluginRuntimeProvider` | Plugin runtime adapter |
| `CapabilityId` inside `Authority` | Permission identity |
| `ComponentImport` | Requirement |
| `ComponentExport` | Provision |
| `ConfigurationFrontend*` | Configuration source/adapter integration |
| crate/package `phenix-harness` | compatibility package name for Product assembly |

New architecture prose should use the canonical concept. Compatibility names must not be used to infer different semantics.

## Core relationship

```text
Plugin declarations + Configuration
              |
              v
          Resolution
              |
              v
          Generation
              |
              v
            Kernel
              |
              v
          Execution
              |
          Authority

Plugins communicate through Interfaces.
```

Example:

```text
Workspace Plugin
    requires Environment Interface

SSH Environment Plugin
    provides Environment Interface

Configuration
    selects SSH Provider

Resolution
    creates Workspace.Environment -> SSH Interface binding

Execution
    invokes Environment through that binding

Kernel
    dispatches under the Execution's Authority
```
