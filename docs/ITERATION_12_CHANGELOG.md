# Phenix graph — Iteration 12: singular typed output contract

## Scope and acceptance targets

Starting point: Iteration 11, not the GitHub kernel. This is a **bounded step** in the unfinished transition from `Vec<Emission>` to one payload per invocation. No second engine, compatibility facade, Python test suite, or production connector has been added.

1. A typed handler's declaration describes **zero or one output port**, not an arbitrary list.
2. A typed handler's conversion produces **zero or one emission**, not a vector.
3. No output decoding of `Option<T>` or `Result<T,E>` by `Output<T>`; these remain ordinary payload types.
4. A real multi-output integration fixture should be rewritten as **one source payload plus an ordinary projection node**, preserving its expected result and the manual-versus-macro comparison.
5. A structured local-input handler should be able to return `Output<T>` and select destinations using the **same runtime adapter** as its ordinary return.
6. Cover both typed one/zero publication behavior and conditional routing with Rust integration tests.

## Actual before / after (from the modified files)

### Typed output contract — `src/lib.rs`

```rust
// BEFORE
pub trait OutputSchema {
    fn output_slots() -> Vec<OutputSlot>;
}
pub trait IntoEmissions: Sized {
    fn into_emissions(self) -> Vec<Emission>;
}

// AFTER
pub trait OutputSchema {
    fn output_slot() -> Option<OutputSlot>;
}
pub trait IntoEmission: Sized {
    fn into_emission(self) -> Option<Emission>;
}
```

**Benefit:** a typed output cannot advertise or produce multiple payloads through this contract. The typed node's existing action lowers its optional emission into the current executor representation; the executor itself is not yet migrated.

### Multi-output fixture — `tests/graph_api.rs`

```rust
// BEFORE
vec![OutputSlot::typed::<i64>(0).named("left"), OutputSlot::typed::<i64>(1).named("right")],
|ctx: InvocationCtx| {
    let value = ctx.arg::<i64>(0)?;
    Ok(vec![emit(0, value), emit(1, value * 2)])
},

// AFTER
vec![OutputSlot::typed::<i64>(0).named("value")],
|ctx: InvocationCtx| Ok(vec![emit(0, ctx.arg::<i64>(0)?)]),

// Additional ordinary node, connected to the same source output:
let double = NodeTemplate::new(
    3,
    vec![Port::all(0, vec![InputSlot::immediate::<i64>(0).named("value")]).named("run")],
    vec![OutputSlot::typed::<i64>(0).named("value")],
    |ctx: InvocationCtx| Ok(vec![emit(0, ctx.arg::<i64>(0)? * 2)]),
).named("double").unwrap();
```

**Benefit:** the right consumer obtains the projected value through a normal node; the producer emits the same value to both branches. Original `6 * value` assertion and both manual/macro wiring variants remain.

### Structured local layout — `src/lib.rs`

```rust
// BEFORE
F: Fn(L::Value) -> O + Send + Sync,
// ...
Ok(vec![emit_local(0, (self.handler)(args))])

// AFTER
R: LocalReturn<O>,
F: Fn(L::Value) -> R + Send + Sync,
// ...
Ok(vec![(self.handler)(args).into_local_emission()])
```

**Benefit:** a structured input handler may return a plain `T` or `Output<T>`; a new node kind or second routing policy is unnecessary. `NodeDefinition` has the same two signature markers used by the atomic-input path.

## Rust regression coverage added

- `typed_action_declares_one_output_and_publishes_one_payload`
- `typed_action_can_have_no_output_without_an_output_variant`
- `structured_input_can_route_one_payload_without_a_special_node_kind`

The existing `manual_builder_derive_and_wiring_macro_have_identical_execution` test now covers the projection-node topology as well as the same `6*x` outcome.

## Unresolved — do not overstate migration status

**The kernel-facing low-level contract is still `NodeAction::execute(...) -> Result<Vec<Emission>, String>`.** Conformance tests and some independent low-level actions still deliberately emit multiple payloads. Those topologies must be rewritten and tested before replacing the remaining vector with an optional or mandatory single emission. This iteration does not claim to finish that cutover. Existing multi-output conformance tests are still active; none were hidden or deleted.

Once low-level migrations are complete, simplify `NodeAction`, `Executor`, completion tickets, `WaveState::publish_via`, and the old `IntoEmission` bridging collection together. Move outward to patch ownership and deployment only afterward.

## Verification status

Archive/file integrity and static API/call-site checks are available. **Cargo/rustc are absent** from this environment, so format, compilation, and actual Rust test passes are not verified. New tests are proposed executable Rust code, not observed green results. No GitHub branch was modified.
