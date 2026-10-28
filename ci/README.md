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

## Declarative trusted push evaluation (native Tailscale four builders)

The source of truth for the native remote backend is `ci/flake-backend.nix`
combined with `ci/render-flake-workflow.nix`, using the installed
`phenix-flake-ci.mkFlakeCiWorkflow` API. It selects `backend = "nix-remote"`,
`transport = "tailscale"`, `workers = 4`, `slotsPerWorker = 2`,
`credentials = "oidc"` and an explicit owned push-branch allowlist.
The library generates the pinned workflow
`.github/workflows/phenix-flake-ci-generated.yml`; the verifier compares
it byte-for-byte with the Nix output. Do not add independently maintained
Tailscale workflow YAML or custom partition/closure handoffs.

This benchmark temporarily admits only the protected `main` push and
`ci/declarative-native-four-20261028` (plus trusted manual dispatch)
to the credentialed coordinator. Arbitrary PRs still use the separate
read-only staged-DAG check; `pullRequest = false` remains enforced by the
module itself. The policy tests reject additional push branches, workflow
pin changes, OIDC on PR and modified check inventory.

After benchmarking, the temporary trusted branch should be removed from the
policy allowlist. The eventual improvement belongs in `phenix-flake-ci`:
provide reusable first-class trusted push branch policy and independently
credential-free PR CI, with one declarative source of truth rather than
per-consumer hand-authored trigger checks.
