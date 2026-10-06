# Plugin SDK context

status: implemented

## Purpose

Define the runtime boundary between a plugin instance, the kernel, and typed SDK clients.

The Core `PluginInstance` ABI receives `PluginHost`. A plugin callback projects that host and its instance data into `PluginContext`. Business logic receives the context instead of `PluginHost`.

Generic context types live in `phenix-core`. The default `PhenixSdk` and `phenix_context` helper live in the passive `phenix-sdk` library. The optional `phenix-plugin-api` runtime plugin provides convenience services used by some default SDK clients.

## Runtime context

Plugin business logic receives one `PluginContext` value:

```text
PluginContext
  kernel
  sdk
  plugin
    id
    settings
    state
  call
    authority
    graph_generation
```

The fields have distinct roles:

- `kernel`: scoped access to generic kernel mechanisms;
- `sdk`: typed userspace clients bound to the component handling the callback;
- `plugin`: current plugin identity plus the settings and state view selected by the plugin instance;
- `call`: metadata for the current kernel-mediated callback.

`PluginContext` borrows the live `PluginHost`. Its `Settings` and `State` slots are generic. A stateful plugin normally supplies `&Settings` and `&mut State`. A stateless plugin may supply `()`.

The context does not create another owner for plugin state. The dynamic `PluginInstance` owns live state and constructs a fresh context for each host-backed callback.

The Core `stop` callback receives a scoped host. Stop logic uses instance-owned data and only the host operations valid during shutdown.

## ABI adapter

`PluginInstance` methods are the runtime ABI adapter. They parse the request, construct the context, call business logic, and serialize the response.

Business logic should use these explicit capabilities:

```text
ctx.kernel  generic kernel mechanisms
ctx.sdk     typed component imports
ctx.plugin  current plugin data
ctx.call    current call metadata
```

A plugin may define a local context alias and SDK dependency struct for its declared contract requirements. Unavailable optional requirements then remain absent from the business-logic type.

`phenix_context` constructs a context with `PhenixSdk`. The caller still chooses the settings and state view placed in `ctx.plugin`.

## SDK access

`PhenixSdk` is an authoring type, not a runtime plugin. Constructing it registers nothing and selects no provider.

The default SDK exposes typed clients such as:

```text
ctx.sdk.sessions
ctx.sdk.models
ctx.sdk.tools
ctx.sdk.skills
ctx.sdk.context
ctx.sdk.options
ctx.sdk.config
```

Some helpers call standard domain interfaces directly. Convenience operations such as session policy, tool registration, skill registration, and config reads use interfaces provided by `phenix-plugin-api` when that plugin is selected and bound.

Each invocation still goes through the caller component's declared contract requirement, resolved provider, and effective authority. A missing API plugin therefore produces the ordinary missing or unbound dependency failure. `PhenixSdk` does not provide an in-process fallback.

Static `#[phenix(tool)]` and `#[phenix(skill(...))]` declarations generate the required SDK contract edges and registration calls. Plugin business logic does not need to call `register` for those static declarations. Dynamic discovery and runtime-created tools or skills still use the explicit SDK APIs. Generated and manual registration converge on the same tool and skill providers.

Plugins can add typed SDK contracts and access them through:

```rust
let testing = ctx.sdk.require::<TestingSdk>();
```

The returned client is usable only when the caller component declared and resolved the matching contract requirement.

## Cross-plugin data

Plugin-owned settings, state, internal resources, and implementation objects are private.

Another plugin may access data only through an exported typed contract. The provider owns the state and decides which operations and values the contract exposes.

Stateful data normally crosses the boundary as a typed handle:

```rust
let testing = ctx.sdk.require::<TestingSdk>();
let run = testing.runs().find("run-123")?;
let result = run.result()?;
```

`run` contains stable identity plus a scoped client back to the provider. It is not a shared reference to provider internals.

## SDK objects

Self-contained immutable values may cross by value. Stateful or behavioral objects cross as capability objects with stable identity and a typed client.

Operations remain kernel-mediated. Handles are callback-scoped because their clients borrow the current runtime host. Persistent plugin state stores stable object identity and reacquires a client on the next callback.

Raw Rust references to another plugin's internal state are not SDK contracts.

## Recursive calls

An SDK call may invoke another plugin, and that provider may perform further SDK calls.

Each provider constructs a fresh `PluginContext` for its host-backed callback. The kernel owns authority attenuation, component bindings, provenance, and cycle protection across the call chain.

## Ownership split

```text
phenix-core
  PluginContext
  KernelAccess
  SdkClient
  SdkContract
  SdkObject

phenix-sdk                  passive-library
  PhenixSdk
  phenix_context
  authoring helpers

phenix-plugin-api           runtime-plugin
  phenix.api component
  API convenience services
  phenix SDK contribution

plugin crate
  PluginInstance ABI adapter
  instance settings and state
  typed business logic
```

A plugin may have multiple components. Component callbacks bind SDK clients to the component supplied by the kernel. A legacy service callback must select the component that declares its imports.

## Invariants

- Core owns generic plugin execution and context mechanisms.
- `phenix-sdk` owns default Rust SDK authoring types and activates nothing.
- `phenix-plugin-api` owns optional runtime convenience behavior.
- A plugin instance remains the single owner of mutable instance state.
- A context carries an explicit settings and state view.
- Plugin business logic receives `PluginContext`, not `PluginHost`.
- Typed contract requirements are available through `ctx.sdk`.
- Cross-plugin access requires an explicit typed contract requirement and remains kernel-mediated.
- Static tool and skill declarations may generate those requirements and registration calls without creating hidden provider lookup.
- Stateful SDK objects are handles, not shared provider-internal references.
- Generic kernel mechanisms are available through `ctx.kernel`.
- Call authority and graph generation are available through `ctx.call`.
- Recursive SDK calls cannot expand effective authority.

## Executable listener context

Generated listeners execute with the same scoped host boundary as callable handlers. `EventContext` wraps the listener's `PluginContext` and adds event metadata without granting additional authority.

When an admitted delivery runs, Core constructs a fresh listener `PluginHost` bound to the resolved listener component, owning plugin, pinned graph generation, cancellation scope, and effective listener authority. That authority is the intersection of emitter authority, listener policy, and resolved plugin/component maxima.

The generated listener context therefore exposes ordinary typed imports through `ctx.sdk`, generic kernel mechanisms through `ctx.kernel`, current plugin identity and state through `ctx.plugin`, and call metadata through `ctx.call`. Event metadata remains available through `EventContext`, including emitter, event identity, causality, policy revision, and graph generation.

Listener contract requirements use the same component graph and provider handles as callable handlers. Undeclared requirements fail through normal component-graph checks. Persistence uses the ordinary host capability and resource-ownership checks, so a listener cannot write another plugin's namespace or expand its authority.

Listener task scopes retain the listener owner, pinned graph generation, and attenuated authority. Spawned work receives owned task metadata rather than a borrowed host. Live listener calls receive the normal call-cancellation token. Reconciliation cancels matching old-generation calls and owned tasks when a plugin generation is stopped or replaced.

Host and SDK borrows remain inside one delivery callback. A listener cannot retain a borrowed context across deliveries. Subscription replacement cancels stale queued or in-flight delivery at the event transport boundary, while plugin replacement cancels the matching old-generation call and task scopes.

## Listener invariants

- A generated listener can call a declared typed contract requirement and write its own durable resource.
- Undeclared requirements and foreign namespace writes use the same rejection paths as callable handlers.
- Emitter authority cannot be expanded by listener imports, persistence, or spawned tasks.
- Listener callbacks retain emitter, causality, graph generation, cancellation, and owner identity.
- Listener task scopes retain owner and graph generation and attenuate requested authority.
- Replacement cannot make an old-generation listener context target a new generation.
