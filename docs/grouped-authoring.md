# Grouped Endpoint Authoring

R12 uses Graph's existing `(port, slot)` endpoint identity for every grouped
input. A nested field path is the complete slot name, not a new endpoint-string
grammar. R14 keeps provider/consumer code on the existing builder and owned
`Input<T>`/`OutputRef<T>` handles. There is no new owner, adapter module, scheduler,
or compatibility layer.

## Before

Provider exports repeated raw slot strings, even when typed field descriptors
already existed:

```rust,ignore
let left = Request::SLOT_PAIR.then(Pair::SLOT_LEFT);
graph.expose_input_slot("request", "pair.left", node.field(left))?;
```

Consumers repeated the same strings and often introduced an unnecessary scope
to release the borrowed binding:

```rust,ignore
let (left, right, result) = {
    let bound = graph.inspect("selected")?.bind_contract(&contract)?;
    (
        bound.local_input::<i64>("request", "pair.left")?,
        bound.local_input::<i64>("request", "pair.right")?,
        bound.local_output::<i64>("result")?,
    )
};
let left = graph.expose_input_slot("request", "pair.left", left)?;
let right = graph.expose_input_slot("request", "pair.right", right)?;
```

`graph.connect("source", "selected.in.request.pair.left")` is not an
alternative for arbitrary module pairs: punctuation cannot unambiguously split
literal port and slot names. This symbolic shorthand has not been extended.

## After

Provider authoring uses the same typed selector for the exported name and its
destination. The destination's leaf type must match the selector in Rust:

```rust,ignore
let left = Request::SLOT_PAIR.then(Pair::SLOT_LEFT);
graph.expose_input_field("request", left.clone(), node.field(left))?;
```

Consumers bind directly and project owned endpoints. Rust ends the graph borrow
after the last use of `bound`; no block, tuple, explicit drop, or binding clone is
required:

```rust,ignore
let bound = graph.bind_contract("selected", &contract)?;
let left = bound.local_field("request", Request::SLOT_PAIR.then(Pair::SLOT_LEFT))?;
let right = bound.local_field("request", Request::SLOT_PAIR.then(Pair::SLOT_RIGHT))?;
let result = bound.local_output::<i64>("result")?;
let left = graph.expose_input_field("request", Request::SLOT_PAIR.then(Pair::SLOT_LEFT), left)?;
let right = graph.expose_input_field("request", Request::SLOT_PAIR.then(Pair::SLOT_RIGHT), right)?;
graph.expose_output("result", result)?;
graph.trigger("run", [left, right])?;
```

`ModuleHandle::local_field` and `ModuleHandle::field` select local and wire
exports respectively. `BoundAuthoringUnit::local_field` and `wire_field` project
only endpoints promised by the contract. `NodeHandle::field` and `wire_field`
select local and wire layout fields without raw slot lookup. All delegate to the existing exact
slot lookup and identity checks. `SlotField::then`/`SlotPath::then` retain the
root and leaf types across nested fields. Descriptors can also be authored
manually with `SlotField::new`; derives are optional and do not certify that a
field exists. Nonexistent/malformed paths and wrong type/mode still fail against
the actual declaration. Local and wire identities are never coerced.
Descriptor `Copy`/`Clone` implementations place no corresponding bounds on their
phantom owner or leaf types; reusing metadata never requires cloning domain values.

For contributions that did not retain export handles,
`graph.public_input_slot("request", "pair.left")?` returns the exact public
input for trigger authoring. `public_input("request")` still means the literal
`("request", "value")` pair, not an arbitrary slot or a grouped barrier. Public
routing-key encoding is not exposed as an authoring convention.

## Executable Evidence

- `cargo run --locked -p phenix-graph --example grouped_contract`: macro-free
  provider and contract-restricted consumer, independent grouped routes, no
  extra ownership wrappers. Also runs with `--no-default-features`.
- `cargo test --locked -p phenix-graph --test grouped_authoring`: derived nested
  `SlotPath`, two module levels, contract checks, noncommutative execution,
  duplicate connect/export, disconnect/reconnect, malformed and wrong-type
  selectors, and exact public-slot trigger lookup.
- `cargo test --locked -p phenix-graph --no-default-features --test grouped_wire_authoring`:
  manually declared wire layout and selectors, nested exports, promised-only
  projection, wrong local/wire mode and type rejection, and subtraction.

Grouping alone adds no readiness policy. Local layouts that take owned fields
must not retain intermediate payload owners; the local execution regression
uses `ReportRetention::TerminalOutputs`. Wire layouts decode their own values
and can retain payloads. Existing scope, declaration-revision, replacement,
privacy, duplicate-edge, and final compiler checks remain authoritative.

## Verification

Locally verified for this change:

- Full `cargo test --locked -p phenix-graph --quiet`, including doctests.
- `cargo clippy --locked -p phenix-graph --all-targets -- -D warnings` through
  `nix develop`.
- No-default-feature `grouped_wire_authoring` and `canonical_interfaces` tests.
- The executable example with and without default features.
- Targeted rustfmt check and `git diff --check`.

The full no-default-feature suite was attempted but is blocked by ungated derives
in the separately added `tests/portable_codec.rs` (`ContractValue`, `PortArgs`,
`PortLayout`, and `LocalPortLayout` macros are absent in that configuration).
That test file was not changed by this authoring task.
