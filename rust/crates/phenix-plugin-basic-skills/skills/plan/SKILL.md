---
name: plan
description: "Turn settled intent into an executable dependency-aware plan. Use when the user asks to plan, break down work, create an implementation plan, define milestones, sequence a migration, or prepare a nontrivial change before coding. If requirements or architectural choices are still open, use `grilling` or `architect` first."
---

Plan strategy and dependencies. Keep execution mechanics out of the semantic plan.

## Process

1. Recover the objective, acceptance criteria, non-goals, settled decisions, and current repository state.
2. Identify the smallest end-to-end slices that move the system toward the objective.
3. Add prerequisite structural work only when it makes a later slice simpler or safer.
4. Build a dependency graph. Mark work parallel only when the slices are genuinely independent.
5. Give every step a concrete outcome and acceptance evidence.
6. Check the plan against current code so steps name real seams instead of imagined abstractions.
7. Remove steps that merely restate the objective, duplicate another step, or exist only to create process overhead.

## Plan contract

A plan contains intended strategy, step dependencies, and completion evidence. It does not encode model choice, agent assignment, authority, retries, timeouts, or scheduling policy.

Prefer vertical slices over horizontal layer-by-layer work. Keep one step large enough to produce a meaningful verified state, but small enough that failure has a clear boundary.

## Ready test

A plan is ready when an implementer can start the first available step without making a new product or architecture decision.

Return:

- ordered or dependency-linked steps;
- acceptance evidence per step;
- explicit blockers;
- safe parallelism;
- risks that change implementation order.

If a step discovers a decision rather than a fact, route back to `grilling` or `architect` instead of hiding the decision inside implementation.
