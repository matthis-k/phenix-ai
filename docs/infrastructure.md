# Nix Infrastructure

`checks.x86_64-linux` is the authoritative CI inventory. The pinned
`phenix-flake-ci` matrix backend discovers it at the checked-out revision;
`ci/flake-check-config.nix` controls scheduling, not a second list of checks.

## Outputs

| Check | Contract |
| --- | --- |
| `format` | Workspace rustfmt and infrastructure nixfmt checks |
| `clippy` | Whole workspace, all targets, warnings denied |
| `storage` | Storage semantic tests, debug assertions enabled |
| `graph` | Graph and plugin semantic tests, debug assertions enabled |
| `kernel` | Kernel library and lifecycle tests, debug assertions enabled |
| `integration` | Integration workspace packages when present; otherwise kernel/storage integration target |
| `kernel-smoke` | Build, install, and execute the release kernel example |
| `workflow` | Generated GitHub caller matches the committed file |

`packages.x86_64-linux.kernel-smoke` installs `bin/kernel-smoke`, not Rust
compiler intermediates. There is intentionally no default package. The smoke
program exercises plugin registration, kernel startup, and graph execution.

## Inputs And Reuse

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
Integration package names must contain `integration`. The current dependency
reader expects direct dependency tables, not target-specific or inherited
workspace dependency declarations; extend it if those are introduced.
Fixtures are retained without an extension allowlist. All nested `target`
and `.git` directories are excluded. Documentation and CI edits do not
invalidate semantic tests. Shared lockfile changes conservatively invalidate
Rust checks. Changes to another member's manifest only affect checks whose
closure or generated workspace policy actually changes.

Run `python3 ci/verify-invalidation.py` to compare real derivation paths after
isolated source mutations. It never edits the working tree. The test verifies
documentation independence and graph/storage source isolation.

## Workflow Maintenance

Upstream API documentation:

- https://github.com/matthis-k/phenix-flake-ci/blob/e83c6029afc0140a242ae8240dbff6efe5979725/docs/ci-backends.md
- https://github.com/matthis-k/phenix-flake-ci/blob/e83c6029afc0140a242ae8240dbff6efe5979725/README.md

`lib.flakeCiWorkflow` uses the documented `mkFlakeCiWorkflow` API with the
locked input's `rev` for both reusable-workflow references. After changing
the policy or pin, regenerate `.github/workflows/flake-ci.yml` from
`nix eval --raw .#lib.flakeCiWorkflow`. The workflow synchronization check
guards against drift. No remote builders or credentials are configured.

Local verification:

```sh
nix flake check
nix build .#kernel-smoke
./result/bin/kernel-smoke
nix shell --inputs-from path:. nixpkgs#python3 -c python3 ci/verify-invalidation.py
```

For newly created, untracked infrastructure files, use `path:.` instead of
`.` while evaluating locally; Git flakes do not include untracked files.
Local success does not establish remote CI cold/warm cache timings or
GitHub evidence-gate success. Those require actual workflow runs.
