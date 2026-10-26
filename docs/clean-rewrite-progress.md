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
