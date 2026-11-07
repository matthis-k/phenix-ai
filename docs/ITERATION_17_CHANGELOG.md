# Iteration 17 — type-preserving declaration layouts

## Scope and acceptance targets

- Preserve the **actual** input layout type on the declaration handle after `.input(...)`.
- Keep `inlet()` available for both an inferred atomic input and an explicit `Layout::atomic()` input.
- Prevent a structurally flattened input from manufacturing a nonexistent atomic `inlet()`.
- Replace positional `(input_schema, output_schema, is_local_input, is_local_output)` signatures with a single typed `NodeSignature` carrying `DeclaredValue { schema, local_type }` for each direction.
- Keep all graph execution, plugin ABI, scheduler and low-level node action behavior unchanged.

## Source-exact before / after

From `src/authoring.rs`, before (Iteration 16):

```rust
pub fn input<L>(self, _layout: LayoutMode<A, L>)
    -> Result<NodeHandle<A, O, StructuredInput, D, Mode>, GraphBuildError>
```

After:

```rust
pub fn input<L>(self, _layout: LayoutMode<A, L>)
    -> Result<NodeHandle<A, O, L, D, Mode>, GraphBuildError>
```

The concrete behavior change in `tests/layout_handle_identity.rs`:

```rust
// Previously this configured node lost the `inlet()` method.
let source = graph.node("source", |x: i64| x * 2).unwrap()
    .input(Layout::atomic()).unwrap();
let request = graph.expose_input("request", source.inlet()).unwrap();
```

Before, `NodeDefinition::signature()` returned a four-part tuple and used two boolean flags to encode whether the types were local or portable:

```rust
fn signature() -> (&'static str, &'static str, bool, bool);
```

After, each direction carries its type identity explicitly:

```rust
fn signature() -> NodeSignature;
// NodeSignature { input: DeclaredValue, output: DeclaredValue }
```

## Why this is better

A selected input layout describes the *actual* compile-time port shape; a generic `StructuredInput` marker threw away information already available to Rust. The new handle can offer precise methods without guessing whether a field or an atomic slot exists. Explicit signature fields avoid positional-boolean confusion and make the eventual port codec boundary more auditable. Neither introduces runtime node kinds or scheduler variants.

## Rust tests

Added four integration tests in `tests/layout_handle_identity.rs`:

- `.input(Layout::atomic())` retains `inlet()` and executes through `Deployment`.
- A move-only argument explicitly constructed with `Node::with_layout(..., Layout::atomic())` also exposes `inlet()`.
- Derived field selectors still wire distinct slots after `.input(Layout::fields())`.
- `NodeSignature` records separate local and wire identity correctly.

Added one Rust compile-fail doctest proving a structured declaration has no atomic `inlet()`.

## Deliberately unchanged

- `GraphBuilder::node()` still uses atomic input by default, and its ordinary closure path still requires `Clone`. Those are unresolved Rust ownership/default-layout issues, not solved by this type-state change.
- `Node::with_layout` is still needed for move-only owned arguments. No compatibility or alternative execution implementation was added.
- The low-level `NodeAction::execute -> Vec<Emission>` contract remains a separate conformance migration.

## Verification

The current execution environment has **no `cargo` or `rustc`** and cannot reach the Rust toolchain servers. `git diff --check` and static call-site checks are possible; compiler and test status must remain unverified until executed by CI or a Rust-equipped machine.
