# Iteration 15 — typed declaration-owned input layouts

**Baseline:** Iteration 14 Rust-only archive. **Scope:** graph authoring and compilation; no kernel, plugin or ABI changes.

## Goal

Make this a real graph-declaration mutation, not an ephemeral wrapper:

```rust
let add = graph.node("add", |x: AddArgs| x.left + x.right)?
    .input(Layout::fields())?;
```

The registered action and input slots must agree; Rust checks the input type; the compiler checks cross-node connections after all edits and patches.

## Actual before and after

These are source-exact excerpts from `crates/phenix-graph/tests/lean_api.rs`.

```rust
// BEFORE — Iteration 14, special construction helper
let plus = g.node("plus", Node::with_layout(|args: AddArgs| args.left + args.right, Layout::<AddArgs>::fields())).unwrap();

// AFTER — Iteration 15, same ordinary node constructor plus typed layout configuration
let plus = g.node("plus", |args: AddArgs| args.left + args.right).unwrap()
    .input(Layout::fields()).unwrap();

// BEFORE — Iteration 14, specialized constructor for a nested argument
let n = g.node("format", Node::with_layout(|r: Request| {
    format!("{}:{}:{}:{}", r.id, r.address.city, r.address.zip, r.metadata)
}, Layout::<Request>::flatten_to::<2>())).unwrap();

// AFTER — Iteration 15, same construction API at a different depth
let n = g.node("format", |r: Request| {
    format!("{}:{}:{}:{}", r.id, r.address.city, r.address.zip, r.metadata)
}).unwrap().input(Layout::flatten_to::<2>()).unwrap();
```

### Endpoint accessor (intentional breaking change)

```rust
// BEFORE: node.input() addressed the sole atomic input slot
let request = graph.expose_input("request", node.input())?;

// AFTER: reserve .input(layout) for layout configuration
let request = graph.expose_input("request", node.inlet())?;
```

Only typed **node** handles change. `ModuleHandle::input("name")` retains its existing explicit name-based API.

## Implementation ownership

- `LayoutMode<A, L>` carries the function argument type **A** directly; `NodeHandle<A, ...>::input` requires `LayoutMode<A, L>` plus `L: LocalInputLayout<Value=A>`. Incompatible layouts fail at Rust type checking, with an added `compile_fail` doctest.
- `GraphBuilder::node` retains the handler in `Arc<D>` and the canonical registered `NodeTemplate` in a declaration-owned pending registry. An `.input(...)` call builds a *new validated action adapter and slots* against the same function; it does not merely edit a port shape while leaving its decoder behind.
- Builder verification reads the pending template so subsequent `connect` and `expose_input` operations see the effective new layout. `GraphBuilder::module` materializes all changes before compilation.
- Changes made after connection can invalidate a previously valid endpoint; final graph compilation rejects it. This is intentional: graph compilation is the structural authority.
- Cloned builders snapshot the pending registry instead of sharing it; patch transactions remain isolated. `GraphAssembly::contribute` materializes staged layouts before change tracking.
- Successfully replacing a node revokes prior configuration handles for that node; failed replacement does not. Once a builder is consumed, `Weak` configuration handles cannot mutate the compiled graph.

## Rust coverage added

`tests/declaration_layout.rs` includes:
1. Inferred `Layout::fields()` with a derived struct and a real engine invocation.
2. Unconfigured atomic default.
3. Compilation rejection when a late layout edit invalidates an existing export.
4. Typed connection to a newly configured field.
5. Snapshot isolation between `GraphBuilder` clones.
6. Rejection of layout updates after graph consumption.
7. Transactionality after a failed node replacement.
8. Revocation of stale handles following a successful replacement.

Plus a `compile_fail` doctest for mismatched layout type. No Python tests or additional scheduling mechanisms.

## Remaining friction and limits

1. **Rust/Cargo unavailable here.** No compile, formatting or runtime test was executed. This package is a source-level implementation candidate until `cargo test` is green.
2. Default node layout remains **atomic**. Deriving fields does not automatically choose it; explicit `input(Layout::fields())` is supported. Choosing a per-type default without colliding blanket traits needs further Rust API work.
3. Ordinary unary closures still require `A: Clone` because the default local atomic action supports shared fan-out by cloning when necessary. Move-only structured argument tests continue to use the existing `Node::with_layout` helper. That helper is **not yet removed**.
4. Only local layout reconfiguration is supported by the new `.input` method. Wire/codec configuration remains a separate upcoming port concern.
5. A small authoring-only pending-template registry now exists; it does not survive into the compiled runtime graph. It should be reevaluated after Cargo verification for unnecessary duplication/allocation.

## Required verification

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo test -p phenix-graph --no-default-features
cargo clippy --workspace --all-targets -- -D warnings
```

No kernel/plugin integration has been attempted.
