# Iteration 8 — `Output<T>` is *payload + recipients*, never a data interpreter

## Architectural boundary

- A normal `Fn(A) -> T` publishes `T` and broadcasts it to all connected edges.
- A directed `Fn(A) -> Output<T>` publishes **the same T** and applies an
  explicit selection (`all`, `none`, `to`, `to_all`, `to_prefix`, `to_filter`).
- `Option<T>`, `Result<T,E>`, and custom enums stay ordinary data in either path.
  Nothing in the output layer unwraps or interprets them.
- `Output::none(value)` still publishes the value, but commits zero deliveries.
- Node action errors (`Result<Vec<Emission>, String>`) remain the only error
  mechanism on this path. A domain error payload must be handled by the next
  node or by domain code, not automatically converted into a failed wave.
- Selections can only refer to the executing output's compiled outgoing edges;
  this iteration does not change or weaken that validation.

## Source-exact before/after

Before: `crates/phenix-graph/tests/iteration_two.rs` (Iteration 7):

```rust
// BEFORE
let optional = graph.node("optional", |v: i64| Output::option((v >= 0).then_some(v))).unwrap();
let fallible = graph.node("fallible", Node::new(|v: i64| Output::result(
    if v < 0 { Err("negative value") } else { Ok(v * 2) }
))).unwrap();
```

After, same file:

```rust
// AFTER
let optional = graph.node("optional", |v: i64| Output::all((v >= 0).then_some(v))).unwrap();
let fallible = graph.node("fallible", Node::new(|v: i64| Output::all(
    if v < 0 { Err("negative value") } else { Ok(v * 2) }
))).unwrap();
```

The `Err` value is now published as `Result<i64, &str>` and does **not** mark
its wave failed. The tests assert that exact payload, including `None`, across
successive waves.

Before: `crates/phenix-graph/tests/output_wrapper.rs`:

```rust
// BEFORE
if x > 0 { Output::all(x) } else { Output::none() }
```

After, same file:

```rust
// AFTER
if x > 0 { Output::all(x) } else { Output::none(x) }
```

The `none` branch now **publishes x but sends it to no connected recipient**.
The test checks both facts independently.

## Changes

- Removed the internal `OutputState::{Emit, Skip, Fail}` state machine.
- Removed `Output::option` and `Output::result` methods, without aliases.
- Changed `Output::none()` to `Output::none(value)`.
- Simplified `Output::into_emission` to produce exactly one `Emission` rather
  than `Result<Option<Emission>, String>`.
- Simplified the directed local node-action adapter accordingly.
- Migrated call sites in Rust tests, README and contributor examples.
- Replaced the old output semantic tests with Rust tests for `Option::None`,
  `Result::Err`, explicit broadcast, typed destination routing with a Result
  payload, zero-target publications, and real action failures/recovery.

## Tradeoffs

`Output::none(value)` means **no deliveries**, not **no publication**. This is
intentional: the payload can remain visible for inspection and in-memory
retention, even if downstream nodes are not activated.

The engine still has older low-level `NodeTemplate::function_optional` and
`function_fallible` helpers for *wire* node-return interpretation. They do not
run through `Output<T>` and are a separate authoring-API consolidation item.
The current `Branch` and graph-owned output routing policy are also pending
cleanup; no new parallel scheduler was introduced.

## Verification status

Rust tests have been written/updated, but this environment has no `cargo` or
`rustc`. Static checks and archive validation are not substitutes for a build
or execution of the Rust suite.

```sh
cargo fmt --all --check
cargo test --workspace --all-targets --all-features
cargo test -p phenix-graph --no-default-features
```
