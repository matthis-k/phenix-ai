# Iteration 3 — unified replacement, conditional-node authoring, traceable outputs

## Acceptance targets established before editing

1. **One regular node entry:** `GraphBuilder::node(...)` must also create
   conditional two-output nodes. A specialized GraphBuilder branch constructor
   and specialized BranchHandle must not remain public alternatives.
2. **One replacement verb:** `replace(name, replacement)` for a node template or
   a module package. Patch contributions must use the exact same operation,
   correctly infer Node-vs-Instance conflict footprints, and retain atomicity.
   Public user implementations must not be able to forge replacement claims.
3. **Structural reproducibility without false identity:** equivalent compiled
   topology must yield the same readable manifest. Changing edges must alter
   it. Changing an opaque closure must NOT be represented as a claimed
   generation/behavior identity.
4. **Source-specific Rust tests:** the shared Fixture must resolve node/output
   names and support typed negative-path assertions plus exact output-source
   and publication-count checks, including metadata-only reporting.
5. **Coverage remains Rust-native:** migrate existing branching and
   replacement call sites, preserve original Rust suites, add external-crate
   acceptance tests; do not introduce any Python scripts or a test scheduler.

## Implemented changes

| Ownership boundary | Before | After |
|---|---|---|
| Conditional outputs | `GraphBuilder::branch[_named]`, separate `BranchHandle` | `GraphBuilder::node(..., Node::branch[_named](...))`, normal `NodeHandle::local_output<T>(name)` |
| Node replacement | `replace_node`, `replace_function` | `replace(name, NodeTemplate)` |
| Module replacement | `replace_instance` | `replace(name, ModulePackage)` or `replace(name, &ModulePackage)` |
| Patch replacement | Node-only `replace_node` | `PatchBuilder::replace` with sealed `Replacement` contract and automatically inferred write claim |
| Macro replacement | Separate/incorrect closure lowering | The optional `patch!` uses `NodeTemplate::local_function` and the same patch verb |
| Internal node type | Private physical `Node` name collided with public authoring `Node` | Private `PhysicalNode`, leaving one public `Node` definition namespace |
| Topology comparison | No compiled reproducibility descriptor | `CompiledGraph::topology_manifest()`, explicit topology-only guarantee |
| Test reporting | Mostly inspect last output / string-formatted errors | Named `Fixture::output`/`node_id` and `ExpectWave::expect_local_at`, `expect_wire_at`, `expect_publication_count`, `expect_error` |

`tests/iteration_three.rs` adds nine external Rust tests (one derive-gated) and existing branching,
patch and agent-loop tests were migrated. The compile path, policy runtime,
executor, store, native bridge and plugin integration remain unchanged.

## Responsibility decisions

- **GraphBuilder** owns node/module declarations and interface-checking replacements.
- **NodeTemplate** is the low-level escape hatch for custom ports and actions;
  it does not introduce another graph or implementation of the compiler.
- **PatchPlanner** owns patch ordering/conflicts; `PatchBuilder` generates the
  right node/instance footprint without having to implement interface checks.
- **CompiledGraph** exposes structural metadata, never pretends to serialize
  `Fn`/`FnMut` bodies, policy closures, host permissions or Rust TypeIds.
  This manifest must NOT be hashed and used as the kernel's generation ID.
- **Fixture** uses production Deployment/Engine; its assertions are observational
  only, including `WaveError` pattern matching.

## Verification status

Rust/Cargo/rustfmt are not installed here and network access to download them
is unavailable. **No Rust test passes, compiler success or formatting success
are claimed**. Source-level checks verify the removed public method names and
show no Python sources inside the workspace. Tests should be run on a Rust
2024 host with:

```sh
cargo run -p xtask -- verify
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

The next iteration must prioritize fixing compiler/test failures from the real
Rust toolchain, then decide whether to narrow the remaining low-level module
builder wrappers and add explicit executable action/policy identities *before*
production generation hashing. Kernel, plugins and frontend integration stay last.

## Provisional scores — contingent on compiler verification

| Responsibility | Previous | Candidate after verification | Target |
|---|---:|---:|---:|
| Core graph ownership | 8.0 | 8.7 | 9.5 |
| Graph authoring | 7.5 | 9.0 | 9.5 |
| Patch composition | 7.0 | 8.7 | 9.5 |
| Compilation | 8.0 | 8.7 | 9.5 |
| Execution | 7.0 | 8.2 | 9.5 |
| Rust test ergonomics | 7.0 | 8.5 | 9.5 |
| Native/plugin integration | 3.0 | 3.0 | 9.5 (last) |

All candidate figures are **predicted API-quality scores**, not earned scores.
