# Typed module projection follow-up (stacked on PR #1260)

## Phase 0 — Problem and no-change baseline

PR #1260 unifies Node and Module operation interfaces, but wrapping a child whose
input interface exactly matches a Module's public interface still requires a
separate `expose(public.field(k), child.field(k).target()?)` call for every leaf.
For a nested structural interface this is repetitive, encourages missed leaves,
and pushes boundary bookkeeping into user code.

The unchanged API remains necessary for partial mappings, renames, fan-out to
specific destinations, and non-matching compositions. This change must **not**
infer public interfaces from children or silently expose private child ports.

## Phase 1 — Design and invariants

Add one authoring-only method:

```rust,ignore
let mut module = Module::new("sum", InputInterface::single(Layout::<Pair>::fields())?)?;
let child = module.node("child", |p: Pair| p.left + p.right)?
    .input(Layout::fields())?;
module.expose_matching(child.operation().inlets())?;
let package = module.finish(child.output())?;
```

This is an explicit instruction to project the **entire** declared public input
frontier to a compatible child boundary. It is not a new Unit, runtime node,
patch kind, or scheduler feature.

The method accepts `Inlets<I, M>` for the same `I` and representation `M`
as the Module. Before any mutation, it verifies:

1. The child interface is from the same graph-builder scope.
2. The exact set of public leaf paths matches (both directions, no duplicate
   child paths or undeclared additional leaves).
3. Every leaf has the same local TypeId or canonical wire schema.
4. Every child endpoint still belongs to the current declaration revision and
   resolves through the existing graph verifier.
5. Existing public projections are compatible; projecting the same destination
   twice fails, while projecting matching interfaces of two different children
   intentionally fans out.
6. All new physical input identifiers fit within the compiler's `u16` range.

Only after the full preflight does it append all corresponding public-input
projections. The existing `Module::expose`, physical `ModuleInput` representation,
compiler and scheduler remain authoritative.

The operation remains explicit and is not run during `Module::finish`.

## Phase 2 — Adversarial critique

- **More public surface:** one helper is justified by removing per-leaf
  construction boilerplate for a frequent, well-defined composition law.
  No additional trait, marker, node, or authoring hierarchy is added.
- **Partial projection after a failed leaf:** forbidden; reject before touching
  any public-input targets. This is tested by pre-projecting one leaf to the
  same child, rejecting the bulk request, then observing that an unprojected
  second leaf still blocks `finish`.
- **Stale or foreign typed handles:** static owner typing is insufficient.
  Scope and current declaration identity are checked per destination.
- **Accidental implicit conversion:** the declared value identity must match.
  No encoding, decoding, cloning, routing or layout reconstruction is inserted.
- **Specialized interfaces:** this convenience is deliberately restricted to
  identical typed interface owners and modes. Differently shaped composition
  must continue to use explicit leaf mappings. This limitation does not block
  primary cases because manually projected composition is retained.
- **Fan-out:** sequential calls with distinct children may map each public
  leaf to multiple physical targets; identical target repetition is rejected.
- **Hidden scheduling state:** none. Modules still flatten at compile time.
- **Generalizing to partial automatic matching:** rejected for this iteration:
  silently skipping unmatched leaves would weaken an essential invariant.

## Phase 3 — Implementation and tests

Implementation: `packages/phenix-graphs/src/authoring/operation.rs`.

Regression suite: `packages/phenix-graphs/tests/matching_projection.rs` checks
structured results and physical-node count, independent child fan-out, duplicate
projection atomicity, and stale/cross-scope rejection.

This change is deliberately scoped to API ergonomics and projection preflight.
It does not address the larger follow-up work of simplifying Action adapters,
NodeHandle declaration staging, and policy/scheduler state.

## Phase 4 — Evidence and limits

This PR must remain draft until exact-head CI passes:
`cargo fmt --all --check`, workspace tests, isolated no-default-feature tests,
Clippy with warnings denied, and Nix flake checks. The code was authored through
the GitHub connector; a local Rust toolchain was unavailable in the current
environment, so local compilation and performance results are **not claimed**.

Review the final diff for unexpected compiler/runtime changes. The intended diff
changes only one typed authoring method, its tests and this design record.
