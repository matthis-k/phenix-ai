# Unified patch API (November 2026)

## Contract

There is **one** plugin contribution entry point: a pre-compilation patch. An
initial graph is an additive patch applied to an empty GraphAssembly. Patches
own operations and dependency/conflict metadata; only the completed graph is
compiled and executed. The `patch!` proc macro is entirely optional.

### Macro-free

```rust
use phenix_graph::{PatchBuilder, PatchPlanner};
let mut b = PatchBuilder::new("branch.b");
b.after("base").node("b", |x: i64| 2 * x);
b.connect("a", "b")?;
b.connect("b", "d.in.run.left")?;
planner.add_patch(b)?;
```

### With `patch!` (default `derive` feature)

```rust
use phenix_graph::patch;
planner.add_patch(patch! {
    id: "branch.b";
    after: ["base"];
    node b = |x: i64| 2 * x;
    a -> b -> d.in.run.left;
}?)?;
```

`patch!` produces `Result<PatchBuilder, PatchError>`; it does **not** mutate an
assembler, run a separate compiler, or bypass patch conflicts. Its supported
statements are `id`, `after`/`requires`, `priority`, `node`, `instance`,
`replace`, `connect` (optional keyword), `disconnect`, `input`, `output` and
`trigger`. For a structured input node use `node d = handler, layout = Layout::<T>::fields();`.

## Canonical addressing

| Source / destination | Meaning |
|---|---|
| `a` | Only available output or input slot (error if ambiguous) |
| `a.out.value` | Named node output |
| `d.in.run.left` | Named input port and slot |
| `d.in.run` | Only slot on port `run` (error if multiple) |
| `module.in.request` | Public module input |
| `module.out.reply` | Public module output |

Macro shorthand *never* chooses a port by matching its type, and never
implicitly accesses a module's private nodes. Explicit typed-handle wiring
(`graph.connect(a.output(), b.inlet())?`) remains available and checked by Rust
at the call site. Symbolic paths are checked at assembly time against the
actual declared TypeIdentity (local TypeId or wire schema).

For macro parsing, component names containing `.` must use the lower-level
builder rather than the path grammar. Names may be scoped via `/` only when
addressable as public module endpoints, not as private internals. In particular
`child/worker` is not an exported endpoint of the instance `child`.

## Conflicts and patch effects

`PatchBuilder` records write footprints automatically. An edge claim is an
ordered source/target pair, with optional unspecified port/slot components. A
shorthand write conservatively conflicts with an explicit write that could
resolve to the same concrete endpoint. The planner checks independent
contributions for conflicts, orders dependencies, and atomically applies the
selected edits. Each actual edit is checked against its declared footprint.

The exact pre-existing connection key is still a length-prefixed pair of
symbolic endpoints, not a physical EdgeId; its parser handles `->` inside
names without confusing it for the edge separator. The runtime sees only
the flattened compiled topology.

## Limitations

- Patch grammar is intentionally small. Arbitrary low-level `NodeTemplate`
  actions, sophisticated policies and all advanced edit kinds remain available
  through the macro-free PatchBuilder and the same PatchPlanner.
- Cross-plugin symbolic references require dependency ordering; only public
  ports of nested modules are visible to other contributions.
- No Rust toolchain was available in this environment. Test sources were added
  but compilation, `cargo test`, and no-default-feature tests are **unverified**.

## Three patches constructing one diamond

```rust
let mut planner = PatchPlanner::new();
planner.add_patch(patch! {
    id: "base";
    node a = |x: i64| x + 1;
    node d = |p: Pair| p.left + p.right, layout = Layout::<Pair>::fields();
    input "request" = a;
    output "result" = d;
    trigger "run" = ["request"];
}?)?;
planner.add_patch(patch! {
    id: "branch.b";
    after: ["base"];
    node b = |x: i64| x * 2;
    a -> b -> d.in.run.left;
}?)?;
planner.add_patch(patch! {
    id: "branch.c";
    after: ["base"];
    node c = |x: i64| x * 3;
    a -> c -> d.in.run.right;
}?)?;
let (graph, resolution) = planner.build("diamond")?;
```

Registration order may be reversed; graph nodes and edges are contributed before
the final compilation. Node statements should precede their wiring within a
patch because the builder applies that patch's edits in source order.
