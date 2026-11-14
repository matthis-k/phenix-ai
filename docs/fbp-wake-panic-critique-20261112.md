# Phase 2 — adversarial critique of wake registration boundary

Status: independently written challenge to `fbp-wake-panic-design-20261112.md`
before implementation, 2026-11-12.

1. **Is a catch alone sufficient?** No. The former catch immediately resumed
   unwinding; the new branch must return an explicit `DriverError` without
   consuming the producer or retaining an empty inventory slot.
2. **Does returning the producer certify it is usable?** No. A panicked
   `set_waker` may have mutated its own state. The kernel guarantees only
   ownership and future explicit operator choices, not automatic safe retry.
3. **Does this accidentally remove retirement serialization?** The
   whole-drain gate must remain held across every registration callback and
   drop by RAII on the error return. Do not unlock per producer.
4. **Does the public error mask ordinary unsupported wake?** No. `Ok(false)`
   remains `UnsupportedWake`; `Err(_)` becomes a distinct panic variant.
5. **What about panic payload lifetime?** The catch result must be dropped
   while the owning kernel retains its exact code lease; no arbitrary
   plugin-defined panic payload should escape without that lifetime.
6. **Can CI prove general reentrancy?** No. This handles one panic boundary.
   `ReadinessPolicy::select` remains under the session mutex, and recursive
   `drain` remains unsupported. A passing regression cannot close H3b.
7. **Unproven cases:** double-panic aborts, arbitrary host `on_event`
   panics, callback-side external effects and plugin-internal locks do not
   get rollback or sandbox guarantees.

Verdict: conditionally accepted under the exact typed-error/owner-retention
tests, strict formatting and lint, all workspace tests and exact-head CI.
No new abstraction beyond a diagnostic variant is justified.
