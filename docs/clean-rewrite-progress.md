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
