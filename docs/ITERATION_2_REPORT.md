# Iteration 2 — graph-first API and responsibility consolidation

Source: Iteration 1 Rust-only standalone workspace. No Phenix kernel, plugin,
frontend, ABI, or native ticket code was reintroduced.

## Acceptance targets (defined before changes)

1. **One typed node entry:** `GraphBuilder::node(name, definition)` for local,
   portable, shared, structured, optional and fallible actions. Configuration is
   explicit and carried by `Node::wire/shared/with_layout/wire_layout/optional/fallible`.
   The low-level `add(name, NodeTemplate)` remains intentionally distinct for
   custom multi-port policies and hand-built actions; it uses the *same* compiler.
2. **One typed compiler error:** `ModuleCatalog`, `ModulePackage`,
   `GraphBuilder` and `GraphAssembly` expose one `compile(...)` or `finish()`
   operation returning `CompileDiagnostic { phase, definition, message }`.
   `compile_detailed` and `finish_detailed` are removed, not aliased.
3. **One graph/patch routing rule:** `GraphBuilder::route` takes typed or
   validated symbolic endpoints; `PatchBuilder::route` applies the same policy
   to explicit symbolic outputs, declares its footprint, and participates in
   atomic conflict resolution and assembly revisions.
4. **Behavioral responsibility preserved:** replacing a node's action does not
   erase its output-routing policy, which belongs to the graph output port.
5. **Rust-only verification:** integration tests cover behavior, declared
   compiler phases, patch conflicts, rollback, action replacement, and both
   local/wire types. No Python tests or scripts are added to the repository.

## Changes made

| Responsibility | Before | After |
|---|---|---|
| Typed node authoring | `node`, `node_wire`, `node_shared`, `node_with`, `node_wire_with`, `node_with_output` | One `node`, with `Node` options |
| Patch node authoring | `node`, `node_with`, `node_template` | `node` with same `NodeDefinition`, `add` for raw templates |
| Low-level patch replacement | `replace_template`, `replace` | `replace_node` for full template and the existing general `PatchBuilder::custom` for advanced typed edits |
| Compilation | Two error-returning APIs per builder | One `compile` returning `CompileDiagnostic` |
| Patch compile failure | `PatchError::Compile(String)` | `PatchError::Compile(CompileDiagnostic)` |
| Output routing | Typed-only builder method; custom patch + manual footprint | One typed/symbolic `route`; patch-specific first-class route |
| Patch target naming | Ambiguous `PatchTarget::route` for an *edge* pattern | Explicit `PatchTarget::connection_pattern` |
| Test trigger dispatch | Fixture accepted only numeric IDs | Fixture accepts same names/handles/IDs as the real engine |

New tests: `tests/iteration_two.rs` (5) and `tests/patch_output_routing.rs`
(5). Existing tests, macros and examples were migrated, not excluded.

**Scope:** 31 Rust files containing 311 `#[test]` declarations in the graph
crate after this iteration, compared with 29/301 in Iteration 1. Test
*declarations* are not test *passes*.

## Verification and score discipline

| Gate | Current state |
|---|---|
| Rust code and call-site migration | Source edited |
| No old typed node constructor names or dual compiler entrypoints | Static search found none |
| No Python files in active workspace | Confirmed |
| `cargo fmt --all -- --check` | **Not run** — rustfmt unavailable |
| `cargo check --workspace --all-targets` | **Not run** — Cargo unavailable |
| Full Rust tests, all features / no-default-features | **Not run** — Cargo unavailable |
| Phenix production integration | Out of scope; still disconnected |

The scores cannot be promoted on uncompiled source. The changes are candidate
improvements, contingent on passing Cargo and negative-path tests.

| Area | Previous reviewed baseline | Target | Estimated upside if verified |
|---|---:|---:|---|
| Graph model | 8.0 | 9.5 | 8.5–8.8 |
| Typed authoring | 7.5 | 9.5 | 8.5–9.0 |
| Patch composition | 7.0 | 9.5 | 8.0–8.6 |
| Compilation | 8.0 | 9.5 | 8.5–8.9 |
| Execution | 7.0 | 9.5 | 8.0–8.4 (largely unchanged from iteration 1) |
| Rust testing architecture | 7.0 | 9.5 | 7.8–8.3 |
| Production integration | 3.0 | 9.5 | unchanged; intentionally last |

## Next graph-only batch

- Run and fix all Cargo/rustfmt/Clippy failures before claiming improved scores.
- Reduce the remaining overlap between `GraphBuilder::replace_node`,
  `replace_function` and `PatchBuilder` replacements without hiding node signatures.
- Decide whether the specialized `branch` constructor belongs in one node
  definition contract that can return multi-output handles; preserve the
  macro-free low-level API.
- Provide deterministic compiled-graph identity with explicit treatment of
  closure/action identity, so hashes do not claim stronger equivalence than
  justified by the representation.
- Expand `Fixture` assertions for named output sources and exact failure
  categories, then add model-checking/property tests for patch combinations.
- **Only after that** reconnect kernel authorization, native pending/drain and
  plugin/frontends. Do not restore alternative graph schedulers.
