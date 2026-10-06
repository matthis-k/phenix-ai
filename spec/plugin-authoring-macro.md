# Rust plugin authoring

status: partially implemented
coverage:
  - rust/crates/phenix-sdk/tests/plugin_attribute_only_gate.rs
  - rust/crates/phenix-sdk/tests/plugin_component_authoring.rs
  - rust/crates/phenix-sdk/tests/plugin_attribute_graph.rs
  - rust/crates/phenix-sdk/src/authoring/static_dispatch.rs

## Purpose

Define the Rust-native authoring model for Phenix plugins.

Plugin authors write ordinary Rust state and behavior plus semantic annotations. Generated SDK code lowers those declarations into the canonical Core contribution and runtime interfaces. Authors do not maintain a second manifest tree, factory registry, dispatch ladder, listener registry, tool registration list, skill registration list, resource-registration list, or hook runtime.

The central rule is:

> Plugins declare, require, and provide contracts. The kernel resolves those contracts into one runtime graph.

Concrete plugin dependencies remain available when an exact implementation or lifecycle relationship is part of the plugin's meaning.

## Ownership model

A Plugin is the independently activatable identity, lifecycle, authority, hosting, and durable-ownership boundary.

A Component is a Plugin-owned runtime composition unit. Components require and provide typed contracts and may contribute Layers, Listeners, tools, skills, resources, and public values.

A Contract is a stable typed and versioned capability description. The current Core representation is an Interface plus its request, response, and optional domain-error schemas. Provider and consumer Rust types may differ when their structural schemas are compatible.

A concrete Plugin dependency selects another Plugin implementation. A contract requirement requests a capability and leaves provider selection to the resolver.

## Author vocabulary

The normal authoring vocabulary is:

| Declaration | Meaning | Canonical lowering |
| --- | --- | --- |
| `#[phenix_sdk::contract(...)]` | Declare a typed, versioned contract. | Contract marker and inspectable contract metadata. |
| `#[phenix(require)]` | Require a contract from the active composition. | Component Interface Import. |
| `#[phenix(provide(...))]` | Provide a terminal implementation of a contract. | Component Interface Export plus terminal service participation. |
| `#[phenix(dep)]` | Require one exact Plugin implementation. | Concrete Plugin dependency. |
| `#[phenix(tool)]` | Declare a model-visible callable. | Ordinary callable provider plus generated tool-catalog wiring. |
| `#[phenix(skill(...))]` | Declare static skill content. | Generated skill registration through the standard skill contract. |

`require` and `dep` are deliberately different.

Use `require` when any compatible provider is valid. Use `dep` when the plugin's semantics name a specific plugin implementation, when compile-time binding needs that implementation, or when both plugins must share one lifecycle closure.

Provider replacement must not require consumer source changes when the consumer uses `require`.

## Contract declaration

The canonical contract declaration is:

```rust
#[phenix_sdk::contract("phenix.environment.exec@1")]
pub struct EnvironmentExec;
```

The declaration establishes stable identity and type-level contract metadata. It does not activate a provider and it does not create a graph edge by itself.

A contract declaration may live beside the plugin that introduced the contract, in a shared protocol crate, or in another passive library. The contract is provider-neutral after declaration.

The existing `#[phenix_sdk::interface(...)]` spelling maps to the same Core concept. Implementation should retain it as a compatibility alias during migration, then use `contract` in first-party authoring and documentation.

## Contract requirements

A plugin or component requires a contract with a field:

```rust
#[phenix_sdk::component]
struct Shell {
    #[phenix(
        require,
        authority = Authority::new([PermissionId::parse("environment.exec").unwrap()])
    )]
    environment: Required<Call<EnvironmentExec, ExecRequest, ExecResponse>>,
}
```

Optionality remains explicit in the type:

```rust
#[phenix(require)]
fallback: Optional<Call<EnvironmentExec, ExecRequest, ExecResponse>>,
```

The generated field contributes an ordinary Component Interface Import. The resolver selects the provider in the active Graph Generation. Runtime code receives a typed client for the resolved logical binding rather than a durable pointer to one provider instance.

A required contract that cannot be resolved fails graph construction. An optional contract remains unbound and uses the existing optional-call behavior.

Authority on `require` is the maximum authority that may leave that call edge. It does not grant authority that the caller does not already possess.

## Contract providers

A component provides a contract by annotating the implementing method:

```rust
#[phenix_sdk::component]
impl LocalEnvironment {
    #[phenix(
        provide(EnvironmentExec),
        authority = Authority::new([PermissionId::parse("environment.exec").unwrap()])
    )]
    fn exec(
        &self,
        request: ExecRequest,
    ) -> Result<ExecResponse, EnvironmentError> {
        // ...
    }
}
```

`provide` means this component can satisfy a contract requirement. It therefore implies terminal service participation.

Layering stays explicit:

```rust
#[phenix(layer(EnvironmentExec, priority = 100))]
fn audit(&self) {
    // ...
}
```

A Layer participates in calls to a contract but does not satisfy a missing provider requirement.

`public` remains separate from `provide`. Public projection controls externally exposed application or SDK metadata. Contract provision controls graph resolution.

The existing `#[phenix(export(...))]` spelling maps to the same Core Export contribution. Implementation should retain it as a compatibility alias during migration.

## Concrete plugin dependencies

Use an exact dependency only when identity matters:

```rust
#[phenix_sdk::plugin("acme.agent")]
struct Plugin {
    #[phenix(dep)]
    sessions: acme_sessions::Plugin,
}
```

This selects `acme_sessions::Plugin` and recursively includes its concrete dependency closure.

Do not use `dep` to ask for a capability such as filesystem access, environment execution, model inference, tools, or skills. Those are contract requirements:

```rust
#[phenix(require)]
environment: Required<Call<EnvironmentExec, ExecRequest, ExecResponse>>,
```

The active harness may satisfy that requirement with a local, SSH, container, or third-party implementation without changing the consumer.

## Tool declarations

Static model tools should require one declaration. Authors should not separately create a callable, construct `ToolDefinition`, register it, expose it to the tool catalog, and wire dispatch.

Example:

```rust
#[phenix_sdk::component]
impl Workspace {
    /// Read one file from the selected workspace environment.
    #[phenix(tool(id = "workspace.read"))]
    fn read(&self, input: ReadFile) -> Result<ReadFileOutput, WorkspaceError> {
        // ...
    }
}
```

The generated authoring code must derive or generate:

- one stable callable identity;
- the description from Rust documentation unless explicitly overridden;
- input, output, and domain-error schemas;
- ordinary component dispatch for the callable;
- a `ToolDefinition` with an empty output prefix unless configured;
- the standard tool registration requirement;
- generated registration wiring through the normal tool contract;
- plugin ownership and provenance for inspection and diagnostics.

The generated requirement is real graph metadata. It must be visible in the component descriptor and runtime graph. The macro must not perform hidden provider lookup.

If the active composition has no provider for the required tool-registration contract, graph construction fails in the same way as any other missing required contract.

The callable itself remains an ordinary contract-backed invocation. Tool discovery and invocation do not bypass Layers, authority attenuation, generation pinning, cancellation, provenance, or schema checks.

An explicit ID is required when the tool is referenced outside the plugin or must survive a Rust rename. A plugin-local tool may derive its identity deterministically from the owning plugin and method name.

## Skill declarations

Static skills should also require one declaration.

Example:

```rust
#[phenix(skill(id = "coding.review"))]
const REVIEW_SKILL: &str = include_str!("../skills/review/SKILL.md");
```

Generated authoring code must derive or generate:

- one stable `SkillId`;
- the skill content bytes;
- the standard skill-registration requirement;
- generated registration wiring through the normal skill contract;
- plugin ownership and provenance.

Static skill declarations are for content known from the plugin artifact. Runtime-created, user-authored, remote, or mutable skills continue to use the manual skill API.

A static skill declaration does not grant filesystem, network, tool, repository, or secret authority. Skill activation and tool authority remain separate.

## Tool and skill lifecycle

Generated tool and skill wiring uses the same standard contracts as manual registration.

The implementation must make registration deterministic for one Graph Generation and reconcile replacement without duplicate catalog entries. Plugin replacement must not leave stale tool or skill definitions owned by the previous generation.

This behavior belongs to generated authoring and the existing tool or skill providers. Core does not gain product-specific tool or skill registries.

Generated startup behavior must compose with an author's explicit `#[phenix(start)]` callback. Declaring one tool or skill must not consume the plugin's only lifecycle hook or force the author to forward generated lifecycle calls manually.

## Manual escape hatch

Manual wiring remains supported.

Use it when registration is dynamic, the set of tools or skills depends on runtime discovery, another language runtime produces the descriptors, or the author needs behavior that the static macros cannot represent.

For example, a plugin may explicitly require the tool contract and register a dynamic set during startup:

```rust
#[phenix(require)]
tools: Required<ToolRegistry>;

#[phenix(start)]
fn start(&mut self, ctx: &PluginContext<'_, '_, State>) -> Result<(), Error> {
    for tool in discover_tools()? {
        ctx.sdk.tools.register(tool)?;
    }
    Ok(())
}
```

The manual path and the macro path must converge before runtime resolution or registration. They must not create two tool systems, two skill systems, or two provider-selection paths.

## Authoring API

The canonical Rust authoring attributes are:

```text
#[phenix_sdk::plugin]
#[phenix_sdk::component]
#[phenix_sdk::contract(...)]
#[phenix_sdk::resource(...)]
```

Plugin fields declare owned relationships:

```text
#[phenix(dep)]
#[phenix(component)]
#[phenix(require)]
#[phenix(resource)]
#[phenix(config)]
```

Component and Plugin methods declare behavior:

```text
#[phenix(provide(...))]
#[phenix(tool(...))]
#[phenix(layer(...))]
#[phenix(listen(...))]
#[phenix(value(...))]
#[phenix(start)]
#[phenix(stop)]
```

Static module items may also declare `#[phenix(skill(...))]`.

Annotations carry semantic data that cannot be inferred safely, including stable IDs, authority, layer priority, public visibility, and event identity.

Simple module plugins and stateful struct plugins lower into the same Core model.

## Generated lowering

Generated authoring code derives the Core-facing representation from annotated Rust definitions:

- Plugin identity and maximum authority;
- concrete Plugin dependencies;
- Components and their stable ownership;
- contract declarations;
- typed contract requirements and providers;
- terminal service participation and Layers;
- Events and Listeners;
- public callables and values;
- generated tool and skill registration metadata;
- Plugin Resources and configuration metadata;
- lifecycle callbacks;
- runtime dispatch adapters.

The generated representation is input to the canonical resolver and standard domain contracts. It does not create a parallel runtime topology.

`rust/crates/phenix-sdk/tests/plugin_attribute_only_gate.rs` remains the adoption gate. An attribute-only Plugin must build its graph and activate generated runtime behavior without parallel author wiring.

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

- resolves concrete dependency closure and contract providers;
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

Cross-Plugin contract identity never derives from a provider implementation's local field or method name.

## Migration

Implementation should make the new vocabulary additive first:

1. add `#[phenix_sdk::contract]` as the canonical spelling for the current interface marker;
2. add `#[phenix(require)]` as the canonical spelling for Component Import authoring;
3. add `#[phenix(provide(...))]` as the canonical terminal-provider spelling;
4. retain `interface`, `import`, and `export` as compatibility aliases while first-party code migrates;
5. add `tool` and `skill` lowering on top of the standard tool and skill contracts;
6. migrate first-party plugins away from manual static tool and skill registration;
7. remove compatibility aliases only in an intentional compatibility-breaking change.

Core contribution names do not need to change in the same PR. `InterfaceId`, Interface Import, and Interface Export may remain kernel vocabulary while the SDK presents the clearer contract authoring vocabulary.

## Required regressions

The implementation is complete when tests prove:

- an attribute-only plugin can declare a contract;
- one plugin can require that contract and another can provide it;
- provider replacement changes future runtime calls without consumer source changes;
- `dep` still selects one exact Plugin and does not behave like `require`;
- `provide` lowers to terminal service participation;
- a `tool` method appears in the tool catalog without an explicit registration call;
- the tool invokes the annotated method through normal kernel dispatch;
- a static `skill` appears in the skill catalog without an explicit registration call;
- generated tool and skill requirements are visible in graph inspection;
- missing generated requirements fail through normal graph-resolution errors;
- plugin replacement leaves no stale generated tool or skill catalog entry;
- explicit manual tool and skill registration still works;
- generated lifecycle wiring composes with author-defined `start` and `stop` callbacks;
- compatibility spellings lower to the same canonical contribution data during migration.

## Invariants

- One Rust authoring model lowers into one canonical Plugin model.
- Plugins declare, require, and provide contracts.
- Contract requirements and concrete Plugin dependencies remain distinct.
- Provider replacement does not require consumer source changes.
- Static tool and skill declarations require no parallel manual registration.
- Generated tool and skill wiring uses ordinary contracts and remains inspectable.
- Manual dynamic registration remains available.
- Runtime topology belongs to the resolver and Graph Generation, not to authoring macros.
- Layers provide synchronous interposition; Events describe facts that already occurred.
- Plugin Resources and lifecycle callbacks are generated from declarations.
- Dynamic values cross Phenix boundaries as `PhenixValue`, then become local typed values.
- Runtime and executor choices do not change Plugin semantics.

## Related contracts

- `plugin-contributions.md` owns the Core contribution vocabulary.
- `plugin-resolution.md` owns provider resolution.
- `plugin-call-binding.md` owns runtime and compile-time call binding.
- `plugin-sdk-context.md` owns typed access from plugin business logic.
- `plugin-host.md` owns executable Plugin access to kernel capabilities.
- `plugin-events.md` and `plugin-service-layering.md` own Event and Layer semantics.
- `plugin-persistence.md` owns durable schema and Store behavior.
- `plugin-runtime-bridges.md` owns Plugin Runtime Adapter and live Plugin-management semantics.
