# Q7 Interface Reason Review

Scope: the InterfaceError erasure gap only, in
`/tmp/opencode/fbp-q7-typed-failures-7c611912`. This is not a Q7 closure claim.

## Changes

- Replaced `InterfaceError.message` with `Box<InterfaceReason>`. The only
  constructor accepts a typed reason; no string fallback or compatibility alias
  remains. Boxing the reason keeps existing unboxed error return types below
  Clippy's large-error threshold without dropping diagnostic data.
- Added reasons for invalid portable schema fields, duplicate ports, incompatible
  types, wrong direction, missing/extra ports, invalid port names, invalid contract
  identities, differing contract definitions, and missing explicit claims.
- `PortableSchemaField` distinguishes name, structure, and codec without string
  reason codes. Reasons own invalid data and both conflicting contract snapshots.
  Shape failures reuse existing endpoint and expected/actual identity context.
- `Display` derives the original human-readable text from the reason. There is
  no independently mutable cached message. `InterfaceError` remains a genuine
  leaf error with no invented `source()`.
- Validation predicates and order, exact shape equality, claim authority, and
  frozen compiler obligations are unchanged. Revision zero remains accepted;
  no portable schema revision validation was introduced.
- Audited `port_keys.rs`: its existing framework Failure wrappers already retain
  the concrete InterfaceError for record-port, record-interface, and key-definition
  errors. No production adaptation there was necessary.
- Migrated Graph and Kernel assertions that read InterfaceError.message to typed
  reasons, retaining meaningful presentation assertions and source checks.

## Exact Files Edited

This list describes this task's edits, not the isolated worktree's pre-existing
or concurrent changes.

- `packages/phenix-graphs/src/interface.rs`
- `packages/phenix-graphs/src/contract.rs`
- `packages/phenix-graphs/tests/fbp_contracts.rs`
- `packages/phenix-graphs/tests/fbp_port_keys.rs`
- `packages/phenix-graphs/tests/error_sources.rs`
- `packages/phenix-kernel/tests/contract_catalog_consistency.rs`
- `packages/phenix-kernel/tests/diagnostics.rs`
- `docs/fbp-q7-interface-review-20261116.md`

## Verification

- `cargo test -p phenix-graph --test fbp_contracts --test fbp_port_keys --test fbp_authoring_equivalence --test error_sources`: 41 passed.
- The same Graph command with `--no-default-features`: 27 passed; the derive-only
  port-key suite correctly contains zero tests in this mode.
- `cargo test -p phenix-kernel --test contract_catalog_consistency --test diagnostics`: 8 passed.
- `cargo clippy -p phenix-graph --all-targets -- -D warnings`: passed.
- The same Graph Clippy command with `--no-default-features`: passed.
- `cargo clippy -p phenix-kernel --test contract_catalog_consistency --test diagnostics -- -D warnings`: passed, also with `--no-default-features`.
- Targeted `rustfmt --edition 2024 --check` on all seven Rust files: passed.
- `git diff --check`: passed.

Clippy was provided through `nix shell` using the repository's pinned nixpkgs
revision. Formatting used the existing Rust 1.96.1 formatter under
`/tmp/opencode/rustup/toolchains/1.96.1-x86_64-unknown-linux-gnu/bin/rustfmt`.

The tests cover actual validation/contract failures, both shape directions,
invalid schema field data and validation order, exact expected/actual identities,
concrete InterfaceError sources through Failure, and reconstruction rejection
before consuming inputs. Existing authoring and frozen-obligation tests pass.

## Remaining Scope

No message-only InterfaceError constructor or InterfaceError.message consumer
remains in production Rust or the audited tests. The remaining plugin `.message`
uses refer to the separate plugin error type, not InterfaceError; plugin error
erasure is outside this task. Shared historical API documentation still describes
the previous InterfaceError shape and is left for the main documentation
aggregate. No broader erasure-gap audit or Q7 completion is asserted.

No edits were made to failure.rs, runtime, process, shared checkpoints, or ledgers.
No commit or push was made, and the main worktree was not modified by this task.
