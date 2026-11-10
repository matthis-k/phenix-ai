# Graph Consolidation API Examples

The consolidation changes implementation ownership, not the public authoring API.
**Before = after in all ten cases below.** Each single Rust block is deliberately
the common before/after excerpt, not a proposed syntax improvement. Node-to-module
replacement changes a graph implementation; it is not a new API introduced by
consolidation.

All excerpts come from [tests/consolidation_api.rs](tests/consolidation_api.rs)
in `phenix-graph`. That file supplies `use phenix_graph::*`, complete surrounding
declarations, compilation, deployment, trigger execution and assertions. These are
tested excerpts, not standalone programs: the named test is the executable example.
`documented_rust_excerpts_are_compiled_test_source` checks all ten excerpts against
that compiled source (ignoring indentation), preventing illustrative syntax drift.
This tests the current tree; it does not claim to have run a historical checkout.

Run all examples with the default `derive` feature:

```sh
nix develop --command cargo test -p phenix-graph --test consolidation_api --locked
```

## 1. Ordinary Input

Before = after. Test: `ordinary_input`; local input `21` produces `42`.

```rust
let double = graph.node("double", |x: i64| x * 2).unwrap();
let input = graph.expose_input("input", double.inlet()).unwrap();
graph.trigger("run", [input]).unwrap();
run(graph, vec![Value::local(21_i64)], &[42]);
```

`run` compiles and deploys with payload retention, fires `run`, checks success,
and compares every published `i64` against the expected sequence.

## 2. Zero-Input Source

Before = after. Test: `zero_input_source`; explicit start, no synthetic unit input,
one publication containing `42`.

```rust
let source = graph.node("source", || 42_i64).unwrap();
graph.trigger("run", [source.operation().start()]).unwrap();
run(graph, vec![], &[42]);
```

## 3. Two Structured Fields

Before = after. Test: `structured_two_fields` (`derive`). `Pair` has `x: i64` and
`y: i64`, deriving `Clone`, `LocalPortLayout`, and `InputFields`. The node uses
`.input(Layout::fields())`; separate public inputs deliver `20` and `22`, and the
ordinary same-cohort join produces exactly one `42`.

```rust
let x = graph
    .expose_input("x", sum.inlet().field(Pair::SLOT_X).target().unwrap())
    .unwrap();
let y = graph
    .expose_input("y", sum.inlet().field(Pair::SLOT_Y).target().unwrap())
    .unwrap();
graph.trigger("run", [x, y]).unwrap();
```

## 4. Independently Declared Encoded Types

Before = after. Test: `independently_declared_encoded_types` (`derive`). No alias
or shared Rust type is used:

```rust
#[derive(Clone, ContractValue)]
#[contract(schema = "consolidation/number")]
struct Sender {
    number: i64,
}
#[derive(Clone, ContractValue)]
#[contract(schema = "consolidation/number")]
struct Receiver {
    number: i64,
}
assert_eq!(Sender::schema(), Receiver::schema());
```

The complete test connects typed `source.output()` to `sink.inlet()` using
`Node::wire` on both nodes. The source encodes `Sender`, the sink decodes
`Receiver`, and encoded input `41` produces `42`. Equal complete descriptors and
compatible codecs matter, not Rust `TypeId`; this does not allow unrelated local
types or silently bridge local and encoded values. Broader descriptor-mismatch,
malformed-byte, and independently compiled crate tests remain in `portable_wire.rs`.

## 5. Custom Readiness

Before = after. Test: `custom_readiness`. `SelectRight` implements `PortPolicy`,
not a replacement scheduler. Its exact selection logic is:

```rust
fn select(&self, snapshot: &PortSnapshot) -> Option<Selection> {
    let candidate = snapshot.slot_named("right")?.candidates().first()?;
    Some(Selection::new(
        candidate.cohort_id(),
        vec![Pick::new(candidate.slot_id(), candidate.delivery_id())],
    ))
}
```

The raw node declares `left` and `right` local slots with that policy and reads
only selected slot `1` through `InvocationCtx`. A left-only wave has zero
activations; a right-only wave has one activation and publishes `42` despite the
absent left input. The policy reports no incomplete groups; no all-fields
reconstruction or implicit readiness barrier is claimed.

## 6. Selective Routing

Before = after. Test: `selective_routing`.

```rust
let target = left.inlet();
let source = graph
    .node("source", move |x: i64| Output::to(&target, x))
    .unwrap();
graph.connect(source.output(), left.inlet()).unwrap();
graph.connect(source.output(), right.inlet()).unwrap();
```

Both routes exist structurally. Input `41` selects only `left`, producing
publications `[41, 42]`; `right` panics if incorrectly invoked. `Output::to`
selects an existing destination, not an implicit connection or a second output.

## 7. Nested Composition

Before = after. Test: `nested_f_x_y_equals_a_x_b_y` (`derive`). The real nested
module `B` computes `2*y`; module `F` instantiates it and combines its output
with `x` in `A`. Thus `F(x,y)=A(x,B(y))=x+2*y`, not a flat closure masquerading
as composition. The critical wiring is:

```rust
f.expose(
    f.inlet().field(Pair::SLOT_X),
    a.inlet().field(Pair::SLOT_X).target().unwrap(),
)
.unwrap();
f.expose(f.inlet().field(Pair::SLOT_Y), b.inlet()).unwrap();
f.connect(b.output(), a.inlet().field(Pair::SLOT_Y).target().unwrap())
    .unwrap();
let f = f.finish(a.output()).unwrap();
```

The root instantiates `F`, delivers separate inputs `2` and `20`, and checks
the retained terminal publication `[42]` and exactly two activations. This test
uses `ReportRetention::TerminalOutputs`: retaining `B`'s intermediate local value
would prevent `A` from taking ownership of that structured field. It does not
claim arbitrary retained local payloads can be reconstructed by value. Module
boundaries add no executable wrapper node.

## 8. Node-To-Module Replacement

Before = after API. Test: `node_to_module_replacement`. A module package exposes
the same atomic input and output as the original node, with internal `add` and
`double` children implementing `(x+1)*2`.

```rust
run(graph.clone(), vec![Value::local(3_i64)], &[8, 18]);
graph.replace("target", &package).unwrap();
assert!(graph.expose_input("stale", target.inlet()).is_err());
run(graph, vec![Value::local(3_i64)], &[4, 8, 18]);
```

The test executes the actual original and replaced graphs. Existing ingress,
trigger, and outgoing edge survive replacement; the downstream sink still yields
`18`. Internal publication history changes, so only the public computation is
equivalent. Old typed handles are revoked rather than silently retargeted.

## 9. Multiple Contributors

Before = after. Test: `multiple_contributors`. The base patch declares `a`, its
public input and trigger; a separate contribution declares and wires `b`:

```rust
let mut extension = PatchBuilder::new("extension");
extension.after("base").node("b", |x: i64| x * 2);
extension.connect("a", "b").unwrap();
```

Both registration orders are executed. The planner selects `["base",
"extension"]`, and `20` flows through `(20+1)*2` to `42`. Dependency ordering
is explicit; registration order is not used as a hidden dependency.

## 10. Finite Cycle

Before = after. Test: `finite_cycle_repeated_activations`. A countdown node emits
`x-1` while positive and returns `Output::none()` at zero:

```rust
graph.connect(step.output(), step.inlet()).unwrap();
let input = graph.expose_input("input", step.inlet()).unwrap();
graph.trigger("run", [input]).unwrap();
```

One engine executes starts `[0, 3, 5, 3]`. Each wave succeeds without exhausting
its budget, has exactly `start+1` activations, and publishes the descending
sequence through zero. This checks repeated activations within a wave and reuse
across waves, not merely repeated independent trigger calls. Suppression is
explicit `Output::none()`, not an interpretation of an ordinary `Option` payload.

## Coverage Boundaries

The optional diamond is already covered by
`selected_operations.rs::independently_selected_operations_form_one_compiled_diamond`:
`A` fans out to independently contributed `B` and `C`, then joins at `D`, with
both contribution orders and result `25`. It is a coverage pointer, not an
eleventh snippet or a claim that the focused command runs that separate target.

The ten cases establish executable current API usage and expected semantics.
They do not independently certify every internal consolidation invariant or
benchmark performance. For Batch B's stored-state deletion and independent
scheduler-oracle evidence, see [the review](graph-consolidation-review.md) and
[the state record](graph-consolidation-state.md). Ownership-specific move-only,
clone-count and explicit-`Arc` evidence lives in `quality_10.rs` and
`action_lowering.rs`; local payload movement and encoded reconstruction should
not be conflated. No new evidence rating or blanket A-H acceptance is inferred
from these examples.
