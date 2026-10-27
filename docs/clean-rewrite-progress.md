# Clean rewrite progress

This file records **implementation evidence**, not proposed acceptance. `main` continues to own the working P1 product.

| Step | Before | Change | Evidence | Remaining |
| --- | --- | --- | --- | --- |
| R0 reference | 29 legacy `phenix-plugin-*` crates on `31f86f454fd4cd3316379b5802b02ba19ecae69c` | Pin `reference/pre-clean-plugins-20261026`; fork `rewrite/graph-v2-clean-plugins-20261026` | Git reference branch created at exact baseline | Confirm rewrite HEAD/CI separately |
| R1 purge | Runtime plugin implementations and product wiring coupled to graph-v1 | Delete all 29 plugin directories **only on rewrite branch**; constrain Cargo workspace to core and two small fixture plugins | Git tree diff after commit; compilation **not run yet** | Old `flake.nix` and shipped P1 targets still refer to purged plugins; rewrite is **not release-ready** |
| R2 first fixtures | No new small plugins in rewrite | Add stateless Model fixture and one stateless Unicode-reverse tool contributor, using public typed contracts | Unit tests and ABI metadata tests authored; execution **not run yet** | Run Cargo and native loading, then a minimal graph composition |
| R3 graph-v2 | Current kernel is graph-v1 | No graph-v2 executable change yet | **Not started** | Implement typed V0/V1 graph delivery before domain migrations |

## Verification commands

```sh
cargo test --workspace
cargo test -p phenix-plugin-model-fixture -p phenix-plugin-tool-reverse
# Do not run 'nix flake check' as a claim of P1 parity on this rewrite:
# the legacy flake references deliberately purged plugin crates.
```

Before advancing R1/R2 to verified, test the checked-out exact rewrite SHA; rebuild the lockfile if Cargo requires it, commit the lockfile change, and rerun at that SHA. Next: native ABI loading of both fixtures, typed Source/Invoke/Flow/Invoke, and a credential-free end-to-end scenario. Document any compile failure before attempting a fix.

## Boundaries

- No runtime changes on `main` in this purge.
- No preservation of a redundant old plugin API for the rewrite.
- No claim that unit tests cover real native composition.
- The reference branch is read-only evidence for behavioral comparisons.

## 2026-10-26 R2 format gate

- Workflow: [first clean Rust run](https://github.com/matthis-k/phenix-ai/actions/runs/40395088816), commit `355871dde43fb204d5a842e34ba180f781a533a2`.
- **Failed** at `cargo fmt --all -- --check`; three formatting differences in the new fixture Rust sources. Cargo tests and native build did not run on that head.
- Corrected the three differences in the next commit. Their success remains **unverified until its own run finishes**.

## 2026-10-26 R3 module-boundary design

- **Before:** graph-v2 defines individual nodes and named graph exports; the clean domain plan lacks a clear standard-vs-advanced interface for nested subgraphs.
- **Design change:** add v2.4 §2.4 and §7.4: stable Entry/Exit module interface, accessible internal graph IDs, explicit advanced patch grants, offline lowering, checked version preconditions, and tests. Link from clean rewrite plan. No new native plugin type.
- **Implementation:** specification only. Native module compiler and tests **not implemented**.
- **Verification:** no module-specific executable tests exist; confirm documentation diff and existing clean workspace CI separately.
- **Next:** implement module IR and typed boundary lowering as part of graph-v2 V0/V1, test interface-only composition and authorized internal replacement before adding domain managers.

## 2026-10-27 R4 legacy non-selected framework purge and author UX target

- **Before:** the active Cargo workspace contained eight crates, but 18 retired implementation and helper crates still occupied the rewrite tree despite not being build-selected.
- **Change:** remove 120 tracked files under 18 unused crates; keep only the ABI/kernel/SDK/contracts and two tiny fixtures. Source preserved at reference commit `31f86f4`.
- **Top-down target:** document usage-first plugin call sites and default Basic, provider, agent, tools, memory and compaction behaviors in `docs/plugin-author-ux.md`. Document repeated design/implementation/feedback cycles in `docs/design-implementation-cycles.md`.
- **Domain logic:** port algorithmic pieces from the reference as needed; do not retain its framework or import the retired crates for convenience.
- **Verification:** Git tree purge verified by diff; compiling tests for this exact commit are initially not run. The legacy flake remains intentionally incompatible with this rewrite.
- **Next:** implement only the canonical graph-v2 minimal module/Flow authoring slice and run author-facing fixture tests, then native selected dispatch.


## 2026-10-27 R5 source-owned types, signatures and State

- **Before:** two clean fixtures depended on externally defined P1 contracts and did not demonstrate locally owned ABI contract/types with private Rust State.
- **Design:** graph architecture v2.4 D35 and §1.2 establish one plugin as owner of topology, private State, operation signatures and domain types. Generated ABI and contract metadata reuse those declarations; no duplicate author-facing schemas.
- **Implementation:** counter fixture with local Portable request/response/error, two contract operations, private State, atomic checked transition and generated native metadata; Cargo and native CI updated to include three fixtures.
- **Verification:** first clean CI run failed at Rust formatting before tests; format corrected in next commit. Exact-head unit/build and native ABI invocation outcomes must be recorded separately.
- **Remaining:** no compiling graph-v2 topology declaration from plugin-owned operations, no native composed graph test, no P1 equivalence.

## 2026-10-27 C1 first bottom-up/feedback loop

- **Top-down call site:** `Graph::module`, `invoke`, `source`, `entry`, `exit` and typed `flow` with stable logical module boundaries. Author-visible internal IDs remain accessible for advanced patch targeting.
- **Implementation:** `crates/phenix-contract/src/graph_v2.rs` provides V0 structural graph declarations and validation; `crates/phenix-sdk/src/graph.rs` provides typed authoring and tests. This is not yet a portable ABI descriptor or native graph-v2 executor.
- **Discovered:** use M1's `valid_node_key` for qualified `plugin/node` identities. The first V0 build compiled, but tests revealed the mistake at `52324d37` (four SDK failures). Corrected at `3f059c33`.
- **Evidence:** `cargo fmt --all -- --check`, `cargo test --workspace`, and native fixture builds passed at `3f059c33`: [CI run](https://github.com/matthis-k/phenix-ai/actions/runs/40424288316). Earlier formatter check failed at `585cad9c` and tests failed at `52324d37`.
- **Top-down simplification:** the public `Graph::flow` now returns `()` because the typed edge cannot fail during declaration. Module-local `flow` retains a Result for cross-owner mistakes. Stable edge IDs are derived from logical port names rather than Rust Debug output; unknown `internal` nodes return an error instead of a fabricated ID.
- **Unverified at this update:** exact-head CI for the ergonomics follow-up; native graph-v2 invocation, selected grants, closure/wave semantics and P1 parity remain unimplemented.

## 2026-10-27 R6 local source graph topology and verification

- **Before:** the new Counter plugin owned its Portable types, contract operations and Rust State, but graph-v2 topology was not yet authored by that same plugin.
- **Change:** define `topology()` within `phenix-plugin-counter-fixture` using the actual `phenix_sdk::graph::Graph` V0 authoring API. Its ingress Source, Counter module Invokes and entry/exit ports derive schemas and operation keys from local Rust types and `Counter::increase/read`. Add offline IR and lowered-flow tests.
- **Discovered:** one test assertion required standard rustfmt wrapping. Initial CI failed at formatting, corrected in follow-up commit.
- **Verified:** commit `6fe95f96a2532500084967c2554315f26674683d`: `cargo fmt --all -- --check`, `cargo test --workspace`, and build/existence of three native cdylibs passed in [clean Rust CI](https://github.com/matthis-k/phenix-ai/actions/runs/40425100834).
- **Scope limit:** this is **plugin-owned graph authoring and structural validation**, not portable graph-v2 descriptor packaging, selected native Flow execution, or live P1 end-to-end acceptance. The ABI contract and private State parts have independent fixture tests.
- **Next cycle:** derive/emit offline graph-v2 metadata from the plugin-owned topology without a manual second descriptor; then compile/admit selected operations and test root/grants/pending/cancellation and parity against the reference.
