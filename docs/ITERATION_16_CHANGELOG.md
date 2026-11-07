# Iteration 16 — explicit shared inputs, one node declaration API

## Acceptance target

Remove the `Node::shared` special case. A shared value is an **explicit `Arc<T>` domain input**, not a separate node kind. Preserve fan-out by sharing the *same allocation* rather than cloning a non-`Clone` domain value. Migrate applicable real layout call sites to `.input(Layout::...)` without removing conformance scenarios.

## Source-exact before/after

From `crates/phenix-graph/tests/lean_api.rs` before this iteration:

```rust
// BEFORE (Iteration 15): a special node wrapper changes the input contract.
let source = g.node("source", Node::shared(|data: Arc<NonClone>| NonClone { value: data.value + 1 })).unwrap();
let second = g.node("second", Node::shared(|value: Arc<NonClone>| value.value * 3)).unwrap();
// Trigger payload was Value::local(NonClone { value: 4 }).
```

After this iteration:

```rust
// AFTER: the ordinary node type; ownership is explicit in the payload type.
let source = g.node("source", |data: Arc<NonClone>| Arc::new(NonClone { value: data.value + 1 })).unwrap();
let second = g.node("second", |value: Arc<NonClone>| value.value * 3).unwrap();
// Trigger payload is Value::local(Arc::new(NonClone { value: 4 })).
```

From `tests/regressions_9.rs`:

```rust
// BEFORE: layout constructor specialized the node.
let n = g.node("read", Node::with_layout(|x: L1| x.inner.inner.inner.inner.inner.inner.inner.inner.value, Layout::<L1>::deep())).unwrap();
// AFTER: configure the pending declaration through the same typed handle.
let n = g.node("read", |x: L1| x.inner.inner.inner.inner.inner.inner.inner.inner.value).unwrap().input(Layout::deep()).unwrap();
```

## Responsibility effects

- `Node::shared`, its authoring wrapper, the shared action adapter, and `NodeTemplate::shared_function` were removed, not deprecated. The ordinary `NodeDefinition` and `LocalFunctionNodeAction` now handle explicit `Arc<T>` values.
- Existing `Node::with_layout` callers with `Clone` input were migrated where possible. Its genuinely move-only and patch-contribution call sites remain; this iteration does **not** claim they are gone.
- `xtask` now flags reintroducing a shared node category.
- New Rust integration tests cover `Arc<NonClone>` fan-out preserving pointer identity and normal single-consumer use.

## Remaining architectural constraints — not completed

1. The **default** layout of bare `graph.node("name", |x: T| ...)` is still **atomic**. Rust does not support a general overlapping blanket fallback plus a derived-fields specialization on stable Rust; supporting derived fields by default requires a coherent `DefaultInputLayout` trait policy, changes to existing authoring bounds, and migration of user-defined types.
2. Bare `graph.node` still requires `A: Clone` for by-value inputs. The existing action consumes a unique `A` or clones it if a fan-out delivery shares the allocation. Move-only **owned** inputs currently require `Node::with_layout(..., Layout::atomic())`, whereas shared non-Clone values work through explicit `Arc<T>` with normal `graph.node`. These are different semantics: moving `T` through multiple consumers cannot be guaranteed without cloning or a sharing strategy.
3. Full Rust compiler and test verification remains pending: neither `cargo` nor `rustc` is installed in this environment. Keep the changed source as a candidate until CI runs.

## Verification commands

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --all-features --all-targets
cargo test -p phenix-graph --no-default-features
cargo run -p xtask -- audit
```
