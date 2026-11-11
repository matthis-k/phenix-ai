# Adversarial critique: explicit nested replacement seams

Phase 2. Independently challenge
[the design note](fbp-nested-replacement-design-20261111.md) before implementation.

1. **Manifest authority:** An exact host grant is necessary but not sufficient.
   A module must opt into making each child visible for replacement. A forged
   `Footprint::Component("module/private")` must still fail if unexposed.
   Conversely an exposed child with no host grant must remain unauthorized.
2. **Deep paths:** A single `allow_replace` on an outer component must not
   accidentally authorize grandchildren. Test `outer/inner/leaf` with only one
   layer exported and require both intermediate seams.
3. **Shape and frozen obligations:** Replacements must preserve the declared
   interface and every old claim/final-state requirement, not just the leaf
   payload's claims. The compiler must reject a replacement that strips a
   historically required contract. New leaf claims must also survive replay.
4. **Provenance and identity:** Do not stamp parent origin with the replacing
   plugin, and do not reuse the parent's unchanged definition key for a
   different composite body. A nested replacement must not forge an ancestor
   or descendant origin; the existing scan applies to the entire payload.
5. **Authoring capabilities:** `allow_replace` must accept only a live handle
   issued by the same builder. It must participate in the additions transaction
   snapshot/rollback and remain meaningful across `NetworkBuilder::from_def`.
6. **Editing boundaries:** Nested `Add`, `Connect`, `Disconnect`,
   `Splice`, `ExposeInput`, `ExposeOutput` and `AddSource` are still
   disallowed. Do not expose a general path-mutation function as an API.
7. **Atomicity:** A failed or defeated plugin registration must not mutate an
   `Arc<NetworkDef>` shared by the base graph or prior published generation.
   Existing sessions must run unchanged after failed register or successful
   recomposition.
8. **Complexity check:** Prefer one immutable declaration manifest and one
   recursive copy-on-write leaf-replacement helper; do not introduce a new
   graph representation, patch language, adapter trait, or execution policy.

**Phase 2 verdict:** Conditionally approve the exact-seam proposal for
implementation only if these tests pass. Arbitrary private nested mutation
and unrestricted deep topology patches remain blocked, not tradeoffs.


## Phase 2 amendment — conflict and frozen-contract counterexamples

- A plugin replacing `m` and an unrelated one replacing
  `m/inner/leaf` must not both be selected as independent non-conflicting
  writes. Require priority-based whole-registration exclusion or explicit
  dependency, even though their *grants remain exact and distinct*.
- Existing ancestor contracts and new leaf claims must be checked in the
  final compiled topology, including claims that the old child required.
  Verify rejection preserves the last published plan and permits subsequent
  corrected registration.

These tests are **mandatory** for acceptance, not deferred improvements.
