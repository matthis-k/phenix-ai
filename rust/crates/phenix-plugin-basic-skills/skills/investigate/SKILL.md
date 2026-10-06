---
name: investigate
description: "Establish missing facts before a decision or change. Use when the user asks to investigate, research, trace, diagnose, explain why or how something works, map a code path, find a root cause, or figure out unknown behavior. Use before implementation when important facts are missing; skip when the task is already mechanically specified."
---

Answer the question from evidence. Keep investigation read-only unless the user explicitly asks for diagnostic instrumentation.

## Process

1. State the concrete question and what evidence would settle it.
2. Inspect the smallest authoritative sources first: implementation, tests, configuration, logs, history, upstream docs.
3. Split independent lines of inquiry when parallel research can reduce uncertainty without duplicating work.
4. Trace behavior end to end. Prefer concrete call paths, state transitions, data flow, and reproductions over summaries.
5. For failures, build the narrowest reproducer and rank falsifiable hypotheses. Seek evidence that distinguishes them.
6. Stop once the question is answered strongly enough to unblock the next decision or action.

## Evidence model

Keep these distinct:

- **observation**: directly seen in code, runtime output, tests, docs, or history;
- **inference**: conclusion supported by observations;
- **hypothesis**: plausible explanation still needing discrimination;
- **unknown**: evidence still missing.

Cite exact files, symbols, tests, commands, revisions, logs, or authoritative sources when available.

## Handoff

Return:

1. answer;
2. decisive evidence;
3. causal path or mechanism when relevant;
4. remaining uncertainty;
5. next action only if the evidence supports one.

Do not invent a product decision. If the evidence exposes a real choice, hand that choice to `grilling` or `architect`.
