# Phenix glossary

This file is the source of truth for architecture vocabulary. Code, documentation, tests, configuration, tracing, and packaging should preserve these distinctions. Qualify a term when its domain is not obvious from the containing scope.

## Primary terms

| Term | Definition | Key distinctions |
| --- | --- | --- |
| **Kernel** | The mechanism layer in `phenix-core`. It resolves and dispatches services, activates generations, hosts plugin lifecycle, attenuates authority, and enforces shared persistence, event, task, and tracing rules. | The Kernel is not the product assembly, frontend, model implementation, or durable owner of plugin domain state. |
| **Plugin** | An independently selectable unit that contributes behavior or resources through declared manifests and components. | A plugin owns its domain behavior and durable state. Resource-only plugins contribute resources without executable code. |
| **Interface** | A typed, versioned contract imported and exported by components. | An interface is the semantic contract. A service is the lower-level kernel dispatch identity used to execute an implementation. |
| **Configuration** | Typed input that selects or parameterizes runtime behavior before a generation is activated. Configuration contributions are attributed, authority-bounded, and merged deterministically. | Product configuration, configuration frontends, resolved configuration, and `KernelConfig` are different stages of the same flow. Configuration is not mutable runtime state. |
| **Generation** | One immutable resolved runtime topology identified by `GenerationId`. It contains the selected plugins, component graph, dispatch topology, resources, resolved configuration, policy, and authority ceiling. | Activation installs a generation atomically. Reconciliation produces and activates a replacement generation instead of mutating topology piecemeal. |
| **Execution** | One application-level unit of work tracked through the execution domain. It may contain many invocations and model turns. | An invocation is one call across an interface or service. `PluginExecution` is a qualified manifest term describing how a plugin implementation is hosted. |
| **Authority** | The set of `PermissionId` values available at a call site. Delegation can only attenuate authority by intersection. | A permission is one grant. Authority is the effective set of grants carried through execution and invocation boundaries. |

## Derived architecture terms

**Runtime** is a live system that hosts an activated generation. `PhenixRuntime` is the configured live product instance. Runtime is not a synonym for generation or configuration.

**Harness** is the supported product assembly. It selects plugins, configuration, persistence, authority, and packaged resources, then constructs a `PhenixRuntime`. Harness terminology belongs to product assembly, not generic kernel topology.

**Component** is a plugin-owned logical unit that imports and exports interfaces. The resolved component graph records the selected component implementations and their bindings for a generation.

**Service** is a kernel dispatch identity. A resolved service plan selects a terminal provider and any ordered layers; component interface bindings lower to these dispatch identities.

**Invocation** is one resolved call across an interface or service boundary. It carries call scope, effective authority, dispatch provenance, and a semantic outcome or runtime failure.

**Provider** is a concrete implementation selected for a contract or domain. Qualify it when more than one provider domain exists, for example component provider, model provider, or persistence provider.

**Adapter** is a translation boundary. Use the pattern name when the implementation translates between protocols or execution environments. Examples are the ACP adapter package (`phenix-adapter-acp`), `ModelAdapter`, and `PluginRuntimeAdapter`. An adapter should not become the durable owner of the domain it translates.

**Binding** is a resolved association between identities that must remain stable for a scope. Examples are `ProviderBinding` and `PluginRuntimeBinding`.

**Listener** observes an event stream or state transition. Name the observed domain when scope does not already make it obvious.

**Callable** is a typed invocable reference with an input and output contract. Callable identity is separate from authority. Do not use capability as a synonym for a callable.

**Permission** is one named authority grant. Permission identifiers describe what may be done, not which implementation will do it.

**Model adapter** translates Phenix model requests into an external model protocol or provider client. A **model provider** is the provider identity, such as a vendor or route, exposed through that adapter.

**Frontend adapter** translates an external frontend protocol into Phenix application operations. It does not own the application semantics it exposes.

**Frontend service provider** is a provider of a frontend-owned service. It is unrelated to model-provider identity unless the specific service contract says otherwise.

**Persistence backend** is a storage implementation behind `PersistenceBackend`. Its supported semantics are described by `PersistenceBackendFeature`; it is not an execution or model backend.

## Naming rules

- Name the domain before a generic role when the containing scope does not already disambiguate it.
- Use architectural pattern names only when the implementation performs that pattern.
- Prefer `Generation` for immutable resolved topology and `Runtime` for the live host that executes it.
- Prefer `Callable` for invocable references and `Permission` or `Authority` for access control.
- Prefer `ModelAdapter` for model protocol translation. Keep `ModelProvider` for provider identity and provider-specific routing or authentication.
- Keep historical product names such as Harness out of generic kernel types and diagnostics unless they refer to the product assembly itself.
- Keep historical serialized compatibility keys unchanged in migrations and migration fixtures. Rename current APIs and symbols, not persisted legacy wire formats.
- Generic public names such as `Manager`, `Provider`, `Adapter`, `Frontend`, `Backend`, `Runtime`, `State`, `Handler`, `Observer`, `Registry`, `Capability`, or `Binding` need a domain qualifier unless their module or type scope makes the role unambiguous.
