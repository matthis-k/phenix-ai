# Iteration 14 — output presence vs recipient selection

## Architecture decision

A typed output port is always declared, but a **successful invocation may produce
no value**. That differs from producing a value and selecting no outgoing edges.
`Option<T>` and `Result<T, E>` remain ordinary **payload types** and are not
implicitly decoded or interpreted by routing.

| Handler return | Value publication | Deliveries |
| --- | --- | --- |
| `value` | One | All compiled outgoing edges |
| `Output::all(value)` | One | All compiled outgoing edges |
| `Output::none()` | **None** | None |
| `Output::to_all([], value)` | One | None |
| `Output::all(None::<i64>)` | One containing `Option::None` | All compiled outgoing edges |

## Actual before / after

```rust
// BEFORE — crates/phenix-graph/tests/iteration_thirteen_unit.rs (Iteration 13)
let source = graph.node("source", |_: i64| Output::none(())).unwrap();

// AFTER — same test: a unit value is produced, but it has no recipients.
let source = graph.node("source", |_: i64| Output::to_all([], ())).unwrap();

// NEW — no value is produced at all; output type is still unit.
let source = graph.node("source", |_: i64| -> Output<()> {
    Output::none()
}).unwrap();

// NEW — type inferred from another branch; no dummy value constructed.
graph.node("source", |x: i64| {
    if x < 0 { Output::none() } else { Output::all(x + 1) }
})?;
```

`Output<T>` is now an enum: `None` or `Some(Produced<T>)`. Its `Some`
case holds one `T` and a selection of compiled outgoing edges. The internal
local return adapter lowers this to **zero or one** `Emission` in the existing
executor. That executor still accepts legacy batches from low-level actions;
it has **not** yet been migrated to one optional emission, and no such claim
should be made.

## Rust inference caveat

`Output::none()` requires the compiler to infer its generic `T` from the
closure return type, another branch, or a consumer. For a completely
unconstrained standalone closure, write `-> Output<()>` or
`Output::<()>::none()`. An output-type annotation is not an extra payload.

## Regression tests

`tests/output_presence.rs` exercises absent value vs undelivered value;
no payload construction on omitted branches; Option::None as ordinary payload;
and unit-valued output type with an absent publication. Existing no-recipient
tests now use `Output::to_all([], value)` so their original semantics remain
covered.

## Verification status

Rust tests were written but not executed because `cargo`/`rustc` are unavailable
in the environment. This release is source-reviewed, not compile-verified.
