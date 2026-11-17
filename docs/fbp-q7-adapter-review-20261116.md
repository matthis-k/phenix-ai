# Q7 Adapter/Value Review Fixes

Scope: the isolated worktree `/tmp/opencode/fbp-q7-typed-failures-7c611912`.
This note records the three adapter/value findings, not closure of Q7.

## Ownership

- Adapter errors and callback panics retain the exact `Arc<dyn EdgeAdapter>`
  through an opaque immutable owner. Retention reaches independently escapable
  cleanup secondaries before the failure leaves preparation.
- Successful local adapter outputs also retain that definition: their Rust
  payloads and destructors can be adapter-defined. Such values now correctly
  reject bare extraction; callers can borrow or use `leased_shared` instead.
  This is a code-ownership safety pin, not a payload/type/provenance conversion.
- Portable output envelopes do not acquire that adapter pin: schema strings and
  byte buffers are host-owned data with no adapter-defined destructor/vtable.
  Existing explicit output leases and inherited source leases remain intact.
- Neither path adds ownership of the source payload/envelope or a process.
  Adapter captures themselves remain subject to the `EdgeAdapter` contract.

## Cleanup

- `Value::dispose_with_cleanup` snapshots known leases through payload and caught
  panic-payload cleanup, then individually catches every snapshot release.
  Preparation uses it for wrong-type output rejection and partial fanout cleanup;
  consuming value helpers use it for invalidated results and extraction errors.
- Original typed primaries stay primary. Payload and final lease cleanup faults
  are separate typed cleanup records; one panicking release does not skip others.
- A private construction-time disposer routes exact `Leased<Failure>` payloads
  through `Failure::dispose_catching`, without cloning/detaching the failure or
  tightening public generic bounds. Other shared owners legitimately defer cleanup
  and return `false`; the last managed owner reports panics actually caught.

## Verification

Eight new tests in `packages/phenix-graphs/tests/fbp_failure_adapters.rs` cover:

- Exact adapter definition/code ownership after targets and input are dropped,
  original non-Clone cause identity, standard source/downcast/format access,
  independently escaped secondaries, and panicking cause destruction.
- Callback panic-payload destruction while callback and input leases are live.
- Last typed local output ownership and portable output without an unnecessary pin.
- One and three final panicking leases on nonpanicking output payloads, both
  wrong-type rejection and an earlier prepared value followed by adapter failure.
  Every release and panic-payload destruction is observed, typed cleanup counts
  are exact, queues stay unchanged, and escaped cleanup retains known code/input.
- Invalidated successful values after source destructor failure, including all
  independent final lease panics, and exact `Leased<Failure>` last-owner reporting.
- Unrelated leased payloads with both ordinary and panicking destructors.

Focused default-feature tests passed: 123 tests. The same selection without
default features passed: 108 tests (derive-only cases are gated out). Selection:
`--lib`, `fbp_failure_adapters`, `fbp_failures`, `fbp_failure_protocol`,
`fbp_connections`, `fbp_types`, `fbp_port_keys`, `fbp_admission_preflight`,
`fbp_typed_admission`, `fbp_custom_closure_admission`, `portable_codec`, and
`fbp_declaration_leases`. Existing library tests include exact request queue
counter and atomicity checks. Documentation tests passed in both configurations
(26 each). Scoped Clippy with `-D warnings` passed with and without default
features. Scoped rustfmt checks passed using the installed Nix toolchain binaries.

No existing tests/assertions were changed, weakened, or deleted, and no expected
result shapes were migrated. `port_keys.rs`, `failure.rs`, `process.rs`, and the
runtime-owned `fbp_failure_protocol.rs` were not edited. No known residual remains
for these three findings in the exercised paths; this is not a workspace-wide or
Q7-completion claim. No staging, commit, or push was performed.
