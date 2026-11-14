# H2 — adversarial critique of host binding callback containment

Status: Phase 2 review, independently of the H2 design note.

1. A `catch_unwind` must enclose **the callback invocation**, not the
   subsequent mapping of its Result, or it will still unwind on panic.
2. Do not catch and drop a `LifecycleError::Plugins` or `Build` into
   `Binding`; only the host binding callback is inside this boundary.
3. Do not convert a returned typed error into a string; source-chain and
   plugin-code lease must be preserved exactly.
4. Panic payloads may contain plugin-defined values. Their destruction occurs
   while preparation still owns `kernel.code_lease`; do not preserve arbitrary
   opaque panic objects beyond this lease.
5. No active or plugin inventory mutations occur before `publish`; regression
   must check active pointer identity, and must not claim rollback of external
   binding effects.
6. The host's `run_until(on_event)` callback is not covered. This issue
   stays partial until callback boundary policy is explicitly audited.
7. Rust panic=abort and double-panic process abort are outside the
   `catch_unwind` guarantee; no claim of a sandbox.

Verdict: accept the narrow catch and typed-error regression, then validate
against exact-head CI. No extra API or scheduler justified.
