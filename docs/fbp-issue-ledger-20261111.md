# FBP code-review issue / resolved / evidence ledger

Updated 2026-11-11. Base review: **8.0/10**. This ledger distinguishes code
presence from verification: a committed fix is **not** verified until a complete
CI run on the exact final HEAD passes. No new score is asserted without that.

Branch: `impl/graph-consolidation-20261110`.
Review categories: H = high-impact runtime; C = concrete functional/security;
A = architecture/API; Q = quality and evidence. Missing evidence is *open*,
not an accepted limitation.

| Issue | Resolved? | Evidence / remaining acceptance |
| --- | --- | --- |
| H1 Shared-session FIFO correlation head blocking | Open; diagnostic added (CI pending) | `NetworkSession::correlation_blocks()` passively identifies conflicting authenticated `All` heads without skipping FIFO; `fbp_streams` checks exact [A,B]/[B,A], aligned, and unscoped cases. Independent bounded sessions remain a workaround, not shared-session resolution. See `fbp-h1-correlation-design-20261112.md`. |
| H2 Callback panic containment | Partial; CI pending | Kernel preparation and activation catch panics into contribution-scoped leased errors; `callback_errors.rs` covers preservation of active generation. Driver wake-registration panics now return `DriverError::WakeRegistrationPanicked` instead of unwinding, with producer slot restoration tested. Factories, steps, readiness and polls already contain panics. Arbitrary host callbacks and in-process aborts remain outside the guarantee. |
| H3a Inventory lock inversion on producer admission | Code fixed; CI pending | `Kernel::drain` takes a producer out of inventory before polling/admitting and restores it after callback, under a separate whole-drain gate; `eager_readiness_during_drain_may_inspect_unlocked_inventory` and `generation_retirement::blocked_poll_finishes_its_drain_before_publication_retires_the_source` cover both lock and retirement invariants. |
| H3b Session/drain recursive callback reentrancy | Partial; CI pending | `KernelLockScope` rejects same-thread recursive session operations (including `run_until`) with errors, and recursive drain with fail-fast panic converted at plugin boundaries. Cross-thread callback wait cycles remain open. See H3b design/critique and native recursion regressions. |
| H4 Full-inventory scheduling scans | No | `runtime/mod.rs::refresh` walks process inventory; need activation-scaled measurements before refactor |
| H5 Expensive closure analysis | No | `runtime/closure.rs`; need bounded stress and topology-aware cost evidence |
| H6 No native performance baseline | No | `docs/fbp-measurements.md` records source counts, not throughput/allocations; add benchmarks with reproducible inputs |
| H7 Request isolation not complete | No | `runtime/request.rs`, FIFO and shared state; need independent request correlation, cancellation and fairness proofs |
| C1 Cancellation overwrites failure/completion | Code fixed; CI pending | `33c6df726160`, `47e56d76bc03`, `fbp_terminal_transitions.rs`; verify exact-head CI |
| C2 Replayed frozen obligations duplicate | Code fixed; CI pending | `f67b2df21ae8`, `6472a3452802`, `0800f267e783`, `4ec113e3f21c`, `fbp_obligation_dedup.rs`, transaction replay tests; verify CI |
| C3 Retired generations remain callable | No — policy blocker | `phenix-kernel/src/host.rs` and `tests/generations.rs`; existing tests **require** archival explicit execution; see `docs/fbp-c3-c4-design-20261111.md`. Do not silently revoke admitted work |
| C4 Full-network preparation inspection | Code fixed; CI pending | `CandidateContext::bind_import` is scoped, full `inspect_network()` requires exact host grant; `prepare_inspection.rs` tests default denial and exact-contribution non-bleed across 6 grant configurations; portable fixture and detached-code-leases test preserved; see revised Phase 1/2 docs |
| C5 No built-in watchdog for deferred jobs | No | `runtime/mod.rs`, dispatcher/handoff ownership; decide host supervision/deadline policy without introducing kernel timers implicitly |
| C6 Live process state not migrated | No | Generation replacement constructs a new network; define migration contract, or explicitly select restart/disposal policy |
| C7 No general in-process plugin sandbox | No — architectural limit | Native Rust plugins share process privileges; do not claim an FBP capability view is a security sandbox |
| C8 Code leases retain defeated snapshots | No | `phenix-kernel/src/lib.rs` code lease retains snapshot inventory; measure retained lifetime and isolate exact winners where safe |
| C9 `state()` is not passive | No | `runtime/mod.rs::state` calls `refresh()`; state queries may execute readiness callbacks; require separate query semantics or explicit refresh naming |
| C10 `run_until_complete()` aliases `run_until_idle()` | Code fixed; CI pending | Removed redundant public alias and migrated native examples/tests to `run_until_idle` without execution change; Phase 0–2 in `fbp-c10-driver-api-design-20261112.md` and critique. Final-head CI required. |
| A1 Replacement-only private nesting | No | `network.rs`, `compose/mod.rs`; exact private composition extension remains unimplemented |
| A2 Rigid interface identity | No | `interface.rs`, `contract.rs`; design compatibility/projection only with evidence for real use cases |
| A3 Store coupled into graph-local types | No | `resource.rs`; check boundary against kernel/storage dependency direction |
| A4 Layered typed authoring | No | `port_keys.rs`, derives and builder; evaluate public concept duplication against real examples |
| A5 Physical string paths | No | `network.rs` and `port_keys.rs`; typed handles are not universally preserved after compilation |
| A6 Admission payload validation ergonomics | No | `runtime/mod.rs`; compare runtime path validation with typed checked admission |
| A7 Unbounded identifier sizes | No | Names/paths from plugin payloads; add explicit limits or justified host-controlled quotas |
| Q1 Large responsibility concentrations | No | `network.rs`, `runtime/mod.rs`, `compose/mod.rs`; first establish stable behavioral seams |
| Q2 Whole-candidate recompilation | No | `Composer` recomposes/recompiles; measure then consider incremental compilation with identity invariants |
| Q3 Dependency/conflict selection scaling | No | `compose/mod.rs`; stress diamond, skew, and adversarial priority graphs |
| Q4 No static liveness guarantee | No | Legal cycles; no sound analyzer/progress guarantee yet; avoid treating all stalls as errors |
| Q5 Concurrency testing not exhaustive | No | Existing deterministic probes cover selected interleavings; expand model checks and lock-order testing |
| Q6 Diagnostics intentionally minimal | No | `runtime/diagnostics.rs`; add actionable explanations without old wave engine |
| Q7 Error reporting uneven | No | Mix of string errors and typed errors; audit public errors and preserve causes/leases |
| Q8 Final reports stale | No | `docs/fbp-report.md` references an old uncommitted baseline; this tracker records newer code but a final exact-head report still must be generated |

## Current verification gate

Most recent C4 code commit: `d00d0bf243fe737dc05f6e4c1fcfb606adf0c237`.
After this ledger is committed, verify CI **on the resulting HEAD**, not an earlier
run. A cancelled/superseded job is not a pass. The previous 4ec113e run was
superseded before its matrix finished.

## Next sequence

1. Close C1, C2, C4 only after exact-head CI and targeted diff review.
2. C3 remains blocked on an explicit retirement policy; no silent ABI/behavior change.
3. Work next on H1/H3 high-impact correlation and reentrancy, then C5/C9/C10 and
   measurement gates. Every new abstraction follows Phase 0–4 of the design protocol.

## H2 focused diff review — plugin callback panics

Scope is kernel-owned plugin `Contribution::prepare` and `CandidateArtifacts::activate`,
both invoked outside the graph runtime. Their panics previously unwound through
`Kernel::build_with_options` or `Kernel::start`, unlike process factories,
steps, readiness, dispatch and polling, which already catch panics.

The fix reuses the existing `ResolutionError::Composition` and
`StartError::Activation` error paths and the exact snapshot code lease,
with a stable non-reflective error message. It does not catch or conceal typed
callback errors, fabricate rollback for external side effects, or reenter a
partially started producer inventory. Caught panic payloads are dropped while
the owning plugin code lease remains held.

Regression: a published old generation survives both failure modes; exact
contribution and error phase are reported. Producer disposal and startup retry
remain separate existing tests. `Producer::set_waker` intentionally continues
to resume panic and is not classified as fixed by H2. Final-head CI is required.

## H1/H3 evidence correction

The original H3 row overstated a stale producer-inventory lock defect. Source audit
on 2026-11-12 confirmed `Kernel::drain` already extracts the producer, releases
the inventory mutex, polls or admits under the separate whole-drain gate,
and restores the slot. Whole-drain completion before publication is tested and
must not be weakened by a superficial per-poll unlock fix.

H1 has a deliberate, tested workaround for primary independent agent requests:
one bounded `NetworkSession` per request. The crossed-session test now also
shares an explicit `Arc<AtomicUsize>` through distinct process instances,
proving global application state can coexist with isolation. This **does not**
address crossed FIFO heads within one physical shared process and does not
permit unbounded session admission; retain H1 as open for that use case.

## H2 wake-registration API revision — Phase 4 verification scope

Unlike a plugin's `Producer::poll`, `Producer::set_waker` previously caught a
panic only to resume unwinding through the public `run_until` driver. The
new typed `WakeRegistrationPanicked { producer }` reports the exact owner
without claiming the plugin's partially mutated state is safe for retry. The
producer is put back into its slot before error propagation. The whole-drain
gate remains held for registration and is automatically released on return.
A native regression tests the typed outcome, retained producer and exact
callback-error propagation from a subsequent poll.

See `docs/fbp-wake-panic-design-20261112.md` and
`docs/fbp-wake-panic-critique-20261112.md`. Final exact-head CI and diff
review must still pass before the row is marked verified.
