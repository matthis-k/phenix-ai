# Iteration 13 — mandatory typed output and real unit payloads

## Objective

Every successful typed node activation publishes exactly one value. A node
that has no meaningful result returns `()`. A zero-recipient output is not the
same thing as an absent output. No Option-based output schema or emission.

## Before / after (real source excerpts)

### 1. Typed output interface — `crates/phenix-graph/src/lib.rs`

```rust
// BEFORE
pub trait OutputSchema {
    fn output_slot() -> Option<OutputSlot>;
}
pub trait IntoEmission: Sized {
    fn into_emission(self) -> Option<Emission>;
}
impl OutputSchema for () {
    fn output_slot() -> Option<OutputSlot> { None }
}
impl IntoEmission for () {
    fn into_emission(self) -> Option<Emission> { None }
}

// AFTER
pub trait OutputSchema {
    fn output_slot() -> OutputSlot;
}
pub trait IntoEmission: Sized {
    fn into_emission(self) -> Emission;
}
impl OutputSchema for () {
    fn output_slot() -> OutputSlot { OutputSlot::typed::<()>(0).named("value") }
}
impl IntoEmission for () {
    fn into_emission(self) -> Emission { emit(0, ()) }
}
```

**Benefit:** No missing-output state in the typed contract. `()` is a real,
inspectable output and can be consumed by another node. Recipients remain an
orthogonal, zero-or-more selection.

### 2. Typed node lowering — `crates/phenix-graph/src/lib.rs`

```rust
// BEFORE
(self.0)(args, &ctx).map(|output| output.into_emission().into_iter().collect())

// AFTER
(self.0)(args, &ctx).map(|output| vec![output.into_emission()])
```

**Benefit:** The typed return can no longer suppress a publication through
`None`. The temporary `vec!` only bridges the *existing* low-level executor;
it is not a claimed low-level contract migration.

### 3. Real multi-port test semantics — `tests/layout_api.rs`

```rust
// BEFORE
impl phenix_graph::IntoEmission for MaybeOutput {
    fn into_emission(self) -> Option<phenix_graph::Emission> {
        self.0.map(|value| phenix_graph::emit(0, value))
    }
}

// AFTER
impl phenix_graph::IntoEmission for MaybeOutput {
    fn into_emission(self) -> phenix_graph::Emission {
        phenix_graph::emit_local(0, self.0)
    }
}
```

**Benefit:** An ordinary `Option<i32>` is now transported unchanged. `None`
is observable domain data instead of a hidden execution-control instruction.
A preexisting invalid wire-only module export was removed from this test;
it is not required to inspect the node's local output publication.

### 4. Retired outputless action — `src/lib.rs`, integration tests

```rust
// BEFORE
pub struct EmitNothing;
impl NodeAction for EmitNothing {
    fn execute(&self, _ctx: InvocationCtx) -> Result<Vec<Emission>, String> { Ok(vec![]) }
}

// AFTER — actual call site in src/tests.rs
let node = NodeTemplate::with_action(
    1, vec![Port::any(0, vec![InputSlot::new(0, TEXT, BufferMode::Deferred)])],
    vec![OutputSlot::typed::<()>(0)], |_ctx| Ok(vec![emit(0, ())]),
);
```

**Benefit:** No special preset for a supposedly outputless node, and no
second meaning for success in these migrated cases. Existing public negative
compile tests and topology tests now use normal closures.

## New Rust regression tests

`crates/phenix-graph/tests/iteration_thirteen_unit.rs`:

- Ordinary implicit `()` broadcasts to a connected `()` consumer.
- `Output::none(())` still publishes unit but activates no consumers.
- Typed wire `()` output feeds a separate wire `()` consumer through a real
  compiled connection.

The earlier typed single-output test now requires a unit publication and zero
actual deliveries (previously it expected zero publications). The original
multi-port test asserts `Some(42)` and `None` as actual local publications.

## Remaining boundary to migrate

The low-level graph compiler still accepts `Vec<OutputSlot>` and
`NodeAction::execute` still returns `Result<Vec<Emission>, String>`. Some of its
existing conformance fixtures emit multiple distinct values or no values and
need graph topology changes to preserve the tested behavior. **Do not hide this
behind a compatibility shim or delete the behavioral tests.** Migrate those
topologies to shared tuples/structs and ordinary projection nodes, then change
`NodeAction`, `ExecutionSession::complete`, and the atomic publish pipeline in
one coordinated change.

## Verification

- Static scans: no `Option<OutputSlot>` / `Option<Emission>` typed contract
  signatures remain; no active `EmitNothing` references in Rust source.
- Rust tests were added and adjusted, **not executed**: no Cargo/rustc is
  available in the container, and external Rust toolchain downloads cannot
  resolve their hostnames.
- No GitHub branch was modified. This is a standalone source patch.
