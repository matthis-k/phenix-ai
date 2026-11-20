# Q7 Adjacent Cause Preservation

Implemented only in the isolated worktree
`/tmp/opencode/fbp-q7-typed-failures-7c611912`. Existing changes in this
worktree were preserved. No staging, commit, push, shared checkpoint, or main
ledger changes were made.

## Changes

- Storage backend openers now return the existing plugin `CallbackError` alias.
  `Error::Open` owns that typed cause and forwards `source()` through the lease
  wrapper to the concrete original error, without a boxed-box source layer.
- Both `mount_offer` and prepared activation use the same opening boundary.
  Before an opener error escapes, it retains the exact selected
  `ContributionHandle<BackendOffer>`, not just the callback captures. This pins
  the immutable offer, provider instance, and factory/code lifetime. Unrelated
  candidate snapshots and resource requesters are not retained.
- `lease_callback_error` now detaches and destroys its source inside a contained
  unwind boundary while its exact lease remains attached. If source destruction
  panics, the first panic payload is also destroyed under that lease. If payload
  destruction panics, the second payload is deliberately forgotten rather than
  recursively executing arbitrary destructors. This follows the existing bounded
  second-payload policy.
- `PluginSnapshot::bind` preserves the actual `NetworkError` from inspection,
  including the missing primitive/module path, while retaining the existing
  `Code::MissingBinding`, requested contract message, and callback context.
- Crate-internal `RegistryFault` sources retain invalid owner/plugin/contribution
  identities, duplicate identities, expected/actual owners, and actual captured
  configuration/contribution `TypeId`s. Unknown kind/ID remains distinct from
  an actual type mismatch. `get` remains an optional typed lookup. No public
  lifecycle error protocol or crate-root export was added; application
  `Failure::new(code, message)` remains a legitimate message-only leaf.
- Runtime directory metadata failure is now `RuntimeDirectoryIo` with the path,
  `RuntimeDirectoryPhase::SymlinkMetadata`, and the original `std::io::Error`.
  Ownership/mode/type validation still returns `InvalidRuntimeDirectory`.
  No symlink, permission, owner, or fallback rule was relaxed.
- Counter and snapshot fixture codecs retain the actual `TryFromSliceError` in
  existing `CodecCause`, with explicit host-authored summaries. Existing genuine
  application-message `Codec(String)` leaves are still allowed.
- The embedded missing-schema compile fixture uses the proper graph `Failure`
  decode signature. It requires exactly one coded compiler error, `E0046` for
  missing `schema`, and rejects stale signature/type-bound diagnostics.

## Regression Coverage

- Direct and prepared backend-opening failures expose the concrete
  `StorageProviderErrorSource` after catalog, plugin-set, snapshot, and key owners
  are dropped. Debug, Display, source traversal, source destruction, source field
  unwinding, and panic-payload destruction all run before definition/provider/code
  release. An unrelated provider is independently released.
- Failed direct opening releases the mount reservation and permits retry.
- Callback cleanup probes cover nonpanicking destruction, source destructor panic,
  first-payload destructor panic, and cleanup during an existing outer unwind.
- Registry and bind tests downcast actual fault sources and verify captured IDs,
  owner provenance, concrete type IDs, unknown-kind/ID behavior, 256-byte identity
  limits, and genuine application lifecycle leaves.
- Missing compiled-unit tests cover primitive/module graphs and qualified paths.
- Runtime metadata tests trigger real `NotFound` and `NotADirectory` failures and
  verify the preserved OS code and error kind. Existing secure runtime tests pass.
- Both fixture codecs downcast the actual slice conversion error and still decode
  valid values.

## Verification

Passed:

- `cargo test -p phenix-plugin -p phenix-storage-plugin -p phenix-storage --lib --tests`
- The same focused suites with `--no-default-features`.
- `cargo test -p phenix-graph --test integration_regressions` (9 tests).
- `cargo test -p phenix-graph --no-default-features --test integration_regressions`
  (8 tests).
- After strengthening the single-error assertion, the negative fixture was rerun
  and passed separately in both graph feature modes.
- `cargo test -p phenix-kernel --example storage q7_codec_tests`
- `cargo test -p phenix-kernel --test storage` (6 tests).
- `cargo check --workspace --all-targets`, with and without `--no-default-features`.
- Workspace all-target Clippy with `-D warnings`, with and without
  `--no-default-features`, using `nix develop --no-write-lock-file --command`.
- Targeted rustfmt checks on every edited Rust file and `git diff --check`.

Workspace-wide `cargo fmt --all --check` reports unrelated formatting in
`packages/phenix-graphs/tests/fbp_composition.rs` and
`packages/phenix-graphs/tests/fbp_identifier_limits.rs`. These excluded files were
not changed. The default shell lacks rustfmt/Clippy; the repository Nix development
environment supplies both.

These are trusted native-code lifetime and typed-cause guarantees, not a sandbox.
The bounded second-payload leak is intentional. Abort, `panic=abort`, and a double
panic escaping native field cleanup are not made recoverable. No new graph
dependency was introduced into storage, and no graph failure shim was used.

## Files Changed By This Task

- `packages/phenix-storage-plugin/src/lib.rs`
- `packages/phenix-plugins/src/error.rs`
- `packages/phenix-plugins/src/registry.rs`
- `packages/phenix-plugins/src/manager.rs`
- `packages/phenix-plugins/tests/graph_contracts.rs`
- `packages/phenix-storage/src/scoped.rs`
- `packages/phenix-storage/tests/snapshots.rs`
- `packages/phenix-kernel/examples/storage.rs`
- `packages/phenix-graphs/tests/integration_regressions.rs` (embedded missing-schema
  signature and its diagnostic assertions only)
- `docs/fbp-q7-adjacent-causes-20261116.md`

The existing storage `CodecCause` definition in `packages/phenix-storage/src/lib.rs`
needed no further edit. Interface, compose, limits, value, process, runtime, query,
and kernel host publication/failure behavior were left untouched.
