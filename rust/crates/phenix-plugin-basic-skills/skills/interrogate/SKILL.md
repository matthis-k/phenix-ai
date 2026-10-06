---
name: interrogate
description: "Independently challenge a code change, design, spec, plan, or PR. Use when the user asks to review, critique, audit, red-team, interrogate, challenge assumptions, find gaps, or assess whether work is actually sound; especially useful before merge or after implementation. Skip for fact-finding alone; use `investigate` for that."
---

Try to falsify the artifact's claims. Stay independent of the reasoning that produced it.

## Process

1. Recover the stated objective, contract, acceptance criteria, and exact artifact or source revision under review.
2. Inspect primary evidence yourself. Do not treat the author's summary as proof.
3. Test the strongest claims first: correctness, ownership, invariants, failure behavior, migration containment, tests, and requested semantics.
4. Search for contradictions, hidden assumptions, duplicated contracts, missing consumers, stale compatibility paths, and cases where tests prove less than claimed.
5. Distinguish a real defect from preference, style, or speculative risk.
6. For each real finding, establish evidence, impact, and the smallest credible correction.

## Finding standard

A finding must contain:

- **severity**: blocker, major, minor;
- **claim**: what is wrong;
- **evidence**: exact code, behavior, test, diff, or contract;
- **impact**: what can fail or become ambiguous;
- **correction**: concrete repair direction.

Reject a finding when you cannot establish the causal or contractual link.

## Review dimensions

Keep conformance and engineering quality distinct:

- **conformance**: does the artifact satisfy the requested behavior and acceptance criteria?
- **engineering**: are ownership, boundaries, state, failure behavior, tests, and maintenance coherent?

## Completion

Return findings ordered by severity, then state what you tried to falsify but could not. Do not edit the artifact unless the user asks for fixes; route repairs to `implement` and final proof to `verify`.
