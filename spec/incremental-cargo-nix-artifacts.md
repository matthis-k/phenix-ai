# Dependency-derived incremental Nix builds

status: partial
stage: first-party Cargo artifact reuse
ownership:
  - phenix-ai Rust/Nix derivations
  - phenix-flake-ci generic impact and verification interfaces

## Implemented and verified

- #738 (merged): affected PR shards selected from Cargo metadata; unknown or
  shared build inputs fail open.
- #748 (merged): PR Clippy accepts the affected Cargo package closure while
  main/manual CI remains full.
- #746 (merged): harness product source derives from Cargo package closure.
  Cargo/Nix parity matched 45 crates; packaged runtime smoke passed.
- #749 (merged): standalone core, client, runtime, Lua and fixture packages
  reuse one source closure selector. Parity matched seven package roots, and
  standalone builds, Lua callbacks and harness smoke passed.
- #754 (under test): build-only source closure excludes dev-only crate source;
  Cargo/Nix build-closure parity and derivation identity probes passed. Full
  build and integration smoke remain merge requirements.

Temporary conformance workflows are linked from their PRs and close without
merge; they should not become permanent production workflow declarations.

## Remaining inefficiency

Per-output source isolation preserves Nix derivation identities for unaffected
crates. It does **not** automatically share compiled first-party Cargo artifacts
between independent Nix package derivations. Each standalone build currently
creates its own Cargo target output. The harness product carries a shared
external-dependency skeleton but touches all selected real first-party sources.

The shared workspace manifest skeleton also invalidates outputs on edits to
unrelated package manifests. Cargo still needs a valid root manifest, lockfile,
and workspace members. Do not blindly delete these inputs without testing that
resolution and lockfile behavior stay correct.

## Sources of truth and invariants

1. Cargo owns package identity, path dependencies, feature resolution,
   dependency kind, target selection, build scripts and lockfile metadata.
   There must not be a second manually edited dependency graph.
2. Nix owns derivation inputs, package outputs, Rust toolchain and cache keys.
3. Nix closure projections must match or conservatively exceed Cargo metadata.
   Unknown dynamic inputs widen rather than narrow affected builds.
4. Maintain one root Phenix flake; Nix remains a deployment utility rather than
   an authority over Phenix runtime semantics.

## Next proof: share compiled Cargo artifacts

#758 prototypes Crane dependency-artifact reuse using separate experimental
outputs; it does not replace any production package. Measure:

- Cold-cache and warm-cache build times on comparable runners, distinguishing
  Nix substitution from fresh Cargo compiler execution.
- Whether an edit to one first-party crate preserves the reusable dependency
  artifact derivation while invalidating only appropriate consumers.
- Whether build scripts, generated application interfaces, proc macros and
  target/profile/feature/RUSTFLAGS settings remain equivalent between the
  dependency artifact and final build.
- Whether core, harness, standalone runtime and Lua callback fixtures retain
  their behavior, including error paths and plugin loading.

Crane is only a backend candidate. Do not add it to the production dependency
closure until its measurable benefit exceeds added evaluation, packaging and
cache maintenance costs.

## Semantic gates

- Docs-only edit: no Rust build derivation identity changes.
- Leaf crate: preserve unrelated crate artifact identities; rebuild its actual
  reverse dependents.
- Dev-only crate: preserve release product derivation; test/example targets
  still include the dependency when required.
- Shared contract, Cargo lock, build.rs, toolchain and feature changes: affect
  every actual consumer or conservatively trigger verification.
- Verify compiled artifact reuse with Cargo build output, not only Nix hashes,
  and compare fresh against cached builds.
- Full main/manual test coverage must remain available.

CI green is not sufficient to declare artifact-level incremental compilation
complete without these semantic tests.
