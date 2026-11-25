# Maintain Nix CI

The flake's `checks.x86_64-linux` defines the check inventory. List the current checks with `nix flake show`. The pinned `phenix-flake-ci` discovers that inventory; `ci/flake-check-config.nix` controls scheduling.

## Dependency reuse

Each Rust check receives only its transitive Cargo path-dependency source
closure, including dev/build dependencies. Workspace members are narrowed in
a generated root manifest. Offline Cargo metadata prunes local lock records
before locked commands execute; registry dependencies come exclusively from
the shared `importCargoLock` vendor derivation. Tests use the debug profile;
only the smoke artifact uses the release profile. Compiled Cargo target
directories are not shared across independent checks. The vendor derivation
is shared, but this is not a claim of shared compiled Rust dependencies.

New explicit workspace members and direct path dependencies are discovered
from Cargo manifests, including new shared schema and integration crates.
The dependency reader expects direct dependency tables. Extend it before adding target-specific or inherited workspace dependencies.
Fixtures are retained without an extension allowlist. All nested `target`
and `.git` directories are excluded. Root documentation does not enter Rust source closures. Documentation inside a crate can affect that crate's checks. Shared lockfile changes conservatively invalidate
Rust checks. Changes to another member's manifest only affect checks whose
closure or generated workspace policy actually changes.

Run `python3 ci/verify-invalidation.py` to compare real derivation paths after
isolated source mutations. It never edits the working tree. The test verifies
documentation independence and graph/storage source isolation.

## Regenerate the workflow

Read the pinned [CI backend documentation](https://github.com/matthis-k/phenix-flake-ci/blob/e83c6029afc0140a242ae8240dbff6efe5979725/docs/ci-backends.md) and [flake CI README](https://github.com/matthis-k/phenix-flake-ci/blob/e83c6029afc0140a242ae8240dbff6efe5979725/README.md) before changing the workflow.

`lib.flakeCiWorkflow` uses the documented `mkFlakeCiWorkflow` API with the
locked input's `rev` for both reusable-workflow references. After changing
the policy or pin, regenerate `.github/workflows/flake-ci.yml` from
`nix eval --raw .#lib.flakeCiWorkflow`. The workflow synchronization check
guards against drift. No remote builders or credentials are configured.

## Run checks locally

Run the relevant checks:

```sh
nix flake check
nix build .#kernel-smoke
./result/bin/kernel-smoke
nix shell --inputs-from path:. nixpkgs#python3 -c python3 ci/verify-invalidation.py
```

For newly created, untracked infrastructure files, use `path:.` instead of
`.` while evaluating locally; Git flakes do not include untracked files.
