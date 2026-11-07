# Iteration 11 — fallible input layouts and single declaration ownership

**Base:** Iteration 10 Rust-only graph workspace. **Scope:** graph declaration and input layout; no kernel/plugin integration.

## Concrete review findings

1. `PortLayoutAt<0>`, tuple layouts and derive-generated layouts used `u16::try_from(...).expect(...)`, while deep tuples used `slots.len() as u16`. An oversized declaration could either panic or silently wrap IDs.
2. `InputLayout::slots() -> Vec<InputSlot>` provided no way to propagate layout failures through the ordinary `GraphBuilder::node` path.
3. Local typed nodes evaluated `L::slots()` at construction **and again on every invocation**. A user-provided layout can be expensive or depend on captured state; the runtime should use the already validated port schema.
4. One existing manual layout test defined `prepare` inside `impl InputLayout` even though it belongs to `DecodedInputLayout`.

## Code changes

- `LayoutError::TooManySlots { path, capacity }` and one checked slot allocator (`push_layout_slot`) replace panics/wrapping in all built-in flattened tuple and derived local/wire layouts.
- `InputSchema`, `PortLayout`, `LocalPortLayout`, bounded variants, and `InputLayout` return structured declaration errors; the derive macro emits the same fallible contract as hand-written implementations.
- `Port::for_layout`, `TypedPort::new`, and layout-dependent `NodeTemplate` constructors propagate errors. `GraphBuilder::node` now returns `GraphBuildError` with distinct `Name` and `Layout` cases, and computes the entire template *before* registration.
- Simple `NodeTemplate::function` stays **infallible**, as its atomic input has exactly one slot; no extra `unwrap()` was introduced for ordinary function declarations.
- `LocalPreparedFunctionNodeAction` stores the slot schema created during registration; subsequent invocations never reconstruct it.
- Updated existing Rust integrations, benchmarks, derive outputs and manually implemented traits; added seven Rust regression tests.

## Exact before → after examples

From `crates/phenix-graph/src/lib.rs`:

```rust
// BEFORE — bounded local layout could abort the process
let id = u16::try_from(slots.len()).expect("too many local tuple slots");
slots.push(InputSlot::local::<$name>(id).named(path));

// AFTER — one checked allocator and typed failure
push_layout_slot(slots, path, InputSlot::local::<$name>)?;
```

From `crates/phenix-graph/src/lib.rs`:

```rust
// BEFORE — local layout was rebuilt on every execution
let mut owned = OwnedSelectedValues::new(raw, L::slots());

// AFTER — the schema is frozen at node declaration
let mut owned = OwnedSelectedValues::new(raw, self.slots.clone());
```

From `crates/phenix-graph/tests/manual_api.rs`:

```rust
// BEFORE — no error channel, and prepare incorrectly mixed into InputLayout
impl InputLayout for RenamedValue {
    type Value = i64;
    fn slots() -> Vec<InputSlot> { vec![InputSlot::deferred::<i64>(0).named("custom.path")] }
    fn prepare(values: &SelectedValues<'_>) -> Result<Self::Value, String> {
        values.get_path("custom.path")
    }
}

// AFTER — separate declaration and decoding responsibilities
impl InputLayout for RenamedValue {
    type Value = i64;
    fn slots() -> Result<Vec<InputSlot>, LayoutError> {
        Ok(vec![InputSlot::deferred::<i64>(0).named("custom.path")])
    }
}
impl DecodedInputLayout for RenamedValue {
    fn prepare(values: &SelectedValues<'_>) -> Result<Self::Value, String> {
        values.get_path("custom.path")
    }
}
```

## Behavioral acceptance tests (Rust)

`tests/iteration_eleven_layout.rs` adds tests for the maximum representable slot ID, checked overflow, failure propagation through a layout, non-mutating graph registration on failure, derived wire and local layout expansion, manual `InputSchema` errors, and freezing the declared layout across repeated waves.

## Open frontier — **not yet complete**

The lower-level `NodeAction::execute` contract is still `Result<Vec<Emission>, String>`. It is inconsistent with the public single-payload rule. Existing low-level conformance graphs contain multiple, *differently typed* emissions in one activation, including error/failure cases and cohort joins. This iteration does **not** rewrite those graph topologies, hide them behind a compatibility adapter, or claim that the low-level single-payload invariant is enforced.

The next cutover should migrate those graphs to one shared struct/tuple payload and explicit projection consumers (or remove tests that genuinely describe a retired behavior **only after** equivalent conformance is preserved), then change `NodeAction`, the `Executor`, `ExecutionSession::complete`, output lowering and `WaveState::publish_via` together. The engine must admit one payload per activation and deliver it to zero or more existing edges, with no multi-value escape hatch.

## Verification limits

Only source-level and packaging checks are possible here. Cargo and rustc are not installed. The added Rust tests are **unexecuted**; compilation and behavior cannot yet be certified. No GitHub branch was changed.
