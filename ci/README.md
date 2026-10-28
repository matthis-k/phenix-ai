# Nix CI execution policy

The flake's `checks.x86_64-linux` is the single source of the complete check
inventory, including native ABI, negative/security, product, Graph V2 and
shipping acceptance. No selective test bypass is permitted.

- **Untrusted PRs**: `minimal-kernel-flake.yml` invokes the pinned
  `phenix-flake-ci` staged DAG. The same exact-head derivation graph is
  planned, high-fanout dependencies are prebuilt once, ready checks run in
  parallel, and the final dependent/shipping join verifies all results.
  It has no Tailscale/OIDC credential or publisher authority.
- **Trusted main**: the generated CI caller retains four Tailscale Nix
  builders with two slots each, and trusted cache publication.
- **Manual diagnosis**: `minimal-kernel-dag.yml` can run the original
  single-store full Nix DAG without blocking ordinary PRs.

Historical successful runs (different inventories/cache states):
- [13 October staged PR DAG](https://github.com/matthis-k/phenix-ai/actions/runs/38379399241): 17m06s, 37/37 checks.
- [13 October trusted four-builder](https://github.com/matthis-k/phenix-ai/actions/runs/38379370033): 7m29s.
- [27 October trusted four-builder](https://github.com/matthis-k/phenix-ai/actions/runs/40503332187): 16m24s.
- [28 October single-runner full DAG](https://github.com/matthis-k/phenix-ai/actions/runs/40612620171): roughly 64m.

The target is 10–15 minutes, not a verified result for current Graph V2.
Compare exact-head plan/output coverage and observed timings; do not
claim a speedup, skip failed checks, or expose trusted secrets to PR code.
