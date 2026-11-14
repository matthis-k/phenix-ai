# Adversarial critique — whole-drain lifecycle

Status: Phase 2 review **before** implementation of
[whole-drain design](fbp-whole-drain-design-20261111.md).

1. The additional gate is not a second lifecycle authority if its only job is
   serializing drain-versus-retirement; `ProducerState.retired` remains the
   authoritative retirement bit. Verify lock order gate -> producer state for
   *every* retirement and drain path, including host Drop.
2. While a producer is temporarily taken out of its slot, another method could
   observe `None`. `run_until` installs wakes; it must not silently miss the
   producer. Serialize initial wake installation with the same gate (releasing it
   before driver loop). Avoid counting only `Some` producers when scheduling.
3. The moved producer must be restored on transient session contention, Full,
   idle and nonfatal admission failures. Source poll errors remove only that
   producer; its destructor must execute outside both inventory and drain locks.
4. If `poll` panics, a poison or missing slot must not corrupt inventory or
   unload a still-leased native library. Catch panics and retire the source with
   a leased diagnostic. Test panic as well as Result::Err.
5. Do not hold the inventory lock while `NetworkSession::inject` refreshes,
   since that is where `ReadinessPolicy` is invoked. Preserve eager readiness,
   accounting, event order, and Full retry *exactly*.
6. A new `Mutex<()>` held across callbacks cannot permit recursive `drain` or
   host `publish` from the same thread. That is an inherent conflict with
   exclusive mutable producer ownership / whole-drain retirement; document
   and test fail-fast or explicitly narrow the reentrancy claim. It is not
   acceptable to describe general callback reentrancy as solved.
7. Race `publish` against a blocked producer poll and multiple pending events.
   It must finish after all of the current drain's calls. The same code-lease
   ordering must hold for detached producers and retired snapshots.
8. `run_until` and `Kernel::start` are separate activation phases. Avoid
   exposing unavailable producer slots or executing callbacks while both locks
   are held. Deferring every session refresh until after drain would regress
   queue credit; eager injection is required.
9. `ProducerState` currently stores tuples with an owned `Box<dyn Producer>`.
   Changing this to an optional slot is visible to in-crate tests; convert
   fixtures rather than weakening their logic.

Verdict: **conditional go to Phase 3** for a minimal lifecycle gate plus
in-flight slots, subject to all existing behavioral tests and new targeted
reentrancy/retirement cases passing. A failure requires another Phase 1 revision,
not a new compatibility wrapper.
