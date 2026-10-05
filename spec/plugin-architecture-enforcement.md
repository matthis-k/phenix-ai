# Plugin architecture enforcement

status: implemented
coverage:
  - scripts/check-plugin-architecture.sh
  - scripts/check-structural-boundaries.sh
  - modules/package-sets.nix
  - rust/crates/phenix-sdk/tests/plugin_attribute_only_gate.rs
  - rust/crates/phenix-core/src/plugin_runtime_adapter_regression.rs
  - rust/crates/phenix-core/src/plugin_management_regression.rs

## Purpose

Define the deterministic repository checks that prevent the converged Plugin architecture from drifting back toward parallel package categories, manual static wiring, implementation-coupled dependencies, or untyped structural boundaries.

Source validation owns facts that are mechanically derivable from repository structure. Rust and Product tests own runtime semantics.

## Package roles

Every first-party Rust workspace package declares one role in `package.metadata.phenix.role`:

```text
runtime-plugin
passive-library
application
assembly
test-support
```

Package names are not authoritative. Runtime ownership comes from metadata.

`scripts/check-plugin-architecture.sh` rejects missing and unknown roles.

## Dependency direction

A passive library cannot have a normal or build dependency on a runtime Plugin.

A runtime Plugin may depend directly on another runtime Plugin implementation only when the consumer declares the dependency in:

```toml
[package.metadata.phenix.implementation-dependencies]
```

Each declaration must match a real normal or build dependency and contain a non-empty reason. Undeclared edges and stale declarations fail Source validation.

Development-only dependencies remain available for test composition.

## Converged metadata

Migration-only `contract-debt` metadata is forbidden.

The repository has one current implementation-sharing mechanism: `implementation-dependencies`.

Do not add a second debt registry, compatibility table, or package-role registry when Cargo metadata already owns the fact.

## Removed package identities

Source validation rejects the retired prerelease runtime package identities:

```text
phenix-plugin-cli
phenix-plugin-sdk
phenix-acp
```

The command toolbelt and ACP adapter use their current package and Plugin identities without compatibility aliases.

## Static authoring

Rust Plugins use the attribute-driven SDK contract in `plugin-authoring-macro.md`. The retired `phenix_plugin!` declaration DSL is not part of the SDK and Source validation rejects any Rust use of it.

`plugin_attribute_only_gate.rs` proves that generated manifests, components, resources, lifecycle, listeners, Layers, and runtime dispatch do not require parallel manual wiring.

## Runtime package set

`phenixPlugins.${system}` contains independently packaged runtime Plugins only.

`modules/package-sets.nix` derives each entry's implementation crate and reads that crate's package role. Evaluation rejects:

- package-set entries whose implementation role is not `runtime-plugin`;
- duplicate implementation packages for independently packaged entries;
- the retired `cli` / `phenix.cli` identity;
- the retired `sdk` / `phenix.sdk` runtime identity.

Passive SDKs, bindings, transports, applications, and assembly packages do not enter `phenixPlugins` merely because they ship in the same repository.

## Structural boundaries

`check-plugin-architecture.sh` delegates the canonical dynamic-value boundary checks to `scripts/check-structural-boundaries.sh` rather than maintaining a duplicate rule set.

`typed-structural-boundaries.md` owns those rules.

## Runtime semantics

Source checks do not infer runtime correctness from file names or grep implementation behavior.

Rust regressions own semantic guarantees such as:

- Plugin Runtime Adapters use the open canonical plugin-runtime-adapter interface;
- runtime dependency cycles are rejected;
- guest and Plugin Runtime Adapter authority remain separate;
- Plugin management commits one resolved Graph Generation atomically;
- failed candidate preparation or start preserves the previous generation;
- attribute-only static Plugins execute through generated canonical adapters.

## Negative fixtures

The architecture Source check includes negative fixtures for the rules it owns, including:

- missing or invalid package role;
- passive-library to runtime-Plugin dependency;
- undeclared runtime-Plugin implementation dependency;
- stale or empty implementation-dependency declaration;
- migration-only `contract-debt` metadata;
- retired package identities.

A rule without a deterministic structural owner belongs in a behavioral test instead of another shell assertion.

## Invariants

- Every workspace package has one declared runtime ownership role.
- Passive libraries do not depend on runtime Plugin implementations.
- Runtime implementation reuse is explicit, exact, and documented.
- Migration-only architecture metadata stays deleted.
- `phenixPlugins` contains runtime Plugins only.
- Independently packaged runtime Plugins have independent implementation ownership.
- Retired prerelease runtime identities stay absent.
- Rust Plugin authoring has one attribute-driven API.
- Structural data rules have one Source-check owner.
- Runtime semantics remain covered by Rust or Product regressions, not inferred by Source scripts.
