# Q7 Query Review Fix

Worktree: `/tmp/opencode/fbp-q7-typed-failures-7c611912`.
No staging, commits, pushes, main-worktree edits, or shared checkpoint edits.

## Ownership Fix

- Snapshot the authenticated request failure before finalizer cleanup and again after termination, before scope release destroys its record.
- Return the actual enriched primary when its origin matches the initiating execution failure. Preserve actual process, ticket, phase, and other metadata while retaining the query's session, generation, and request identity.
- Keep a newly installed unrelated failure, or an unrelated primary with newly appended cleanup, in `Error::Cleanup::termination_failure: Option<Failure>`.
- Do not attach unchanged, already-terminal unrelated failures. Do not manufacture a stored failure for ordinary `Unsettled` cancellation.
- Inspect the stored failure even when termination returns `Ok`, rather than treating a unit `RequestError` as the complete failure record.
- Keep the initiating cause in the source chain and `RequestInterrupted` outermost. Existing typed close, termination, inventory, and release diagnostics remain intact.
- Retain escaping failure leases. No Q6 observation or summary fields acquire failure ownership.

## Files

- `packages/phenix-storage-graph/src/derived/query.rs`: before/after capture and enriched-primary selection.
- `packages/phenix-storage-graph/src/derived/mod.rs`: optional typed termination failure.
- `packages/phenix-storage-graph/tests/query_cleanup.rs`: recovery-panic, COW cleanup ownership, unrelated-primary, ticket, and lease regressions.
- `packages/phenix-storage-graph/tests/error_sources.rs`: initialize the new optional field without changing source-chain assertions.
- `packages/phenix-graphs/src/failure.rs`: public `same_origin` identity accessor backed by a private immutable `Arc<()>`, preserved across COW clones. No generic equality, text comparison, callbacks, or execution authority.
- `packages/phenix-graphs/tests/fbp_failures.rs`: clones and enriched handles retain origin; independent equal-text construction does not.

Existing changes in these files were retained. Runtime recovery code, Q6 diagnostics, storage grants, and policy were not edited.

## Verification

Passed with default features and with `--no-default-features`:

```sh
cargo test -p phenix-storage-graph --test query_cleanup --test typed_failures --test derived --test error_sources
cargo test -p phenix-graph --test fbp_failures --test fbp_failure_protocol --test error_sources
nix develop --command cargo clippy -p phenix-storage-graph -p phenix-graph --all-targets -- -D warnings
```

Storage: 40 tests per configuration. Graph: 32 default-feature tests and 31 no-default-feature tests. Standalone graph no-default-feature Clippy also passed, avoiding dependent feature unification masking the derive-disabled build.

The cleanup regressions run under both request queue policies. Recovery panic inspection occurs after runtime/kernel and late-completion teardown, checking the concrete `CallbackPanicked`, `RecoveryConstruct`, actual request context, known ticket versus unknown ticket, initiating `Unsettled`, exact cancelled-job inventory, and independently escaping cleanup leases. Existing normal query, cancellation, source, grant, and policy assertions continue to pass.

## Limits

- Full workspace tests and verification models were not rerun for this focused query fix.
- The unrelated-terminal regression uses the real deferred-timeout clock during capture draining. It does not force the narrower instant of deadline expiry inside the finalizer's `fail_request`; before/after capture covers that path without depending on a termination error being returned.
- New cleanup detection for an unchanged origin relies on the runtime's append-only secondary list. Previously escaped snapshots remain immutable.
