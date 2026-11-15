# Resolver Causes And Invalid-Emission Context

Status: implemented, independently reviewed and locally verified; exact-head hosted
acceptance pending. Based on verified `b74201d80`.

## Constraints And Alternatives

Q7 still loses a real `InterfaceError` when an explicitly selected import advertises
an incompatible contract: the resolver formats it into a string-only rejection.
Q6 combines unknown emission outputs and wrong value identities into one message
without the compiled process path or expected/actual identities.

Preserve import eligibility and full policy-key preflight, callback ordering,
request-local failure/recovery, cleanup, admission and publication authority. These
are diagnostic corrections, not a new error protocol, scheduler or liveness proof.

- Leaving the paths unchanged retains the missing cause and ambiguous diagnostic.
- Adding an optional cause beside the existing free-form reason creates redundant
  representations. Instead, `IneligibleReason` distinguishes an incompatible
  contract with its actual boxed cause from host-policy denial.
- A new top-level resolution variant is unnecessary. Keep requirement/offer context
  in `ResolutionError::Ineligible`, preserve its display phrases, and expose the
  stored `InterfaceError` directly through `source()`.
- For emissions, split validation locally and retain the existing `fail_process`
  call and immediate return. A public diagnostic API or runtime error hierarchy is
  unnecessary for explaining these two known failure cases.

The public `Ineligible.reason` field changes from `Box<str>` to `IneligibleReason`.
Repository callers do not depend on the old field type; this native branch adds no
compatibility shim. Host denial covers explicit deny and omission from an allowlist.
Incompatibility still takes diagnostic precedence when an offer is also denied.
Policy preflight rejects missing selected keys before resolution, so there is no
unreachable missing-offer reason. The retained contract error remains its original
generic definition-mismatch leaf; no richer fields or causes are fabricated.

Emission diagnostics use only the compiled path/output declaration and envelope
`TypeIdentity`. Local identities remain opaque Rust `TypeId`s; wire identities are
canonical schema identities. No payload formatting, schema codec, readiness callback
or transport preparation is added. Valid emissions retain their existing path.

## Independent Critique And Acceptance

Preimplementation review supported the design, calling out the intentional public
field-type change, the generic leaf's limits and request-local recovery acceptance.
It confirmed that policy preflight justifies indexing the selected offer, and that
denial must remain sourceless rather than inventing a cause.

Existing Kernel contract-catalog/import tests now retain typed failures and check
requirement/offer context, reason variants, direct cause downcasts/pointer identity,
leaf termination, unchanged display, both policy-denial forms, incompatible-plus-
denied precedence and zero preparation callbacks. Unknown policy keys still reject
as `Invalid` before resolution.

Existing stream tests cover unknown output names, local/local and both local/wire
directions, wire/wire mismatch with malformed bytes, non-Debug local payloads,
unconnected/explicit-discard validation, and zero invalid publication. The existing
request-recovery fixture adds invalid identity/output emissions under both FIFO and
bounded-lane policies, checking request failure context, no prepared/queued invalid
packets, fresh subsequent process state and unchanged unrelated process state.

These close bounded Q6/Q7 diagnostic defects, not the whole findings. General
readiness/liveness explanations and string-based process/runtime failures remain;
the authoritative 33-issue counts and scores do not change.

Final independent diff review found no actionable defects. Full locked workspace
tests with default and no-default features, workspace formatting and strict
all-target Clippy pass locally. Malformed bytes with a mismatched identity test
metadata-only diagnosis, not byte-integrity validation of a matching schema.
Hosted same-head acceptance is still required; earlier passing commits do not
certify this batch.
