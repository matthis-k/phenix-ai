# Iteration 9 — one output-routing authority

**Baseline:** Iteration 8 (`phenix_graph_iteration_8_20261107.zip`).
**Scope:** Only the isolated Rust graph crate, its integration tests, docs and Rust xtask.
**Status:** Source implementation and regression suite authored. **No cargo/rustc in this environment; not compiler-verified.**

## Targets and observed changes

| Target | Change | Evidence / gate |
|---|---|---|
| One output-routing API | Removed `OutputSelector`, `OutputRouting`, `GraphBuilder::route`, `PatchBuilder::route`, `OutputSlot::select`, `RouteEndpoint` | None remain in active Rust graph source |
| One action owns output decision | Plain `T` broadcasts, `Output<T>` chooses connected targets during node execution | One executor settlement path; prior selector fallback removed |
| Patch conflicts remain central | Patches use ordinary node `replace`, not `OutputRoutingChanged` or a separate output patch footprint | Replacement/conflict/undeclared-write Rust tests migrated |
| Fail closed on empty prefix | `Output::to_prefix("")` cannot unexpectedly broadcast | New dedicated Rust regression |
| Keep data separate from routing | Tuple, `Option<Result<...>>` are ordinary single payload values | New Rust graph integration tests |
| Only existing destinations | Filter is invoked only over compiled outgoing edges | New Rust graph integration test |
| Prevent regression | Rust xtask scope audit prohibits reintroduction of parallel selector API | New Rust audit fixture |

## Before / after — source-exact authoring

From `crates/phenix-graph/tests/iteration_five.rs`.

```rust
// BEFORE (Iteration 8): compute with one closure, then register a second
// persistent policy on the output port.
let source = graph.node("source", |x: i64| x + 1).unwrap();
let left = graph.node("left", |x: i64| x * 2).unwrap();
let right = graph.node("right", |x: i64| x * 3).unwrap();
graph.connect(source.output(), left.input()).unwrap();
graph.connect(source.output(), right.input()).unwrap();
graph.route(source.output(), |_, edges: &[EdgeId]| Ok(vec![edges[0]])).unwrap();

// AFTER (Iteration 9): the action returns both the payload and routing decision.
let left = graph.node("left", |x: i64| x * 2).unwrap();
let right = graph.node("right", |x: i64| x * 3).unwrap();
let selected = left.input();
let source = graph.node("source", move |x: i64| Output::to(&selected, x + 1)).unwrap();
graph.connect(source.output(), left.input()).unwrap();
graph.connect(source.output(), right.input()).unwrap();
```

**Benefit:** Routing can depend on intermediate results inside the action without coordinating a separate callback or an output-port policy. A plain return remains a broadcast. The output port declares a type, not an action-specific decision.

## Before / after — actual patch authoring

From `crates/phenix-graph/tests/patch_output_routing.rs`.

```rust
// BEFORE (Iteration 8): second routing-specific patch verb and conflict target.
let mut selection = PatchBuilder::new("route");
selection.after("base");
selection.route("source.out.value", |_, edges: &[EdgeId]| {
    Ok(edges.first().copied().into_iter().collect())
}).unwrap();

// AFTER (Iteration 9): one replacement mechanism, one action return contract.
let mut selection = PatchBuilder::new("route");
selection.after("base");
selection.replace("source", NodeTemplate::local_directed(
    |x: i64| Output::to_prefix("left", x + 1),
));
```

**Benefit:** The conflict footprint is just `PatchTarget::Node("source")`. The planner does not maintain a second class of output-policy edits. A later node replacement changes behavior **including routing**; no hidden port-side selector persists.

**Tradeoff:** Patches that previously modified only routing must now replace the producing action, or insert a normal downstream node. This is intentional single-owner semantics, but it can increase changes when the original action is opaque. A better action-decorator API could be evaluated later only if real plugin call sites demonstrate the need; do not preemptively restore a parallel routing mechanism.

## Before / after — executor

From `crates/phenix-graph/src/lib.rs`.

```rust
// BEFORE (Iteration 8): a second fallback path for graph-configured policies.
None => match &output.routing {
    OutputRouting::Broadcast => Ok(candidate_ids.clone()),
    OutputRouting::Select(selector) => selector.select(&em.value, &candidate_ids),
},

// AFTER (Iteration 9): no fallback; plain T always broadcasts.
None => Ok(candidate_ids.clone()),
```

**Benefit:** The executor has one selector source (`Emission.selection`) and one validation/prepare/commit pipeline. This removes policy-order ambiguity between returned `Output<T>` and permanently attached port routing.

## Additional correctness change

`Output::to_prefix` used to treat an empty prefix as a match for **every** edge. It now matches **no** edges. A missing or empty path cannot silently broaden routing authority; `Output::all` explicitly requests broadcast.

## Rust coverage and scope

- Migrated route-selection and wave-failure tests in `tests/core_iteration.rs` and `tests/iteration_five.rs` to `Output<T>`.
- Rewrote `tests/patch_output_routing.rs` around normal action replacement while preserving conflict/atomicity assertions.
- Added `tests/iteration_nine.rs` (four focused integration tests: tuple fan-out, nested data values, empty prefix, candidate-filter confinement).
- Added one xtask test that rejects a reintroduced public routing method.
- No Python test scripts or secondary executor added.

## Ratings — design review, conditional on Rust verification

| Responsibility | Prior candidate | This candidate | Why |
|---|---:|---:|---|
| Graph API coherence | 9.2 | **9.4** | Exactly one output routing vocabulary |
| Patch composition | 9.0 | **9.2** | One kind of node-behavior replacement/conflict |
| Execution | 8.7 | **8.9** | One routing decision path; candidate set always compiler-owned |
| Compiler/graph model | 9.0 | **9.1** | Output port no longer carries an opaque policy callback |
| Rust tests | 9.2 | **9.2** | Better conformance, but execution unverified |

**Do not promote these to verified scores.** Neither `cargo test` nor `cargo fmt` was available. Source scans and `git diff --check` are not substitutes for compiling Rust.

## Remaining friction, ordered graph-first

1. **Compilation and tests must pass.** A Rust 2024 toolchain is needed before acceptance or production reintegration.
2. Generic node-return behavior still has low-level `function_optional`/`function_fallible` and branch-specific implementations. Review these next against the agreed one-value output model; preserve existing low-level policy tests or migrate them.
3. `NodeTemplate::local_directed` still exists beside `local_function` as an adapter. The public `graph.node(...)` already presents one entry point, but the internals can be simplified further after verification.
4. Action/policy executable identity remains unresolved; the structural manifest must not be used as a generation identity.
5. Only after graph quality gates pass should native, provider, kernel or frontend wiring be reintroduced.
