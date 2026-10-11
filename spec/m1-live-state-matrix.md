# S2 live state implementation and evidence

Status: TESTED locally; real Nix CI pending
Baseline: accepted S1b, stacked on A0 PR #775 to retain the agent smoke gates
Frozen profiles: `m1-live-state-contract.md` and `m1-epoch-snapshot-contract.md`

| File or task | State | Semantic evidence |
| --- | --- | --- |
| `phenix-artifact/src/lib.rs` | TESTED | Exact native metadata/executable binding, definition-only packages, canonical M1 normalization; v1 decoding stays frozen |
| `phenix-contract/src/descriptor/mod.rs` | TESTED | Internal live schema and slot identity; neither is accepted in a v1 descriptor |
| `phenix-kernel/src/state_resolve.rs`, `resolve.rs` | TESTED | One preparation pass, explicit One/Optional/Many selections, contiguous indices, revision/schema matching, conflict and duplicate rejection |
| `phenix-kernel/src/native.rs`, `runtime.rs` | TESTED | ABI 3 loading and initialization before admission; header-first rejection, v2 replies and existing ticket path |
| `phenix-kernel/src/value_state.rs` | TESTED | Immutable snapshots, incarnation-bound refs, CAS conflicts, atomic batches, one epoch, coalesced cursors and bounded subscriptions |
| `phenix-kernel/src/state_effects.rs`, `runtime.rs` | TESTED | Service and state effects share the operation/resource authority check; denied writes have zero changes; selected refs convey no authority |
| `phenix-kernel/src/pending.rs`, `generation.rs` | TESTED | Root/ticket cursor cleanup, pending/cancellation leases, canonical definition/slot inputs, old roots pinned through promotion and retirement |
| `phenix-kernel/src/bin/phenix-kernel.rs` | IMPLEMENTED | Explicit state composition and v2 input; immutable store paths required for native and definition packages |
| `examples/reactive-provider`, `examples/reactive-client` | TESTED | Independently built SDK-only non-agent images, second selected provider, negative initialization/schema/table variants |
| `phenix-conformance/src/bin/phenix-m1-state.rs` | TESTED | Independent host assertions, bootstrap gap, atomic failure at the last edit, multi-read authority, resource context, stale refs, scope replay and retirement |
| `tests/m1-live-state-product.py` | TESTED vectors; CLI CI pending | Independent canonical metadata/schema hashes and v2 CLI producer; no fake Nix store paths |
| `flake.nix`, conformance workflow | IMPLEMENTED | Two new gates plus all eighteen existing gates; realized outputs and exact checkout uploaded in CI |
| `spec/evidence/m1-live-state-local.json` | TESTED | Source hashes, debug/release reports, package digests and SDK isolation |
| CI acceptance | NOT YET PROVEN | Requires the exact implementation commit, downloaded archive digest, twenty gates and report hashes |

S2 adds no agent semantics or graph executor. S3 owns generated property storage,
typed snapshots/edits and contract authoring. S4/S5 own graph preparation and
reactive execution. S6 owns concurrent admissions; the current profile remains
serial. Pending child imports for generated ABI 3 authoring belong to the S3
host/SDK gate, using the existing ticket manager. No model I/O, persistence,
skills, context or Basic service parity is claimed by this kernel stage.
