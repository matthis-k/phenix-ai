# Plugin call binding

status: specification-only

## Purpose

Give plugin authors one typed call API while keeping target binding explicit at the call site.

A plugin may know a callable contract at compile time even when its provider is loaded, replaced, or unloaded at runtime. Phenix therefore separates contract declaration, contract requirement, target binding, and value representation.

The Rust authoring vocabulary is contract-first. A plugin declares a contract with `#[phenix_sdk::contract(...)]`, requires it with `#[phenix(require)]`, and provides it with `#[phenix(provide(...))]`. These spellings lower to the existing Core Interface, Import, and Export representation.

## User model

Generated plugin context exposes statically known contracts as typed members:

```rust
ctx.sessions.new(request)?;
ctx.sessions.new::<Runtime>(request)?;
ctx.sessions.new::<CompileTime>(request)?;
```

`Runtime` is the default binding mode.

The mode belongs to one invocation edge. A plugin may mix runtime and compile-time calls in the same method.

## Static context members

`ctx.x` exists when the caller artifact was compiled with the contract for `x`.

The member represents the contract. It does not represent a specific provider instance.

A caller may therefore contain `ctx.sessions` before any Sessions provider is active. A later Graph Generation may add, replace, or remove the provider without changing the caller's Rust type.

A normal runtime call resolves through the active Graph Generation:

```text
caller -> resolved callable edge -> Layer -> Layer -> terminal provider
```

If no provider is available, the call returns the canonical unavailable or resolution error. The client must not retain a stale provider pointer across Graph Generations.

## Runtime binding

`Runtime` means the callable contract is known to the caller but the implementation is selected by the active Graph Generation.

Runtime calls preserve canonical kernel behavior:

- provider selection;
- Layer interposition;
- authority attenuation;
- cancellation;
- provenance;
- schema compatibility checks;
- Graph Generation pinning for an invocation already in progress.

A runtime call started under one Graph Generation stays pinned to that generation. A later call uses the then-active generation.

Loading or unloading a provider therefore affects future calls without changing existing typed clients.

## Compile-time binding

`CompileTime` means the concrete target implementation is known when the caller artifact is compiled.

The compiler must reject `CompileTime` when it cannot name a concrete target implementation.

Valid cases include:

- a method or component in the same plugin artifact;
- a statically linked concrete plugin dependency;
- another target whose lifecycle is coupled to the caller artifact.

An independently unloadable plugin cannot satisfy a compile-time edge for a caller that remains active after that plugin is removed.

The target lifetime must be at least as strong as the caller artifact lifetime. Phenix may unload the whole static closure together, but it must not expose a surviving caller whose compile-time target has disappeared.

Compile-time calls use the concrete target selected at compile time. They retain the same Layer interposition, authority attenuation, cancellation, provenance, and value conversion rules as runtime calls.

## Value representation

Binding mode and value representation are independent.

All of these forms are valid when the target and conversion contracts exist:

```rust
ctx.sessions.new::<Runtime>(typed_request)?;
ctx.sessions.new::<Runtime>(phenix_value)?;
ctx.sessions.new::<CompileTime>(typed_request)?;
ctx.sessions.new::<CompileTime>(phenix_value)?;
```

A `PhenixValue` input makes request conversion dynamic. It does not make target binding dynamic.

For a compile-time call with `PhenixValue`, Phenix converts the value to the statically known request type before invoking the statically known target.

## Runtime-discovered members

Rust cannot add a new field to an already compiled context. A capability that was not known when the caller artifact was compiled cannot later appear as `ctx.x`.

Phenix therefore provides dynamic lookup for runtime-discovered capabilities.

When the contract type is known but the binding name or instance is discovered at runtime:

```rust
let sessions = ctx.get::<Sessions>(binding_id)?;
sessions.new(request)?;
```

The returned typed handle represents a logical runtime binding. It must resolve through the active Graph Generation on each new invocation and must not retain a provider implementation pointer.

When the contract itself is not known to the compiled caller:

```rust
let capability = ctx.get(binding_id)?;
capability.call(callable_id, value)?;
```

A direct dynamic call form may be provided as equivalent sugar:

```rust
ctx.call(callable_id, value)?;
```

Dynamic lookup cannot enable `CompileTime`. A compile-time target must already be representable in the caller's Rust types.

## Callable identity

An exposed plugin function may optionally declare a stable global callable identity.

```rust
#[phenix(expose(id = "sessions.new@1"))]
fn new(&mut self, request: NewSession) -> Session
```

Global identity is optional. A statically generated member such as `ctx.sessions.new(...)` does not need to consume the global namespace unless late lookup or external reference is required.

Typed generated calls and dynamic calls to the same global callable must lower to the same runtime callable edge when using `Runtime`.

## Generated API

A generated contract client should expose one method name with a per-call binding mode:

```rust
ctx.sessions.new(request)?; // Runtime by default
ctx.sessions.new::<Runtime>(request)?;
ctx.sessions.new::<CompileTime>(request)?;
```

Conceptually each generated callable has a marker that owns its contract:

```rust
trait Callable {
    const ID: Option<&'static str>;
    type Request: HasPhenixSchema;
    type Response: HasPhenixSchema;
}
```

The generated method delegates to a binding-mode implementation. `CompileTime` is only implemented when the generator can prove a concrete target type.

`Runtime` is available for a declared runtime import and resolves through the kernel.

## Plugin declarations

Concrete plugin dependencies and contract requirements remain distinct.

A contract requirement gives the caller a typed runtime member but does not imply a compile-time target:

```rust
#[phenix(require)]
sessions: Required<Sessions>
```

The requirement lowers to the ordinary Core Interface Import and leaves provider selection to the active Graph Generation.

A concrete static dependency may enable compile-time calls when the dependency is part of the caller's compiled and lifecycle-coupled closure:

```rust
#[phenix(dep)]
sessions: sessions::Plugin
```

The compiler must not silently fall back from `CompileTime` to `Runtime`.

## Loading and unloading

Runtime-loaded callers may use both modes.

Loading time does not determine binding mode. A plugin artifact loaded later can make compile-time calls to implementations compiled into its own static closure and runtime calls to capabilities resolved from the host Graph Generation.

Provider changes have these effects:

| Change | Runtime call | Compile-time call |
| --- | --- | --- |
| Provider loaded later | Future calls may resolve to it | No effect |
| Provider replaced | Future calls use new resolution | No effect |
| Provider unloaded | Future calls fail or resolve elsewhere | Target may not be independently unloadable |
| Layer added or removed | Future calls use new chain | Future calls use the new chain |
| Graph Generation changes during call | Started call remains pinned | No graph dependency |

## Dynamic handle lifetime

A typed handle returned by `ctx.get::<T>(...)` carries logical identity, caller identity, and contract identity. It does not own the provider.

If the provider is replaced after the handle is obtained, a later call through the handle resolves against the new active Graph Generation.

If no compatible provider remains, the later call fails canonically.

An invocation already started before replacement remains pinned to its starting Graph Generation.

## Failure rules

- Missing required runtime provider fails graph construction when the contract requirement is required by the active composition.
- Missing optional runtime provider produces the existing optional or unavailable behavior when invoked.
- Dynamic lookup of an unknown binding returns a lookup error.
- Dynamic lookup with a known contract but incompatible schema returns a typed compatibility error.
- `PhenixValue` conversion failure is a value/schema error and does not trigger provider fallback.
- Compile-time calls do not fall back to runtime resolution.
- Runtime execution failure does not silently switch provider inside an already-started invocation.

## Invariants

- Contract typing, target binding, and value representation are separate concerns.
- `Runtime` is the default call mode.
- Binding mode is selected per invocation edge.
- `ctx.x` is generated only for contracts known when the caller artifact is compiled.
- Runtime clients represent contracts and logical bindings, never durable provider pointers.
- Runtime calls use canonical Graph Generation resolution and Layer interposition.
- Compile-time calls retain canonical Layer interposition.
- Compile-time calls require a concrete target type known to the compiler.
- Compile-time targets cannot disappear independently while the caller survives.
- `PhenixValue` is valid with either binding mode when conversion is defined.
- Runtime-discovered contracts use `ctx.get(...)` or dynamic `ctx.call(...)`.
- Author-facing `require` and `provide` lower to the canonical Core Import and Export model rather than creating another binding path.
- Dynamic lookup cannot manufacture compile-time binding.
- Global callable identity is optional and is required only for late or external lookup.
