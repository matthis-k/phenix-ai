## Iteration 14 — output presence and recipient selection are distinct

A declared output type exists even when a particular invocation produces no
value. `Output::none()` emits no value; `Output::to_all([], value)` publishes a
value without delivering to any edges. Ordinary `Option<T>` and `Result<T,E>`
remain payload data. See [`docs/ITERATION_14_CHANGELOG.md`](docs/ITERATION_14_CHANGELOG.md).

## Iteration 11 — fallible layouts and declaration ownership

The graph declaration API now propagates `LayoutError` through `GraphBuildError` rather than panicking on an overflowing layout. Derived and manual wire/local layouts use the same checked slot allocator. Local layout expansion happens **once** at node declaration; the runtime reuses that validated shape. Ordinary `graph.node("name", |x: T| ...)` remains unchanged.

See [`docs/ITERATION_11_CHANGELOG.md`](docs/ITERATION_11_CHANGELOG.md). The low-level `Vec<Emission>` contract is still being migrated and is explicitly **not** claimed as complete. Rust tests are unexecuted in this environment.

# Phenix — one standalone graph, Rust-only test and build tooling

This isolated workspace contains the November 6 standalone graph prototype,
its derive macro, and a **Rust `xtask`** for scope auditing, destructive
branch-only cutover and verification. The old kernel/graph/agent execution
paths do not exist in the active workspace.

## Run the entire Rust verification suite

Requires Rust 2024 (Rust >= 1.85), Cargo and Git.

```sh
cargo run -p xtask -- verify
```

This checks the graph scope, formats, compiles all workspace targets and runs
all graph and cutover tests (including `--no-default-features`). For a focused
operation:

```sh
cargo run -p xtask -- audit .
cargo test --workspace --all-targets --all-features
cargo test -p phenix-graph --no-default-features
```

## Test API

`phenix_graph::testing::{Fixture, ExpectWave}` provides concise assertions
for normal waves, failed waves, typed local/wire outputs, and repeated triggers, using the same named/typed trigger selector as `Engine`.
It delegates to the exact production `Deployment` / `Engine` implementation;
there is **no second test scheduler or test-only topology definition**.

```rust
use phenix_graph::{GraphBuilder, Value};
use phenix_graph::testing::{Fixture, ExpectWave};

let mut graph = GraphBuilder::new("test");
let node = graph.node("double", |x: i64| x * 2).unwrap();
let ingress = graph.expose_input("in", node.inlet()).unwrap();
graph.expose_output("out", node.output()).unwrap();
let trigger = graph.trigger("run", [ingress]).unwrap();
let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
fixture.fire("run", vec![Value::local(21_i64)]).unwrap()
    .expect_success().expect_local_at(fixture.output("double", "value"), &42_i64);
```

Additional integration tests use the same public API for patch resolution,
module boundaries, output retention, native selected-operation simulation,
cancellation, wave coloring and negative/error cases.

**No Python unit tests, audit scripts or cutover scripts** are active. Any
future cross-language plugin fixture may be a narrow test-only exception.

## Current boundary

The language-boundary example under
`crates/phenix-graph/tests/support/selected_operation.rs` is a Rust mock,
not a production native ABI. Kernel integration remains future work. Output routing is returned by node actions through `Output<T>`. Patches change an action with `replace`; there is no second output-policy patch or output-port selector. Native host binding remains out of scope. Graph-only tests do not establish product parity.

See `docs/OWNERSHIP.md` and `docs/REINTEGRATION.md` before reconnecting the host.

## Canonical authoring / compilation (Iteration 2)

Typed nodes use **one** `graph.node(name, definition)` call. A plain closure
uses local values; shared ownership is explicit `Arc<T>` in the handler signature; `Node::wire`, `Node::with_layout`,
and `Node::wire_layout` configure input representation. A plain `Option<T>` or `Result<T, E>` is ordinary output data.
`Output<T>` changes only routing: payload values are never decoded or interpreted by output routing.
`GraphBuilder::add` remains the direct low-level path for custom actions, not a
second execution engine.

Compilation returns structured `CompileDiagnostic` errors. Plain node values broadcast; `Output<T>` alone selects a subset of the producing node's existing outgoing edges. Patches use `replace` to change routing behavior, with the same node-edit conflict detection as any other action replacement.

See `docs/ITERATION_2_REPORT.md` for tests, limitations and score targets.

## Conditional selection — the same ordinary node

A conditional node has one output. Its function returns the same payload
regardless of target, with `Output::to` selecting existing connected inputs.
No `Branch` enum, specialized action type, or second output-port vocabulary.
The following pattern is exercised by `tests/iteration_three.rs`.

```rust
let yes = graph.node("yes", |x: i64| x + 10)?;
let no = graph.node("no", |x: i64| x + 20)?;
let yes_target = yes.inlet();
let no_target = no.inlet();
let choice = graph.node("choice", move |x: i64| {
    Output::to(if x >= 0 { &yes_target } else { &no_target }, x.abs())
})?;
graph.connect(choice.output(), yes.inlet())?;
graph.connect(choice.output(), no.inlet())?;
```

Node/module replacement still uses one `replace` verb, shared by the graph
and the patch builder.

`CompiledGraph::topology_manifest()` is a deterministic *structural report*,
not an executable or cryptographic identity: closure and custom policy behavior
are opaque. Never use this manifest alone as a generation ID. Test fixtures
can now assert named output sources, typed errors and publication counts
without relying on publication order. Iteration 4 additionally includes public
module interfaces and readiness-policy types in the structural report, and
requires exactly one matching retained publication for value assertions.
See `docs/ITERATION_4_CHANGELOG.md` for source-exact before/after examples.

## Iteration 5 — observable edge routing, no partial fan-out

`WaveReport::delivery_events()` now distinguishes an output's emission from
actual accepted edge deliveries. For route-specific Rust checks, use
`Fixture::input` plus `ExpectWave::expect_deliveries_from` to assert the complete
set of reached input slots. `ReportRetention::TerminalOutputs` now retains a
routed emission when **no** outgoing edges were selected in that wave.

The former patch-level routing policy was removed in Iteration 9; use a node action returning `Output<T>` and replace that action in patches. The historical implementation is described in `docs/ITERATION_5_CHANGELOG.md`.

## Iteration 6 — in-function output selection

A plain node return broadcasts by default. Only custom dispatch requires
`Output<T>`; one invocation produces at most one payload under the new typed
construction path. Prefix and predicate selectors inspect **only compiled
outgoing connections**. `OutputRef<T>` names a graph-authoring output handle.

```rust
use phenix_graph::{GraphBuilder, Node, Output};
let mut graph = GraphBuilder::new("example");
let sink = graph.node("sink", |x: i64| x + 1).unwrap();
let selected = sink.inlet();
let source = graph.node("source", Node::new(move |x: i64| {
    let route_to_sink = x > 0;
    let value = x.abs();
    if route_to_sink { Output::to(&selected, value) }
    else { Output::to_all([], value) }
})).unwrap();
graph.connect(source.output(), sink.inlet()).unwrap();
```

See `docs/ITERATION_6_CHANGELOG.md` and `tests/output_wrapper.rs` for source examples and coverage; the legacy selector cleanup was completed in Iteration 9.

## Iteration 8 — payload and routing are independent

`Output<T>` is strictly a value plus destinations. The node return type is
preserved, including `Option<T>`, `Result<T, E>`, and domain enums. The graph
cannot infer domain failure or absent data from these payloads.

```rust
let optional = graph.node("optional", |x: i64| Output::all((x > 0).then_some(x)))?;
let result = graph.node("result", |x: i64| {
    Output::all(if x >= 0 { Ok(x) } else { Err("negative") })
})?;
let quiet = graph.node("quiet", |x: i64| Output::to_all([], x))?;
```

`Output::to_all([], value)` publishes the payload while selecting zero outgoing edges.
A node execution failure comes from its action's `Result<_, String>` at the
`NodeAction` boundary, not from the value carried in `Output`.

`Output::option` and `Output::result` were removed in Iteration 8;
zero-argument `Output::none()` was reintroduced in Iteration 14 with its
separate no-value semantics. See `docs/ITERATION_8_CHANGELOG.md`.

## Iteration 9 — one output routing API

An ordinary return broadcasts. `Output::to`, `to_all`, `to_prefix`,
`to_filter`, `all` and `none` select only existing outgoing connections.
`GraphBuilder::route`, `PatchBuilder::route`, `OutputSlot::select` and
`OutputRouting` have been removed, including their patch-specific footprints.
A patch replaces a node's action to change routing, instead of registering a
second persistent policy on the output port.

```rust
let mut patch = PatchBuilder::new("route");
patch.replace("source", NodeTemplate::local_function(|x: i64| {
    Output::to_prefix("workers", x + 1)
}));
```

An empty prefix matches no destinations; it never accidentally broadcasts.
See `docs/ITERATION_9_CHANGELOG.md` for existing before/after call sites and
Rust acceptance tests. Cargo/rustc are not available in this environment.

## Iteration 10 — one local return adapter, fewer layout layers

- Ordinary `T` and directed `Output<T>` returns now use the same
  `LocalFunctionNodeAction`; there is no `local_directed` constructor.
- All `Node::branch` / `Branch<L,R>` authoring, wire-specific optional/fallible
  action presets, and their private runtime adapters are removed.
- `PortArgs` now directly implements `InputLayout`/`DecodedInputLayout`.
  Use `Port::for_layout::<Args>(...)` rather than the old `for_args` alias.
- The low-level `NodeAction` still accepts a vector of emissions for existing
  graph-conformance fixtures. Converting that entire interface to the one-value
  invariant remains an explicit, **unverified and unfinished** migration; it
  must not be silently assumed completed.

See `docs/ITERATION_10_CHANGELOG.md`. Rust compilation remains necessary.

## Iteration 13 — unit is the only outputless value

Every typed node declares exactly one output slot; its ordinary returns produce
one value, including `()`. A directed `Output::none()` return can choose to
produce no value on a particular invocation. This latter distinction was
implemented in Iteration 14. The output payload and its destinations are
independent: ordinary `()` broadcasts to connected consumers, while
`Output::to_all([], ())` publishes unit with zero deliveries.

```rust
let producer = graph.node("producer", |_: i64| {}).unwrap();
let consumer = graph.node("consumer", |_: ()| 42_i64).unwrap();
graph.connect(producer.output(), consumer.inlet()).unwrap();

// A unit payload that is published but sent to nobody:
let quiet = graph.node("quiet", |_: i64| Output::to_all([], ())).unwrap();
```

`OutputSchema::output_slot()` and `IntoEmission::into_emission()` no longer return
`Option`. The `EmitNothing` helper was retired; ordinary unit-valued actions
are sufficient. `Option<T>` and `Result<T,E>` remain data. A multi-port test now
publishes `Option<i32>` (including `None`) rather than silently dropping outputs.

**Scope note:** The preexisting low-level `NodeAction::execute` still returns
`Result<Vec<Emission>, String>` for multi-output conformance graphs. This
inconsistency is explicitly not solved by the typed contract change; the
remaining low-level graph topologies must be migrated to shared tuple/struct
payloads and projection nodes before the vector contract can be removed.
See `docs/ITERATION_13_CHANGELOG.md`.


## Typed declaration layouts (Iteration 15)

```rust
#[derive(Clone, phenix_graph::LocalPortLayout)]
struct AddArgs { left: i64, right: i64 }

let mut graph = phenix_graph::GraphBuilder::new("example");
let add = graph.node("add", |x: AddArgs| x.left + x.right)?
    .input(phenix_graph::Layout::fields())?;
// `add.field(AddArgs::SLOT_LEFT)` and SLOT_RIGHT now resolve to real slots.
```

Input layout is type-checked against the handler, then connectivity is validated
at graph compilation. The unconfigured default remains atomic. The atomic input
endpoint accessor is now `node.inlet()`; `node.input(layout)` configures a
declaration. See `docs/ITERATION_15_CHANGELOG.md` for limitations and migration.

## Input ownership and layout status (Iteration 16)

Ordinary local nodes express shared ownership in Rust types. Use
`|value: Arc<Domain>| ...` and produce `Arc<Domain>` when multiple downstream
consumers need one non-`Clone` allocation. `Node::shared` is removed.

A typed declaration can override the input layout before compilation:

```rust
let plus = graph.node("plus", |args: AddArgs| args.left + args.right)?
    .input(Layout::fields())?;
```

The default for an unconstrained `A` is still an **atomic input**; derived
field-default selection is not implemented. By-value non-`Clone` inputs still
require the owned structural-layout adapter. See
`docs/ITERATION_16_CHANGELOG.md` for these remaining boundaries.
