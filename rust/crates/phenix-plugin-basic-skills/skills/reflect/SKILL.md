---
name: reflect
description: "Extract reusable lessons from completed, difficult, or failed work. Use when the user asks to reflect, run a retrospective or postmortem, capture lessons, improve agent behavior, explain repeated mistakes, or preserve knowledge after a surprising implementation or investigation. Skip routine work with no reusable learning."
---

Promote only knowledge that should change future behavior.

## Process

1. Recover the objective, decisions, attempts, failures, verification evidence, and final outcome.
2. Identify what was surprising, repeatedly expensive, easy to get wrong, or unavailable from the environment by simple inspection.
3. Separate reusable knowledge from task-local history.
4. Check whether the lesson already has one canonical home.
5. Choose the smallest durable destination that will put the lesson in context when it matters.
6. Propose or apply the minimal change needed. Remove stale or conflicting guidance rather than layering another instruction on top.

## Promotion targets

Choose by meaning:

- **skill**: reusable process or judgment that should trigger for a class of tasks;
- **project instruction/context**: project-specific convention, invariant, rationale, or gotcha;
- **test**: behavior that should become mechanically enforced;
- **tooling/configuration**: deterministic mechanism better enforced by the environment;
- **decision record**: durable rationale for a consequential settled choice.

Prefer environment enforcement over prose when a machine can enforce the rule cheaply and reliably.

## Keep out

Do not promote:

- transient branch state;
- one-off filenames or commit IDs with no lasting meaning;
- generic advice the agent already follows;
- a symptom when the reusable lesson is the underlying cause;
- duplicated guidance whose source of truth already exists.

## Handoff

Return each proposed lesson with its evidence, future trigger, destination, and exact change. If nothing would materially improve future behavior, say so and create no permanent guidance.
