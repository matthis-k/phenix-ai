# M1-only runtime transition

This transition follows the 12 October decision and the prepared-effect work in
#779. M1 is the sole active contract, wire and native runtime implementation.
Historical commits retain M0 acceptance evidence. Its behavioral guarantees
remain contemporary conformance requirements.

## Canonical data

ContractMetadata and ValueSchema own contract meaning. StaticSchema encodes those
schemas at compile time. PreparedOperation retains the selected OperationSchema
and native/cell target. Imports bind named slots, selected export slots and exact
revisions. Preparation computes ceilings, selected bindings, lifecycle order and
generation inputs once. Artifact verification never creates a synthetic contract.

An import may declare an exact provider identity. Each canonical import record
contains `provider`, either Null or that identity. Generated require declarations
accept `provider = "id"`; preparation rejects substitutions or selections that
violate it. This retains the proven exact-import guarantee in M1 metadata.

Composition is a flat phenix.composition.v2 document with artifacts, definitions,
entry, slots, grants, excluded, substitutions, authority and optional resource.
Every provider selection is explicit. Required One/Many selections impose native
lifecycle dependencies. Optional slots retain their optional lifecycle semantics
after selection. Cell targets do not require native invocation ordering.

## Native and wire boundary

ABI 3 is the only native table. Native and definition-only packages use the same
metadata semantics with their existing packaging distinction. Portable values use
the reference-capable PV 02 00 wire prefix. Structural schema encoding keeps its
frozen internal representation and hash; it is not a selectable value wire format.
One Portable trait and one derive generate typed contract values.

Native scopes remain thread-authenticated, inherited and attenuated. Native
pending children reuse the existing ticket tree and physical leases. Their host
commands carry the import slot, operation and request or child ticket. Poll/cancel
authenticate root, parent, caller, selected slot, provider and operation. Revoked
scopes permit cleanup of existing children while admitting no new effects.

## Acceptance

Port provider selection, schema conflict, authority/resource denial, lifecycle
rollback, pending cancellation/wakes, generation pinning and safe retirement to
M1 fixtures. Independent offline, wire and C ABI gates remain. Remove legacy
format tests and product gates only after their behavioral assertions are owned
by the contemporary suite. The A0 agent remains a fixture outside the kernel.
G1-G4 begin after this transition and its exact-head conformance evidence pass.
