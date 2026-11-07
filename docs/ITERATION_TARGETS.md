# Graph-first quality climb — iteration 1 (2026-11-07)

Only the Rust graph library is within the current change boundary. Native plugin
loading, ABI authority and product/frontends stay disconnected. One execution
engine (`Engine`/`ExecutionSession`) is used by production and tests.

| Responsibility | Before (review) | Target | First-batch acceptance |
|---|---:|---:|---|
| Graph model/ownership | 8.0 | 9.5 | Output policy belongs to producing output, no duplicate scheduler |
| Graph creation | 7.5 | 9.5 | Typed `route(output, f)`; existing `connect` and `trigger` preserved |
| Patching | 7.0 | 9.5 | No added planner or graph IR; integrate routing edits in later batch |
| Compilation | 8.0 | 9.5 | Named triggers and explicit, consistently applied compile limits |
| Execution | 7.0 | 9.5 | Select any subset of existing output edges; invalid/duplicate/panic fail closed before delivery |
| Rust testing | 7.0 | 9.5 | Thirteen Rust integration tests + one internal regression for precise delivery colors |
| Integration | 3.0 | 9.5 | Deliberately last, unchanged |

## Acceptance gates

1. `cargo fmt --all -- --check` and `cargo test --workspace --all-targets`.
2. `cargo test -p phenix-graph --no-default-features`.
3. A routed output cannot emit to a non-connected edge, duplicate an edge,
   or partially deliver when its selection fails.
4. Named trigger lookup must resolve from compiler metadata, without introducing
   a second trigger scheduler; numeric IDs are still usable internally.
5. Compiler depth/instance/work limits must be enforced in validation and
   expansion, and should not impose a runtime execution budget.

## Next batches (Iteration 2 addresses the first three)

- Unify node declaration families; remove overlapping authoring entry points.
- Unify `GraphBuilder` and `PatchBuilder` edit vocabulary and infer edit footprints.
- Replace dual `compile`/`compile_detailed` error APIs with a single diagnostic.
- Introduce deterministic compiled-graph identity and regression tests.
- Make test fixtures assert source-specific outputs and exact failure kinds.
- Only then reconnect native operations, provider selection and external cancellation.

Status: Rust toolchain not present in this environment; the above Cargo
acceptance gates are specified but have **not been executed**. The batch is
source-level implementation awaiting verification. No score is promoted based
only on these edits.

Iteration 2: see `docs/ITERATION_2_REPORT.md`; all Rust acceptance gates remain unexecuted in this environment.
