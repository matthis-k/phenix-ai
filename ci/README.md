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

## User-facing flake CI policy

The entire execution policy is inside `ci/flake-check-config.nix` under
`ci.trusted` and `ci.pullRequest`. It is consumed by one call to
`phenix-flake-ci.mkFlakeCiWorkflows` in `ci/render-flake-workflows.nix`.

That call generates **both** `.github/workflows/phenix-flake-ci-generated.yml`
and `.github/workflows/minimal-kernel-flake.yml`. CI compares each checked-in
caller byte-for-byte against Nix output, using the same exact tested
`phenix-flake-ci` source SHA for workflow calls and library checkout.

Trusted pushes use one Nix derivation scheduler with four Tailscale ssh-ng
builders and two slots each; no custom shared-build frontier or NAR joins.
Untrusted pull requests run the credential-free complete staged DAG.

The additional `ci/usage-first-paired-workflows-20261028` is a **temporary benchmark allowlist**
beside protected `main`. The generated remote job guards the exact
repository, allowed refs and event, and has no PR trigger or default manual
dispatch. Branch naming alone is not sufficient to grant trust: GitHub
repository write permissions, branch protections and Tailscale ACLs govern
whether a ref is appropriate. Remove the benchmark branch after acceptance.

Future backends and trust behavior belong in the typed library policy, not
consumer-maintained GitHub Actions YAML.

The paired user-facing API is merged in `phenix-flake-ci` PR #37 and
pinned to its immutable squash commit `070cb93f5972147ab0ed6efd1019e11ef6112a93`. Both generated workflows
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
