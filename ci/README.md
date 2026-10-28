# Nix CI execution policy

The flake's `checks.x86_64-linux` is the single source of the complete check
inventory, including native ABI, negative/security, product, Graph V2 and
shipping acceptance. No selective test bypass is permitted.

- **Other/fork PRs**: `minimal-kernel-flake.yml` invokes the pinned
  `phenix-flake-ci` staged DAG. The same exact-head derivation graph is
  planned, high-fanout dependencies are prebuilt once, ready checks run in
  parallel, and the final dependent/shipping join verifies all results.
  It has no Tailscale/OIDC credential or publisher authority.
- **Owner PRs**: PRs authored by `matthis-k` with their head branch in
  `matthis-k/phenix-ai` use the same native Nix coordinator and four
  Tailscale builders, with no redundant per-shard store. PR publication is
  disabled, and other/fork PRs remain in the credential-free staged backend.
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

## User-facing flake CI policy

The entire execution policy is inside `ci/flake-check-config.nix` under
`ci.trusted` and `ci.pullRequest`. It is consumed by one call to
`phenix-flake-ci.mkFlakeCiWorkflows` in `ci/render-flake-workflows.nix`.

That call generates **both** `.github/workflows/phenix-flake-ci-generated.yml`
and `.github/workflows/minimal-kernel-flake.yml`. CI compares each checked-in
caller byte-for-byte against Nix output, using the same exact tested
`phenix-flake-ci` source SHA for workflow calls and library checkout.

Trusted main pushes and same-repository PRs authored by `matthis-k` use one
Nix derivation scheduler with four Tailscale ssh-ng builders and two slots
each. The author allowlist is `ci.trusted.pullRequestAuthors` in the same
Nix policy. Generated workflows contain complementary job-level admission
and fallback predicates. No manually maintained parallel CI authority is
allowed. All other PRs use the credential-free staged DAG.

The trusted caller only accepts main pushes or approved same-repo
`pull_request` events; it never uses `pull_request_target` or manual dispatch.
Keep write access to trusted PR head branches restricted. The Tailscale
federated-identity grant must authorize GitHub's pull_request OIDC subject;
a grant scoped only to main branch subjects will reject these PR jobs.
The native workflow deliberately does not publish to shared binary cache on PRs.

The temporary benchmark branch was removed after verified end-to-end run
[40671228754](https://github.com/matthis-k/phenix-ai/actions/runs/40671228754):
all 86 Nix checks, four Tailscale builders and both post-build test packages
passed in 4m17s on a warm cache. This result is not a cold-build benchmark.

Future backends and trust behavior belong in the typed library policy, not
consumer-maintained GitHub Actions YAML.

The paired API originally merged in `phenix-flake-ci` PR #37. Owner PR
routing was merged in library PR #41 and is pinned to immutable squash
commit `a0460dd78cd03478f906421844d769dddc66a69c`.
Both generated workflows
are verified byte-for-byte against that exact merged library revision.

## Automatically discovered runnable tests

The existing complete 86-check Nix DAG remains mandatory. As an additional
post-build test mechanism, generic `phenix-flake-ci` discovers flat
`packages.x86_64-linux.\"test.<name>\"` packages. The native Nix coordinator
builds their exact output closures with four Tailscale builders, then
GitHub Actions automatically fans out one read-only job per runnable test.
Final acceptance verifies each test's exact-head evidence.

`flake.nix` exposes `test.basic-composition-smoke` and
`test.kernel-composition-inspect` as thin scripts consuming the **existing**
`basicRuntime`, `kernel`, and immutable `basicComposition`; they do not
build or embed additional plugin copies. Composition semantics (bindings,
authority, selected packages) remain Phenix-owned.

The optional source-preparation convenience command accepts repeatable
`--plugin SOURCE` flags:

```sh
python3 tools/phenix-plugin-sources.py profile.json /tmp/selected.json \\
  --plugin ./plugins/provider --plugin ./plugins/consumer
```

The tool generates the same canonical `phenix.composition.v3` JSON with
resolved package paths, and **does not infer** authority, grants, or bindings.
Plugin paths resolve relative to the input configuration file.

This is an opt-in migration: old Nix checks still run, and `test.*` packages
are additional separately executed post-build tests. Cargo workspace tests
remain in the existing complete check DAG until a compiled-test-artifact
adapter provides strict no-recompile separation.
