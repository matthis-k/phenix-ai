# Iteration 1 — graph-first change report

Source baseline: the Rust-only graph workspace from 2026-11-07 (prior package).
This iteration does not reconnect or modify the Phenix kernel, ABI or plugins.

## Implemented together

1. **Output policy owned by the output slot.** `OutputRouting::{Broadcast, Select}`,
   `OutputSelector`, and `GraphBuilder::route(typed_output, closure)` choose any
   subset of the output's *already compiled edges*. Selector errors, panics,
   duplicates and fabricated edges settle the producing invocation as failed.
   All selection and edge-adapter preparation occurs before the single atomic
   downstream delivery batch. No alternate scheduler or host runner was added.
2. **Coloring tracks actual deliveries.** Successful settled outcomes retain
   selected destination addresses, rather than assuming every edge belonging
   to an emitted slot was traversed. A branch that was not selected appears as
   `Closed`, not `Delivered` or `EmittedTowardReachablePort`.
3. **Stable external trigger names.** Compiled metadata indexes root trigger
   names. Both `Engine::fire` and `Engine::begin` use one `TriggerSelector`
   interface for trigger handles, names and existing low-level IDs.
4. **Compiler policy is configurable.** `CompileLimits` is supplied through
   `GraphBuilder::limits` or `ModuleCatalog::limits`, and applied consistently
   during declaration validation and module expansion. These limits do not
   constrain runtime waves or graph cycles.
5. **Rust regression tests.** Thirteen new external Rust tests plus one
   internal skipped-edge coloring test. They cover default broadcast,
   selected/drop/error/panic/forged/duplicate routing, failure isolation,
   names, numeric ingress rejection, compiler limits and cross-scope rejection.

## Gate results (do not infer green from source edits)

| Check | Status |
|---|---|
| Source-level API and caller inspection | Completed |
| No Python tests added | Confirmed |
| `cargo fmt --all -- --check` | Not run — rustfmt unavailable |
| `cargo check --workspace --all-targets` | Not run — Cargo unavailable |
| `cargo test --workspace --all-targets --all-features` | Not run — Cargo unavailable |
| `cargo test -p phenix-graph --no-default-features` | Not run — Cargo unavailable |
| Native integration, ABI, provider and frontend tests | Out of scope |

## Score tracking

Scores are **not upgraded** until the Cargo gates pass and failures are
investigated. Source-review expectations are direction-only.

| Responsibility | Before | Target | Candidate benefit | Still missing |
|---|---:|---:|---|---|
| Model/ownership | 8.0 | 9.5 | Output policy localized; exact delivery colors | Formal ownership/invariant review |
| Authoring | 7.5 | 9.5 | One typed `route` verb, named entry lookup | Node constructor family consolidation |
| Patching | 7.0 | 9.5 | No new parallel patch mechanism | Route edits through canonical patch builder |
| Compilation | 8.0 | 9.5 | Configurable limits; named triggers | One diagnostic API; stable graph identity |
| Execution | 7.0 | 9.5 | Selective fan-out; invalid routing fails closed | Authorized store inspection; hard cases and profiling |
| Rust testing | 7.0 | 9.5 | 14 regression tests | Cargo green, fixture consolidation, property tests |
| Integration | 3.0 | 9.5 | Intentionally unchanged | Last phase only |

## Next implementation batch

- Remove the duplicated `compile()` vs `compile_detailed()` error surface after
  migrating real Rust call sites; keep one structured `CompileDiagnostic`.
- Consolidate `node`, `node_wire`, `node_shared`, `node_with` and conditional
  output semantics into one config-driven low-level constructor and one
  inferred authoring entry point. Retain advanced traits; no compatibility facades.
- Unify graph/patch route edits and footprint inference. Do not introduce
  a second `GraphBuilder` or execution IR.
- Add graph identity and high-confidence conformance/property testing.
- After the core suite is independently verified, move outward to kernel
  authorization, native completion, provider binding and frontend integration.
