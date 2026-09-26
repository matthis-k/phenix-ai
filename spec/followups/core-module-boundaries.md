---
status: planned
source: expanded-rust-library-ownership-audit
base_pr: 608
---

# Core module and private-crate boundaries

## Goal

Make the physical Rust structure match the architectural structure after the library-ownership cleanup.

This PR is structural. It must not redesign kernel behavior. The work should be mechanical: move canonical contract primitives into one private dependency-leaf crate, group the remaining flat `phenix-core` files into semantic modules, update imports, and enforce the intended dependency direction.

## Decision summary

Create exactly one new private workspace crate in this PR:

- `phenix-contract`: wire-stable Phenix value/schema/identity/interface primitives that are useful without a running kernel.

Keep these as modules inside `phenix-core`:

- authority
- capability
- composition
- configuration
- events
- metadata
- observable
- persistence
- plugin lifecycle/build
- reconciliation
- runtime
- tasks

Do **not** create `phenix-graph`, `phenix-authority`, `phenix-events`, `phenix-observable`, `phenix-persistence`, or `phenix-reconciliation` crates. #519 already externalizes generic graph mechanics to `petgraph`; #520-#521 centralize storage mechanics. Those subsystems still encode kernel semantics and do not gain an independent consumer/API boundary from another crate split.

## Layering

The intended dependency direction after this PR is:

```text
phenix-contract
      ↑
      ├──────── phenix-sdk / phenix-domain contract definitions
      ├──────── provider/adapters that only need contract primitives
      └──────── phenix-core
                    ↑
                    ├──────── phenix-sdk authoring/runtime helpers
                    ├──────── runtime/harness
                    └──────── runtime plugins/adapters that need kernel access
```

Rules:

1. `phenix-contract` is a dependency leaf inside the Phenix workspace. It may depend on general-purpose serialization/error crates, but never on another Phenix crate.
2. Contract-only code must not depend on `phenix-core` merely to obtain `PhenixValue`, schemas, IDs, or `ComponentInterface`.
3. Kernel mechanisms may depend on `phenix-contract`; the reverse dependency is forbidden.
4. `phenix-core` remains the owner of host/runtime behavior. Moving a type into `phenix-contract` must not move lifecycle, dispatch, authority, persistence, provider-selection, or reconciliation policy with it.
5. Do not introduce forwarding modules or duplicate type definitions. A moved type has one canonical definition.

## Step 1: add `phenix-contract`

Add workspace member:

```text
rust/crates/phenix-contract/
  Cargo.toml
  src/
    lib.rs
    identity.rs
    contract.rs
    contract_wire.rs
    std_value.rs
    structural_value.rs
    infallible_value.rs
    interface.rs
```

`Cargo.toml`:

- `publish = false`
- `[package.metadata.phenix] role = "passive-library"`
- normal dependencies limited to the general-purpose crates actually required by the moved code (`serde`, `serde_json`, `thiserror`, plus any already-required leaf utility dependency proven by the moved implementation)
- no `phenix-*`, `rusqlite`, `parking_lot`, `tokio`, `petgraph`, filesystem, network, or process dependency

### Move unchanged where possible

Move these current `phenix-core` modules into `phenix-contract`:

- `identity.rs`
- `contract.rs`
- `contract_wire.rs`
- `std_value.rs`
- `structural_value.rs`
- `infallible_value.rs`

Preserve serde/wire forms, validation text, ordering, and schema compatibility behavior exactly.

### Move `GraphGenerationId`

`GraphGenerationId` is currently defined in `resolver.rs` but is referenced by the contract/value layer and exposed through SDK-facing APIs. Move the type definition and its parse/serde/value behavior into `phenix-contract::identity` beside the other identity types.

Keep generation creation/derivation in composition/resolution. The move changes type ownership, not generation semantics.

There must be no second wrapper or conversion type.

### Split `typed_component.rs`

Move the contract-only half into `phenix-contract/src/interface.rs`:

- `InterfaceSchema`
- `InterfaceCompatibility`
- `InterfaceSchemaMismatch`
- `ComponentInterface`
- associated schema compatibility helpers/tests

Keep kernel-dependent invocation in `phenix-core`:

- `ComponentInvocationError`
- `ResolvedImportHandle::invoke_value`
- tests that require `Kernel`, resolved providers, runtime dispatch, or plugin instances

The kernel-dependent implementation imports `ComponentInterface` and `InterfaceSchema` from `phenix-contract`.

`phenix-contract` must have no `Kernel`, `KernelError`, `ResolvedImportHandle`, provider, runtime, or plugin-host reference after the split.

### `ArtifactRevision`

Keep `ArtifactRevision` in `phenix-core` in this PR. It is a useful value type, but extracting it would add hashing concerns to a crate whose boundary is contract/schema/identity mechanics. Revisit only if a non-core crate needs artifact revision semantics without otherwise depending on core.

## Step 2: canonical imports

Update first-party crates according to role.

### Import from `phenix-contract` directly

Use `phenix-contract` for code whose dependency is only on contract primitives, including schema/ID/value definitions in:

- `phenix-domain`
- contract-definition portions of `phenix-sdk`
- binding/adapter modules that only require canonical IDs/value/schema types
- tests/fixtures that intentionally verify the contract layer in isolation

### Keep `phenix-core` dependency where runtime access is real

Do not remove `phenix-core` from crates/modules that use any of:

- `Kernel` / `KernelConfig` / `KernelError`
- `PluginHost` / `PluginContext` / `KernelAccess`
- `PluginInstance` / plugin activation/lifecycle
- resolved component/provider state
- persistence backend/registration
- event/task runtime
- reconciliation/management

A crate may depend on both `phenix-contract` and `phenix-core` when it contains both contract definitions and runtime implementation. Do not create a façade solely to avoid two honest dependencies.

### Re-export policy

`phenix-sdk` remains the normal plugin-author-facing façade and may re-export canonical `phenix-contract` types deliberately.

`phenix-core` may re-export a `phenix-contract` type only when that type is part of a core runtime API signature. Do not blanket `pub use phenix_contract::*`.

Update first-party source to import the canonical crate where practical instead of relying on a core re-export.

No compatibility alias modules such as `core::contract_legacy` or duplicate `SessionId`-style wrappers.

## Step 3: group `phenix-core` into semantic modules

Replace the current flat root module list with this target layout. File names may remain unchanged inside the directory when renaming would add no clarity.

```text
phenix-core/src/
  lib.rs
  agent.rs
  artifact.rs
  authority.rs

  capability/
    mod.rs
    registry.rs                 # current capability.rs implementation, split only if natural

  composition/
    mod.rs
    activation.rs
    component.rs
    inspection.rs
    manifest.rs
    provider_resolution.rs
    registry.rs
    resolver.rs
    component_invocation.rs     # kernel-dependent half of typed_component.rs

  configuration/
    mod.rs
    resolution.rs               # current configuration.rs

  events/
    mod.rs                      # current events.rs or existing event children

  metadata/
    mod.rs
    composition.rs              # composition_metadata.rs
    frontend.rs                 # frontend_metadata.rs
    input.rs                    # metadata_input.rs
    inspection.rs               # metadata_inspection.rs
    reconciliation.rs           # metadata_reconciliation.rs

  observable/
    ...                         # retain existing internal layout

  persistence/
    mod.rs
    backend.rs                  # current persistence.rs
    bootstrap.rs                # persistence_bootstrap.rs
    provider.rs                 # persistence_provider.rs
    value.rs                    # persistence_value.rs

  plugin/
    mod.rs
    build.rs                    # plugin_build.rs
    build_execution.rs          # plugin_build_execution.rs
    context.rs                  # plugin_context.rs
    management.rs               # management.rs
    prepared_mutation.rs

  reconciliation/
    mod.rs
    graph.rs                    # reconciliation.rs
    inspection.rs               # reconciliation_inspection.rs
    live.rs                     # live_reconciliation.rs

  runtime/
    ...                         # retain existing runtime submodule layout

  sdk.rs
  tasks.rs
  invocation.rs
```

Notes:

- This is organization, not a request to invent additional abstractions.
- If `capability.rs`, `events.rs`, or `tasks.rs` remains a single cohesive file after #519-#524, a directory containing only `mod.rs` is not required. Prefer a file until there are at least two meaningful children.
- `observable/` and `runtime/` already have internal structure; preserve it unless a move is required by the ownership changes above.
- `agent.rs`, `artifact.rs`, and `sdk.rs` remain top-level because this PR does not change their architectural ownership. A later domain-boundary change requires separate justification.

## Step 4: module dependency direction

Within `phenix-core`, use these conceptual layers:

```text
contract crate
    ↓
authority / capability / events / observable / persistence / tasks
    ↓
composition / configuration / metadata / reconciliation
    ↓
plugin lifecycle / runtime / management
```

This is not a reason to create more crates. Enforce it through module visibility and imports where possible:

- prefer `pub(crate)`/`pub(super)` for implementation helpers
- do not expose a helper at crate root solely so a sibling module can reach it
- avoid `use crate::*` in production modules; import the concrete types used
- runtime may orchestrate lower layers; lower layers must not call runtime orchestration to perform their own primitive operation
- persistence remains unaware of plugin runtime instances; plugin/runtime code calls persistence through its contract
- reconciliation computes/represents transitions; runtime applies them

If an existing dependency violates this direction and cannot be fixed by a mechanical move, record it in the PR body as a named exception rather than adding an indirection layer to hide it.

## Step 5: test placement

Clean up root-level regression modules while moving files.

For every current `*_regression.rs` at `phenix-core/src/`:

- if it uses only public API, move it to `phenix-core/tests/`
- if it intentionally tests crate-private behavior, move it under the owning module as `tests.rs` or an inline `#[cfg(test)]` child
- do not make private implementation types public just to turn a unit/regression test into an integration test

After this PR there should be no unrelated regression-test files mixed into the root production module list.

## Explicit non-goals

- no behavior or wire-format redesign
- no provider/routing policy change
- no persistence schema/layout migration beyond import/module paths
- no async/runtime-model change
- no plugin API redesign
- no additional graph/storage abstraction beyond #519-#521
- no crate-per-module split
- no public compatibility layer for old internal module paths

## Mechanical work order

1. Add `phenix-contract` crate and workspace dependency entry.
2. Move contract/value/identity modules without semantic edits.
3. Move `GraphGenerationId`; update resolver imports.
4. Split `typed_component.rs` into contract interface definitions and core component invocation.
5. Make `phenix-contract` compile/test alone.
6. Update `phenix-core` to consume the new crate; delete moved source files.
7. Update SDK/domain/adapter/provider imports and Cargo dependencies according to the rules above.
8. Run the full workspace and remove now-unused dependencies from `phenix-core` only after compilation proves them dead.
9. Move `phenix-core` files into the target semantic directories, one group at a time: composition → metadata → persistence → reconciliation → plugin.
10. Relocate root regression tests.
11. Run import/dependency audit and delete transitional re-exports/helpers not required by intentional API signatures.
12. Run exact-head maintenance/product/integration checks and record LOC/dependency deltas.

The implementation should not require new architectural choices after step 1. If a move reveals a real cycle, preserve the type in its current semantic owner and document the exact cycle; do not invent a new shared crate during implementation.

## Acceptance criteria

### Crate boundary

- [ ] `phenix-contract` exists, is `publish = false`, and has no dependency on any `phenix-*` crate.
- [ ] `identity.rs`, contract/value/schema mechanics, and the contract-only interface definitions have one canonical definition in `phenix-contract`.
- [ ] `GraphGenerationId` has one definition and resolution still owns generation construction.
- [ ] `phenix-contract` contains no kernel/runtime/persistence/provider/plugin-host type reference.
- [ ] `cargo test -p phenix-contract` passes independently.
- [ ] SDK contract definitions no longer import `PhenixValue`/`PhenixSchema`/IDs/`ComponentInterface` from `phenix-core` when no runtime type is required.

### Core organization

- [ ] `phenix-core/src/lib.rs` declares semantic module groups rather than the current flat list of implementation files.
- [ ] composition files live under `composition/`.
- [ ] metadata files live under `metadata/`.
- [ ] persistence files live under `persistence/`.
- [ ] reconciliation files live under `reconciliation/`.
- [ ] plugin lifecycle/build/context files live under `plugin/`.
- [ ] runtime and observable retain coherent existing submodule structure.
- [ ] no root-level `*_regression.rs` remains merely because the old layout was flat.

### Dependency hygiene

- [ ] No new private crate beyond `phenix-contract` is introduced.
- [ ] No forwarding/legacy module exists for moved internals.
- [ ] No duplicate canonical IDs/schema/value/interface types exist.
- [ ] Production modules do not use `use crate::*` to bypass the new structure.
- [ ] Lower-level persistence/observable/event/task modules do not acquire runtime/plugin-management dependencies during the move.
- [ ] Unused dependencies made obsolete by the split are removed.

### Compatibility and validation

- [ ] Existing serde/wire fixtures for moved types are byte-for-byte/JSON-equivalent.
- [ ] Existing interface schema compatibility tests pass unchanged in semantics.
- [ ] Existing provider resolution, activation, reconciliation, persistence, observable, event, and task tests pass.
- [ ] SDK external-consumer canonical and renamed-dependency fixtures still compile.
- [ ] Source, Rust, Clippy, Product, Integration, Docs, and Maintenance checks pass at exact head.

### Size/reporting

Record in the PR body:

- hand-maintained production Rust LOC before/after, excluding lockfiles/generated fixtures
- `phenix-core` production LOC before/after
- `phenix-contract` production LOC
- direct normal dependency count for `phenix-core` before/after
- direct normal dependency count for `phenix-contract`
- number of top-level production module declarations in `phenix-core/src/lib.rs` before/after

This PR is successful even if total production LOC is roughly neutral: its primary objective is dependency and ownership clarity. It must not materially increase production LOC through wrappers or compatibility scaffolding.
