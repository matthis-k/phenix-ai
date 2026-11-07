# Iteration 4 — graph-first inspection, one assertion API, actual call-site repairs

## Scope and targets

Baseline: `phenix_graph_iteration_3_20261107.zip` (the supplied Iteration 3 workspace).
No kernel, ABI, frontend, or native plugin integration changes. No Python tests or scripts.

| Responsibility | Problem found in baseline | Acceptance target | Change |
|---|---|---|---|
| Graph compilation / inspection | Root public ports and nested module boundaries invisible in `topology_manifest()` | Distinct public interfaces must produce distinct descriptors | Include canonical, named root and instance input/output mappings |
| Graph inspection | Every unnamed endpoint was rendered as `#unnamed`, losing its actual identity | Two different unnamed slot destinations must not compare identical | Include the local numeric port/slot ID as fallback |
| Readiness | `AllSameCohort` and `AnyOne` were invisible in topology output | Distinct policy types must be inspectable | `PortPolicy::diagnostic_type()`, also delegated through `Arc<P>` |
| Test API | `expect_local` and `expect_wire` silently selected the *last* publication | One unambiguous, source-specific assertion API | Removed ambiguous methods; migrated their Rust callers to `expect_local_at` / `expect_wire_at` |
| Test semantics | `expect_local_at` / `expect_wire_at` silently selected the last of multiple publications *from the same output* | Source-specific value assertion requires exactly one retained publication | Reject multiple matching events; keep `expect_publication_count` for multiplicity |
| Graph assembly API | Both `GraphAssembly::finish()` and `GraphAssembly::compile()` could finalize assembly | One finalization operation preserving provenance | Removed `GraphAssembly::compile()` alias |
| Call-site correctness | Host-driven fixture declared trigger `run` but fired `go` | Fire a real declared trigger | Corrected test to `fire("run", ...)` |
| Example correctness | The patch documentation passed a bare closure to `replace`, which requires a `Replacement` | Examples must use real public Rust APIs | Use `NodeTemplate::local_function`; dispatch by named trigger and actual output source |

## Source-exact before / after

These snippets come from the real Iteration 3 source and modified Iteration 4 source, not mock API designs.

```rust
// BEFORE — crates/phenix-graph/tests/testing_api.rs
fixture.fire("go", vec![Value::local(20_i64)]).unwrap()
    .expect_success().expect_local(&42_i64);

// AFTER — crates/phenix-graph/tests/testing_api.rs
fixture.fire("go", vec![Value::local(20_i64)]).unwrap()
    .expect_success().expect_local_at(fixture.output("second", "value"), &42_i64);

// BEFORE — crates/phenix-graph/tests/testing_api.rs
fixture.fire("go", vec![Value::local(10_i64)]).unwrap()
    .expect_success().expect_local(&11_i64);

// AFTER — crates/phenix-graph/tests/testing_api.rs
fixture.fire("run", vec![Value::local(10_i64)]).unwrap()
    .expect_success().expect_local_at(fixture.output("increment", "value"), &11_i64);

// BEFORE — crates/phenix-graph/docs/ASSEMBLY_COMPILATION_EXECUTION.md
extension.after("agent.base")
    .replace("output", |v: i64| v * 3);

// AFTER — crates/phenix-graph/docs/ASSEMBLY_COMPILATION_EXECUTION.md
extension.after("agent.base")
    .replace("output", NodeTemplate::local_function(|v: i64| v * 3));
```

## Rust regression tests

New file: `crates/phenix-graph/tests/iteration_four.rs`.

- Root public-input and public-output renaming affects the structural descriptor.
- Different port policy types are visible without claiming policy behavior identity.
- Strict assertion accepts exactly one named publication.
- Expected panic when two publications from the same output would previously pass a last-value assertion.
- Two unnamed physical slots have distinguishable descriptor paths.
- Nested module public input renaming survives expansion in the structural descriptor.
- Documented patch replacement and named-trigger execution produce the expected output using the production engine.

The existing Rust tests are retained; no new scheduler or alternative graph IR was introduced.

## Ownership decisions

- **GraphBuilder / ModuleCatalog:** declarations, public module boundaries, expansion.
- **CompiledGraph:** structure and provenance inspection. The manifest is **not** a stable executable identity, not a security hash, and cannot capture policy configuration or opaque closure bodies.
- **PortPolicy:** one diagnostic type method; it does not own identity or serialization.
- **GraphAssembly:** one finalization verb (`finish`) that returns compiled graph *and* provenance.
- **Fixture / ExpectWave:** real engine execution plus exact observational assertions.
- **Deployment / Engine:** unchanged; production graph execution remains single-path.

## Verification and remaining friction

Completed: diff whitespace check, artifact integrity, static reference review, Rust-only source/test checks.

**Not completed:** `cargo fmt`, `cargo check`, `cargo test`, `cargo clippy` — Cargo/rustc are absent and network DNS is unavailable. Therefore **do not promote any numerical architecture score as verified**.

Remaining priority order:

1. Compile with Rust 2024 and fix baseline/iteration errors; run all Rust tests and no-default-feature tests.
2. Establish explicit, stable action/policy identity before any executable generation hash. Captured closure state must never be silently equated.
3. Continue narrowing `GraphBuilder` / patch-edit overlap and low-level API surface, only when passing tests confirm equivalence.
4. Review runtime cancellation, multiple waves, edge-output policy corner cases and conformance coverage.
5. Reconnect kernel authorization, native operations and plugin boundaries **last**.
