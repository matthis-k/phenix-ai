# FBP: explicitly replaceable nested components

Date: 2026-11-11. Baseline: `14f022e471609059cc1bab92d17cbaa1546272e0`.
References: [FBP architecture](fbp-design.md), [migration ledger](fbp-migration.md),
[agent composition proof](fbp-agent-composition-design-20261111.md).

## Phase 0 — requirement and no-new-concept test

Requirement: plugin authors can intentionally make a stage inside a reusable
composite replaceable, allowing host-authorized plugins to substitute behavior
without copying the entire preset's internal graph. A plugin cannot rewrite
someone else's arbitrary private subgraph. Existing `Edit::Replace` already
preserves typed public interfaces and exact `Footprint::Component(path)`
authority. Only the nested scope address and visibility are missing.

No new feature: currently any plugin can replace the *entire root composite*
with an interface-identical new definition. This works but forces unrelated
internal topology duplication, complicates independent plugins and weakens
encapsulation. A general nested Add/Connect/Splice would require a larger graph
patch/port authority redesign; do not build it speculatively.

## Phase 1 — alternatives

| Option | Benefit | Limitation | Decision |
|---|---|---|---|
| Baseline: whole-composite Replace | Already implemented, no new concept | Copies unrelated internals; no narrow extension seam | Retain as fallback |
| Authorize all nested Replace by exact host grant | No new manifest | Mutates provider-private implementation without opting in | Reject |
| Nested Add/Connect/Splice | Full deep rewiring | More complex scope, source/export contracts, provenance and grant resolution | Defer |
| **Explicit nested Replace seam** | Small additive boundary; avoids privileged plugins; preserves parent connections | Changes one existing nested component only; not arbitrary topology edits | **Selected** |

Each `NetworkBuilder` can mark an existing **child handle** replaceable using
`allow_replace(&ComponentRef)`. This produces an immutable, name-only manifest
on its `NetworkDef`: `replacement_points: BTreeSet<String>`. Authorizing a
path `preset/loop/model` requires both `preset`'s child network to expose
`loop` and `loop`'s child network to expose `model`. No wildcard ancestry
and no implicit inheritance. The root component itself remains editable as
before.

A registered `Edit::Replace { path: "preset/loop/model", definition }`
succeeds **only if**:

1. The trusted host explicitly granted the exact
   `Footprint::Component("preset/loop/model")`.
2. Each traversed composite exposed that exact immediate child as replaceable.
3. The replacement has the **same interface** as the existing declaration,
   including transport type identities.
4. The payload passes the existing origin-forgery scan; only the replaced
   declaration is stamped with its registration origin. Ancestor origin and
   semantic claims remain unchanged.
5. Existing frozen contract requirements still hold after the nested update.
   Additional claims on the replacement are checkpointed as obligations.
6. Replay recompiles the complete immutable network; failure leaves the
   published plan and registrations untouched.

Implementation: clone only the `Arc<NetworkDef>` ancestors along the target
path, replace the leaf, preserve the ancestors' interfaces and origin, update
declaration identity for changed composite bodies, and compile with existing
checks. No new execution semantics, patch verbs, renderer, graph scheduler or
host authority type. `Edit::Add`, `Splice`, edges and exports remain
root-scoped and must still fail on private paths.

## Explicit tradeoffs and excluded use cases

- A replaceable child is an **opt-in extension seam**, not arbitrary permission
  to edit its private siblings, edges, exported ports or source starts. High-level
  graph customization requiring insertion still uses root-public stages or
  replacement of a stage by a composite with additional internal processing.
- Existing compiled sessions keep their immutable topology; edits apply only to
  new candidate generations. No hot state migration or synchronous graph patching.
- A change to a parent composite body results in a fresh definition identity,
  while child declaration origin reflects the actual registering plugin; no
  filesystem path or plugin namespace becomes implicit authority.
- This does not solve cross-request correlated FIFO head blocking. That remains
  an independent primary-use-case issue, not an accepted limitation for all agents.

## Phase 4 acceptance

Add execute-and-assert tests for nested replacement and revocation; prove
private siblings remain private even with a forged exact grant; reject
interface mismatch and unauthorized changes atomically; test a 2-level seam and
stale/foreign authoring handles. Existing `fbp_authority`, `fbp_components`,
integration tests and full exact-HEAD CI must pass. Inspect the final diff for
unintended new APIs/hidden authority.


## Phase 1 amendment — ancestor/descendant write conflicts

Critical edge case: two independent registrations can have distinct *exact*
component grants (e.g. `m` and `m/inner/leaf`) while modifying overlapping
physical definitions. Their writes must not silently erase each other. Extend
the **existing** composer conflict/priority selection predicate to treat a
component path as overlapping any actual descendant path separated by `/`.
This does **not** broaden authorization: each registration still needs its own
exact `Footprint::Component(path)` grant and each nested path still needs the
composite's explicit seam.

Equal-priority independent ancestor/descendant edits conflict. At unequal
priority, select the winning registration as a whole, deactivating its loser.
Explicit dependency allows an ancestor replacement followed by a nested edit,
subject to the new parent's seam and the same post-replay contract checks.
Root component names sharing a mere string prefix, e.g. `m`/`module`, do not
overlap. Integration tests exercise all three cases.

The bounded overlap check uses the existing footprint sets, not a new
authority or edit-ordering mechanism.
