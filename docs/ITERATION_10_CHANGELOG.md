# Iteration 10 — graph boundary, first consolidation batch

**Baseline:** Iteration 9 source. **Scope:** isolated Rust graph workspace; no kernel, plugin-loader or native integration changes. **Status:** source changes complete; not compiler-verified.

## Acceptance targets and current result

| Target | Result | Verification still needed |
|---|---|---|
| One local node action for ordinary/direct returns | Implemented: `LocalFunctionNodeAction<A,O,R,F>` with sealed `LocalReturn<O>` | Cargo check and inferred-signature tests |
| Remove special branch node authoring and implementation | Removed `BranchNode`, `Node::branch[_named]`, `Branch<L,R>`, `LocalBranchNodeAction`, `NodeTemplate::local_branch` | Run migrated conditional/cycle/agent-loop tests |
| Remove optional/fallible wire action variants | Removed `OptionalFunctionNodeAction`, `FallibleFunctionNodeAction`, `function_optional`, `function_fallible` and `register_*` | Run explicit-error and selected-operation regressions |
| Make derived/manual PortArgs a direct layout | Removed `PortArgsLayout` and `Port::for_args`; `PortArgs` implements `InputLayout` and `DecodedInputLayout` | Compile with and without `derive` |
| Eliminate all low-level multi-emission results | **Outstanding.** `NodeAction::execute` still returns `Vec<Emission>` for older multi-output conformance cases | Migrate those cases to shared payloads/projections before making the signature singular |
| Turn layout-slot overflow panics into structured failures | **Outstanding.** Existing `PortLayout`/`LocalPortLayout` append methods are infallible | Change trait/derive contracts together; do not add a catch-all panic shim |

No new scheduler, value interpreter, independent patch execution path, Python tests or compatibility aliases were introduced.

## Source-exact before and after

### Conditional node (tests/iteration_three.rs)

```rust
// BEFORE — Iteration 9
let choice = graph.node("choice", Node::branch(|x: i64| {
    if x >= 0 { Branch::<i64, i64>::Left(x) }
    else { Branch::<i64, i64>::Right(-x) }
})).unwrap();

// AFTER — Iteration 10
let yes_target = yes.input();
let no_target = no.input();
let choice = graph.node("choice", move |x: i64| {
    Output::to(if x >= 0 { &yes_target } else { &no_target }, x.abs())
}).unwrap();
```

**Benefit:** one output type and one node category; selection is based on computation, and both consumers receive the same payload. The actual test also connects `choice.output()` to both existing inputs.

### Action replacement (tests/patch_output_routing.rs)

```rust
// BEFORE — Iteration 9
selection.replace("source", NodeTemplate::local_directed(
    |x: i64| Output::to_prefix("left", x + 1),
));

// AFTER — Iteration 10
selection.replace("source", NodeTemplate::local_function(
    |x: i64| Output::to_prefix("left", x + 1),
));
```

**Benefit:** one low-level constructor and physical action for plain values and `Output<T>`. The return value determines broadcast vs selected delivery; the graph compiler and executor are identical.

### Wire decoding (tests/support/selected_operation.rs)

```rust
// BEFORE — Iteration 9
NodeTemplate::function_fallible(move |input: A| -> Result<O, String> {
    let value = self.dispatcher.invoke(
        &self.selected, &self.operation, Value::of(input),
    )?;
    value.get::<O>()
})

// AFTER — Iteration 10
NodeTemplate::with_action(
    0,
    vec![Port::for_layout::<Atomic<A>>(0, "run", AllSameCohort)],
    vec![OutputSlot::typed::<O>(0).named("value")],
    move |mut ctx: InvocationCtx| {
        let input = ctx.take_prepared::<A>()?;
        let value = self.dispatcher.invoke(
            &self.selected, &self.operation, Value::of(input),
        )?;
        Ok(vec![emit(0, value.get::<O>()?)])
    },
)
```

**Benefit/tradeoff:** `Result<O,E>` is not an output-control keyword; the external operation adapter explicitly maps provider errors into node action errors. This requires a few more lines at the lower-level ABI seam. Production operation binding remains deferred.

### Port arguments (tests/graph_api.rs)

```rust
// BEFORE — Iteration 9
vec![Port::for_args::<DerivedArgs>(0, "run", AllSameCohort)]

// AFTER — Iteration 10
vec![Port::for_layout::<DerivedArgs>(0, "run", AllSameCohort)]
```

**Benefit:** the argument type directly implements the layout contract. No extra `PortArgsLayout` wrapper or duplicate port construction method.

## Ownership decisions

- **Node action:** calculates one local result via `LocalReturn<O>`; the typed local path always creates one `Emission`. The trait is sealed so downstream crates cannot silently reinterpret `Option` or `Result`.
- **Output wrapper:** contains the unchanged payload plus an existing-edge selection. It does not alter the value or create graph topology.
- **Port/layout:** declares and decodes named input slots; `PortArgs` supplies this directly. Port readiness remains separate.
- **Execution:** validates routing, prepares adapters, and commits edge deliveries atomically. No scheduling change in this iteration.
- **Low-level conformance:** still supports `Vec<Emission>` and heterogeneous output ports. This is now the chief semantic mismatch with the agreed one-payload model, not an alternative scheduler.

## Verification

New Rust tests: `tests/iteration_ten.rs` (direct `PortArgs` layout, directed low-level return, `Option`/`Result` as data). Existing Rust tests were migrated rather than deleted (`iteration_three`, `assembly_cancel`, `assembly_model_checks`, `patch_syntax`, `layout_api`, `selected_operations`, `graph_api`, `manual_api`, `patch_output_routing`).

Source-only checks: diff whitespace and retired-symbol scans. **Rust/Cargo are absent in this execution environment.** The changed generic inference, derive coherence, formatting, and all behavioral expectations must be compiled and tested before the new API is treated as verified.

## Next iteration, still graph-first

1. Replace `NodeAction::execute -> Result<Vec<Emission>, String>` with an at-most-one-payload contract. First refactor real multi-emission conformance cases into shared tuple/struct payloads and ordinary projections, preserving failure-isolation coverage.
2. Make slot expansion fallible, including `PortLayout`, `PortLayoutAt`, `LocalPortLayout`, `LocalPortLayoutAt`, the derive crate, and all constructor call sites. Replace the current `expect("too many slots")` behavior with structured diagnostics; no parallel `try_` API.
3. Compile the entire workspace with Rust 2024 and run both feature configurations before proceeding outward to patch ownership/deployment/store.
