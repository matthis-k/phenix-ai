# Test architecture (Rust only)

## One source of semantics

All tests run the real `GraphBuilder`/`PatchPlanner` compilation path and
`Deployment`/`Engine` execution path. Tests never implement a substitute
scheduler or bypass the same port, edge, readiness, coloring and identity
checks that production will use.

| Responsibility | API | Coverage |
|---|---|---|
| Graph construction | `GraphBuilder`, `ModuleCatalog` | Existing graph/module integration tests |
| Patch composition | `PatchBuilder`, `PatchPlanner` | `tests/patch_planner.rs`, `tests/testing_api.rs` |
| Wave scheduling and cancellation | `Engine::begin`, `ExecutionSession` | `tests/assembly_cancel.rs`, internal unit tests |
| Local/wire values | `Value::local`, `Value::of` | `tests/testing_api.rs`, `tests/layout_api.rs` |
| Error and isolation checks | `Fixture`, `ExpectWave` | `tests/testing_api.rs`, internal and integration tests |
| Repository hygiene | `xtask::scope` | `tools/xtask/src/scope.rs` `#[test]` fixtures |
| Native binding seam | Rust-selected-operation mock | `tests/selected_operations.rs` |

## Testing API design

`Fixture::new(compiled_graph)` defaults to full payload retention; this is
only a test configuration of the production engine. Use
`Fixture::from_deployment(Deployment::new(...))` when testing custom store,
executor, edge adapter or retention. `Fixture::fire()` directly delegates to
`Engine::fire`. `Fixture::engine().begin(...)` supports manual in-flight
interleavings and cancellation. Assertions are extension methods on the
ordinary `WaveReport`, not an alternative report type.

Prefer typed endpoints, named triggers and source-specific `ExpectWave`
assertions. `expect_local_at`/`expect_wire_at` require exactly one retained
publication from the selected output; for multiple activations assert the
publication count and inspect the individual events explicitly.
Test negative cases: failed input admission, failed activations, incomplete
cohorts, patch conflicts, cancellation, and later unaffected waves.

## Run

```sh
cargo run -p xtask -- verify
```

This performs scope audit, Rust formatting/check, all workspace tests (with
all features), and graph tests without the derive feature. GitHub Actions
runs the same command. No Python executable is required.

The only acceptable non-Rust test source is a future explicit cross-language
plugin/ABI fixture, with Rust asserting its results.

## Delivery-vs-publication assertions (Iteration 5)

A `PublicationEvent` means the node emitted an output; it does not mean every
structurally connected consumer received it. `DeliveryEvent` records the
compiler-owned edge and actual target **only after** an entire fan-out batch
is accepted. Tests can assert exact routes:

```rust
let source = fixture.output("source", "value");
let left = fixture.input("left", "run", "value");
fixture.fire("go", vec![Value::local(4_i64)]).unwrap()
    .expect_success()
    .expect_publication_count(source, 1)
    .expect_deliveries_from(source, &[left]);
```

An empty expected destination list proves no outgoing edge was committed.
The check is independent of retained payloads, including in `MetadataOnly`
mode. Use `tests/iteration_five.rs` for selector errors, rejected adapters,
terminal-output retention, and failure isolation.

## Iteration 9: unified return-based routing

`tests/iteration_nine.rs` verifies tuple fanout, `Option`/`Result` payload
semantics, prefix fail-closed behavior and filtering restricted to compiled
candidate edges. `tests/patch_output_routing.rs` verifies patch-provided
node-action replacement, conflict/rollback and restoration of ordinary
broadcast. The `xtask` scope audit rejects accidentally reintroducing the
removed graph-level or patch-level output-policy APIs.
