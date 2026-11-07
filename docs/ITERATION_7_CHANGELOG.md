> Historical iteration record. The API described here was superseded by
> `ITERATION_8_CHANGELOG.md`; these examples are not current usage.

# Iteration 7 — Consolidate return semantics into `Output<T>`

**Scope:** standalone graph core + Rust tests. No kernel, ABI, frontend or plugin changes.
**Baseline:** full Iteration 6 workspace, unchanged before this batch.
**Verification status:** static source audit and `git diff --check` only. Rust/Cargo are not installed in this environment; **none of the new tests has been run**.

## Concrete acceptance targets

| Requirement | Change | Acceptance evidence |
|---|---|---|
| One generic local node construction | Remove `Node::optional` and `Node::fallible` and their authoring wrappers | No remaining active call sites; existing Rust tests migrated |
| Ordinary value is data | Preserve `Fn(A) -> Option<T>` and `Fn(A) -> Result<T,E>` as ordinary broadcast values | `tests/output_semantics.rs` (new, unexecuted) |
| Explicit skip or failure | Add `Output::option` and `Output::result`; both use the existing directed node action | Rust success/failure/recovery tests (new, unexecuted) |
| One-value output semantics | Return at most one emission, no per-destination payload | `OutputState::{Emit, Skip, Fail}` lowers to the existing `Emission`/`NodeAction` path |
| Prevent silent discarded dispatch decisions | `#[must_use]` on `Output<T>` | Rust compile-fail doctest (new, unexecuted) |
| Validate selected edges efficiently | Build one output-scoped edge-ID index instead of repeated linear searches | Runtime implementation; performance not benchmarked |
| No regression to specialized local node types | Extend Rust `xtask` scope audit | Rust negative audit test (new, unexecuted) |

## Before / after — excerpts from *real* test call sites

`crates/phenix-graph/tests/iteration_two.rs`:

```rust
// BEFORE — Iteration 6
let optional = graph.node("optional", Node::optional(|v: i64| (v >= 0).then_some(v))).unwrap();
let fallible = graph.node("fallible", Node::fallible(
    |v: i64| -> Result<i64, &'static str> {
        if v < 0 { Err("negative value") } else { Ok(v * 2) }
    },
)).unwrap();

// AFTER — Iteration 7
let optional = graph.node("optional", |v: i64| Output::option((v >= 0).then_some(v))).unwrap();
let fallible = graph.node("fallible", Node::new(|v: i64| Output::result(
    if v < 0 { Err("negative value") } else { Ok(v * 2) }
))).unwrap();
```

`crates/phenix-graph/tests/lean_api.rs`:

```rust
// BEFORE — Iteration 6
let option = g.node("optional", Node::optional(|x: i32| (x > 0).then_some(x))).unwrap();
let fallible = g.node("fallible", Node::fallible(|x: i32| -> Result<i32, &'static str> {
    if x > 0 { Ok(x) } else { Err("negative") }
})).unwrap();

// AFTER — Iteration 7
let option = g.node("optional", |x: i32| Output::option((x > 0).then_some(x))).unwrap();
let fallible = g.node("fallible", |x: i32| Output::result(
    if x > 0 { Ok(x) } else { Err("negative") }
)).unwrap();
```

These aren't conceptual alternatives; both pairs were extracted from the source at their respective revisions.

## What this actually buys

1. **Consistent contract:** the meaning of `Option<T>`/`Result<T,E>` no longer changes because a different node constructor was chosen. Both are ordinary domain values unless explicitly interpreted through `Output<T>`.
2. **Less public API and runtime plumbing:** removed two public wrapper types, two `Node::...` helpers, two `NodeDefinition` implementations, two local action adapters and two `NodeTemplate` constructors. Existing normal broadcast and targeted routing still use the same executor.
3. **No extra output shape:** `Output<T>` now privately holds `Emit(T)`, `Skip`, or `Fail(String)`. The old redundant `OutputSelection::None` case is removed; suppression creates no `Emission`, rather than emitting one that selects zero edges.
4. **Safer control syntax:** `Output` is `#[must_use]`. Accidentally constructing `Output::to(...)` without returning it produces a warning, rather than silently doing nothing unnoticed. Strict-warning builds reject it.
5. **Fan-out lookup:** selected edges are validated using an output-local `BTreeMap<EdgeId, &Edge>` rather than repeated linear scans. This changes membership/address lookup from linear to logarithmic per selection, at the cost of building an index for each publication. No performance gain is claimed without benchmarking.

## Semantics that deliberately remain distinct

- `return Some(value)` broadcasts the `Option<T>` payload.
- `return Output::option(Some(value))` emits `T`.
- `return Output::option(None)` emits nothing (successful invocation).
- `return Err(error)` broadcasts the `Result<T,E>` payload **when the declared output is that Result**.
- `return Output::result(Err(error))` fails the node and emits nothing.
- `return Output::to(target, value)` still selects only an existing connected edge. Other output constructors and compilation safeguards are unchanged.

Wire-specific `NodeTemplate::function_optional`/`function_fallible` are **not** migrated: the current `Output<T>` converts to local values, not portable encoded contract values. Removing these without designing the transport boundary would be a functionality regression.

## Remaining highest-value issues

1. **Eliminate overlapping graph-level selectors:** `GraphBuilder::route`, `PatchBuilder::route`, `OutputSlot::select` and `OutputRouting::Select` remain beside the returned `Output<T>` policy. Rewrite their existing conformance tests with explicit return decisions or first-class node replacements, then remove this second routing surface. Preserve patch conflict guarantees.
2. **Conditional heterogeneous output types:** `Node::branch` still creates two named output slots and two-output action plumbing. Evaluate using a shared tuple/struct output and downstream projections before deleting heterogeneous branches. Do not eliminate it without preserving the real use cases.
3. **Wire output boundary:** unify `ContractValue`/codec-based return handling with the one-output model before reopening native integration.
4. **Rust toolchain gate:** run formatting, compile, all features, no-default-features and targeted negative tests. The API and runtime tests cannot earn a verified rating before this gate passes.

## Required Rust verification

```sh
cargo run -p xtask -- verify
cargo test -p phenix-graph --test output_semantics
cargo test -p phenix-graph --all-features --doc
```

No Python tests or external scripts have been added. No GitHub branch has been modified.
