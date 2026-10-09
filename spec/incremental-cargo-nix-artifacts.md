# Dependency-derived Nix artifact closures

status: partial
stage: incremental-CI artifact granularity
depends-on: #738
ownership:
  - phenix-flake-ci generic impact and provenance APIs
  - phenix-ai Rust/Nix package derivations

## Problem and baseline

PR #738 selects affected CI jobs from actual Cargo path dependencies. That is
**job-level** selectivity, not artifact-level incrementality. Several packages
in `modules/package-sets.nix` use `pkgs.lib.cleanSource ../rust`, so an edit in
an unrelated crate changes the source derivation for every output. The
`phenix-product-rust-artifacts` derivation also starts from a dependency
skeleton and touches all first-party Rust sources before compilation. As a
result, a cache hit for third-party dependencies does not preserve already
compiled first-party crate artifacts across source revisions.

## Single sources of truth

- Cargo manifest and lock metadata define crate identities, path dependencies,
  target and feature resolution. Never maintain a hand-written reverse-dependency
  map. The CI planner consumes `cargo metadata` directly.
- Nix derivations define the actual build inputs and output identity. The
  inspection/benchmark layer queries derivation paths and closures; it does
  not infer their contents from branch names.
- CI declarations define which artifact/test target each suite executes.
  Those target selections can be passed to planners, but may not redefine
  Cargo dependency relationships.
- Dynamically discovered or untracked build inputs are an **uncertainty**,
  causing conservative inclusion rather than an unsafe skip.

## Implementation sequence

1. Record the current derivation hashes and build-time metrics for controlled
   docs-only, isolated-leaf, shared-core, manifest, build-script and lockfile
   changes. Include a complete input classification with explicit exclusions.
2. Prototype an isolated Cargo package source projection. Preserve the root
   workspace/lock metadata, every required workspace manifest, and sources and
   ancillary files in the *resolved* dependency closure. Nonselected workspace
   members must remain valid to Cargo without injecting their real source into
   the selected derivation. If that cannot be guaranteed, retain the broad
   source rather than substituting fabricated semantic inputs.
3. Derive source ownership and path dependency edges from the same manifests
   Cargo reads. If Nix must parse the TOML directly for pure evaluation,
   compare its conservative closure against `cargo metadata` in CI and
   reject under-approximation; do not introduce a manually edited dependency
   inventory or impure import-from-derivation.
4. Replace the monolithic first-party compilation cache for product/runtime
   builds with separately reusable crate or coherent artifact closures.
   Preserve one canonical Rust build configuration, features, toolchain,
   generated inputs, and build-script outputs. Eliminate blanket `touch`
   of all real workspace sources only when replacement/fingerprint safety is
   proven. Investigate reuse via `crane` or an equivalent Nix-native Cargo
   artifact graph before writing another build orchestrator.
5. Expose a generic `phenix-flake-ci` derivation-diff/report mechanism that
   records which output identities change and which artifacts were rebuilt.
   Keep the manifest-specific projection in a provider adapter, not a bespoke
   source table in GitHub Actions.

## Semantic proof gates

- Docs-only edit changes **zero** Rust build derivation identities and runs
  no Rust build suite on a focused PR.
- A leaf-crate edit invalidates that crate and real reverse dependents, but
  preserves unrelated crate artifact derivation paths and cache identity.
- Shared-core and SDK changes invalidate every actual reverse dependent.
- Cargo.lock, toolchain, build.rs, generated sources, feature switches,
  renamed/deleted files, macros, and unknown include paths cannot be skipped
  unsafely; uncertainty widens the closure.
- Fresh cold-cache builds and warm-cache incremental builds produce equivalent
  binaries, tests, generated application interfaces and smoke results.
- The full main/explicit manual integration matrix remains complete. Tests
  must compare actual output hash and build trace, not just planner decisions.

## Merge policy

This is a design and verification contract, **not** an implementation claim.
Do not merge as completed artifact-level optimization without successful
derivation-diff fixtures, an isolated-crate implementation and CI measurements
showing actual first-party artifacts are reused.
