> Historical iteration record. The API described here was superseded by
> `ITERATION_8_CHANGELOG.md`; these examples are not current usage.

# Iteration 6 — One-value output decisions

**Scope:** Standalone graph only. Native kernel/plugin integration remains last.
**Verification:** 14 new Rust integration tests (plus one compile-fail doctest); `cargo` and `rustc` are not installed in this environment. Source and ZIP checks are *not* Rust compilation.

## Concrete API targets

| Responsibility | Acceptance rule | Implemented in source |
|---|---|---|
| Default behavior | Plain `Fn(A)->T` broadcasts unchanged | Yes |
| Output choice | One optional `Output<T>` return wrapper, no per-target payloads | Yes |
| One destination | `Output::to(&input, value)` | Yes |
| Selected destinations | `Output::to_all([&a, &b], value)` | Yes |
| Prefix | `Output::to_prefix("workers", value)` matches node-path components | Yes |
| General filter | `Output::to_filter(predicate, value)` examines only outgoing edges | Yes |
| Suppression | `Output::none()` emits no payload | Yes |
| Ordinary `Option/Result` | Preserved as data, not auto-unwrapped | Yes |
| Connectivity | Targets must be reachable through *existing outgoing edges* | Validated at execution |
| Scope | Foreign builder's input handle must be rejected | Validated at execution |
| Atomicity | Invalid route/predicate failure commits no deliveries | Uses existing atomic batch |
| Verification | `cargo test --all-targets --all-features` | **Not run** |

## Before and after — source-exact examples

From Iteration 5 `tests/iteration_five.rs`:

```rust
// BEFORE — routing configured separately from the producer function
let source = graph.node("source", |x: i64| x + 1).unwrap();
graph.route(source.output(), |_, edges: &[EdgeId]| Ok(vec![edges[0]])).unwrap();
```

From Iteration 6 `tests/output_wrapper.rs`:

```rust
// AFTER — the calculation and its destination decision stay together
let source = g.node("source", Node::new(move |x: i64| {
    let score = x > 0; // Intermediate decision isn't part of returned data.
    let value = x.abs();
    Output::to(if score { &l } else { &r }, value)
})).unwrap();
```

From Iteration 6 `tests/output_wrapper.rs`, another real call site:

```rust
// AFTER — prefix and filter, without a separate node class
let source = g.node("source", |x: i64| Output::to_prefix("workers", x)).unwrap();

let source = g.node("source", |x: i64| {
    let chosen = if x % 2 == 0 { "left" } else { "right" };
    let result = x * 10;
    Output::to_filter(move |destination| destination.path() == chosen, result)
}).unwrap();
```

## Architectural impact

* Output-return wrappers are consumed by ordinary node actions before graph delivery.
  Neither a second scheduler nor runtime node-kind enum was introduced.
* Graph-authoring output handles were renamed from `Output<T>` to `OutputRef<T>`;
  `Output<T>` now exclusively means the one-value return wrapper.
* Output decisions are evaluated against the compiled output's existing edge list.
  Unconnected named targets and foreign authoring scopes fail closed.
* A filter sees stable **qualified node paths**, port/slot names and the candidate
  edge ID. `to_prefix("workers")` matches `workers` and `workers/process`,
  never `workers-old`.
* A node returning `Option<T>` or `Result<T,E>` still broadcasts exactly that
  type unless it explicitly returns an `Output<T>` wrapper.

## Remaining friction / explicitly not complete

1. The older port-owned `OutputRouting::Select` plus `GraphBuilder::route` and
   `PatchBuilder::route` still exist for existing low-level conformance tests
   and patch-policy tests. They use the **same executor delivery path**, but
   they are an overlapping public configuration mechanism. Remove them after
   rewriting their tests and preserving patch-edit conflict semantics.
2. `Node::optional`, `Node::fallible`, and `Node::branch` helpers still exist.
   They should be removed through a test-preserving migration to plain values,
   explicit error interpretation and the new one-value output model.
3. Generic `NodeDefinition` signature-marker inference needs actual Rust
   compilation before this API can be considered stable. No ratings are promoted.
4. `Output<T>` is currently local-value-only; wire/ABI integration is deferred.
5. Filtering closures are required to be pure/deterministic by contract;
   the runtime can enforce connectivity and atomicity, not user side effects.

## Run verification

```sh
cargo test --workspace --all-targets --all-features
cargo test -p phenix-graph --no-default-features
cargo run -p xtask -- verify
```
