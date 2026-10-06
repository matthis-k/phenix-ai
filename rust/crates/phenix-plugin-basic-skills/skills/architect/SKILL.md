---
name: architect
description: "Choose system boundaries and canonical contracts before implementation. Use when the user asks to design architecture, APIs, protocols, plugin systems, module boundaries, ownership, data flow, migration strategy, or a structural refactor; also use to compare architectural approaches. Skip for local implementation choices that do not change system semantics."
---

Design the smallest coherent architecture that satisfies the settled objective.

## Process

1. Recover requirements, non-goals, existing contracts, constraints, and relevant prior decisions.
2. Map current ownership, data flow, dependency direction, lifecycle, persistence, and authority boundaries.
3. State the invariants the design must preserve.
4. Produce alternatives only where a real tradeoff exists. Keep equivalent variants out.
5. Compare alternatives against the invariants, migration cost, failure modes, testability, and conceptual load.
6. Choose one canonical contract. Name what it replaces and what must disappear.
7. Define migration boundaries and validation seams without turning the design into an implementation checklist.

## Design rules

- One owner for each semantic fact.
- Prefer deep modules: small interfaces hiding substantial policy or mechanism.
- Keep adapters at boundaries. Do not leak transport, provider, or frontend concerns into domain semantics.
- Make invalid states hard to represent when the domain can express the invariant directly.
- Separate durable semantics from process-local handles and caches.
- Preserve explicit authority and provenance across boundaries.
- Avoid compatibility layers in prerelease code unless an external constraint requires one.

## Handoff

Return the chosen model with:

- canonical terms;
- ownership and responsibilities;
- interfaces and data flow;
- invariants;
- rejected alternatives with decisive reasons;
- migration shape;
- validation seams;
- unresolved decisions.

If unresolved product or preference choices remain, use `grilling`. If the architecture is settled and needs decomposition, use `plan`.
