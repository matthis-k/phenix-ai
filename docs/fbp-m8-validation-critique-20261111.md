# M8 Phase 2 — adversarial critique before implementation

Date: 2026-11-11. Target: [M8 design](fbp-m8-validation-design-20261111.md).
The proposed M8 is a new proof milestone, not part of the original M0–M7 plan.

1. **False correlation victory:** Two sessions sidestep the shared-session
   head-blocking case but do not repair that case. Do not change the historical
   blocked-join assertion or claim shared-session arbitrary-skew progress.
   Explicitly record the per-request factory/state cost and host-side admission
   bound. If any primary use case requires mutable state shared across requests,
   provide it only via explicit store grants or return to Phase 1.
2. **Not a fake loop:** A linear
   `memory -> model -> tool -> reply` fails M8. The data must actually
   travel back from tool output to memory/model via a cycle, and model must
   produce **two independently observed activations** before the final output.
   One process with an embedded whole agent loop fails plugin-parity intent.
3. **No privileged preset:** Start composer from an empty `NetworkBuilder`;
   every preset/provider/compaction stage is an ordinary registration with
   exact host grants and normal conflict/revocation semantics. Do not add an
   `AgentLoop` kernel API or top-level feature flag.
4. **One committed external effect:** When a model requests a tool, the tool
   effects count must be exactly one even if a bounded capture blocks a later
   emission. Do not treat a string transformation as proof of sandboxing or
   transactional tool effects.
5. **Real independent state:** A `ProcessFactory` must create fresh mutable
   memory per session. Test with two sessions and different input histories;
   no accidental sharing through a factory closure's `Arc<Mutex<Vec<_>>>`.
6. **Backpressure and cycle:** Directed feedback is legal, but the chosen
   queue capacities must be bounded. If a retry becomes blocked, the process
   must not rerun committed effects. Run budgets must be finite and the
   acceptance fixture must assert no unnecessary spin.
7. **Cancellation:** Cancelled session must report cancelled, discard owned
   pending packets and not produce final output afterward. Cancellation of a
   nonyielding external plugin cannot be preempted; do not claim that.
8. **Provenance/schema:** Replacements must use the same exact port interface;
   compare provider behavior, not mere presence of a declaration. Splice must
   have both generated-edge grants. Revocation restores base behavior.
9. **Scope:** M8 is a **proof of architecture**, not a production agent or
   provider integration. A passing deterministic fixture cannot close P1/P2.

Verdict: **conditionally accepted for Phase 3** if all existing invariants
and the new multi-step, isolated-request tests pass at exact final HEAD.
If they do not, return to Phase 1 rather than changing assertions to fit
the implementation.
