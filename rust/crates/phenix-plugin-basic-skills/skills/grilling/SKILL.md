---
name: grilling
description: "Resolve ambiguous or risky product, architecture, or implementation decisions through evidence-backed questioning. Use when the user asks to grill, pressure-test, clarify, stress-test, challenge assumptions, or when implementation is blocked by unresolved choices. Skip when intent and acceptance criteria are already settled."
---

Resolve decisions, not facts the environment can answer.

## Process

1. Recover settled context first: objective, prior decisions, code, tests, docs, and current behavior.
2. Build a **decision graph**. An edge means one decision depends on another.
3. Compute the current **frontier**: unresolved decisions whose prerequisites are settled.
4. Resolve environmental facts yourself. Ask the user only for choices that require judgment, preference, or product intent.
5. Ask the whole frontier in one round. Number every question. Give a recommended answer and the consequence of choosing differently when material.
6. Apply the answers, update canonical vocabulary, recompute the frontier, repeat.

## Question format

```text
**Q<number>** - **<decision>**: <question and relevant options>

Recommendation: <answer>
Reason: <why this best fits current evidence>
```

Keep dependent questions out of the round until their prerequisites settle.

## Evidence

Inspect the repository, tests, history, documentation, and authoritative external sources when needed. Separate:

- **fact**: directly established by evidence;
- **decision**: chosen by the user or an already authoritative artifact;
- **inference**: conclusion from facts;
- **unknown**: missing evidence that does not require user preference.

Do not turn an unknown fact into a user question.

## Completion

Stop when no unresolved decision can materially change scope, semantics, interfaces, acceptance criteria, or migration strategy. Produce a compact handoff of settled decisions, canonical terms, acceptance implications, and remaining factual unknowns.

Do not implement unless the user explicitly asks to continue into implementation.
