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

The staged PR shared prebuild additionally restores compatible verified-main
Nix outputs read-only from `remote-phenix-m1-*` before building new
PR-specific derivations, while its primary key and all leaf/join caches
remain `phenix-m1-*` and cannot publish to the trusted namespace.
The cache restoration is merged in `phenix-flake-ci` PR #35 and pinned to its immutable squash commit.

## Native Tailscale remote CI for owned push branches

`.github/workflows/trusted-native-nix.yml` uses the previously verified
`phenix-flake-ci` Nix coordinator with **four** Tailscale ssh-ng builders
(two build slots per worker), triggered only by direct pushes to `ci/**` or
`review/**` on `matthis-k/phenix-ai`, or manual dispatch from those refs.
It is **not** a pull-request or pull-request-target workflow. The authorize
job checks the event, repository, and ref before enabling the Tailscale
backend; the reusable workflow retains full exact-head DAG checks, negative
and shipping acceptance, and trusted-only cache publishing.

Fork PRs still use the credential-free staged PR gate. Do **not** remove or
mark that gate optional until an independently verified exact-head push
acceptance check is installed and required for internal branches. The
main-only generated production remote workflow is unchanged.
