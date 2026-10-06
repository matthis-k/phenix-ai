---
name: pickup
description: "Reconstruct prior work and continue from its true endpoint. Use when the user asks to resume, continue, pick up, take over, recover a handoff, find where a previous worker stopped, or continue an existing PR, issue, objective, plan, or interrupted session. Use before changing existing work when prior state matters."
---

Reconstruct state from durable evidence and the current environment. Do not restart from the old transcript.

## Recover

1. Identify the exact assigned objective, PR, issue, or plan. Keep that scope fixed.
2. Recover settled decisions, acceptance criteria, plan progress, prior attempts, failure summaries, and verification evidence when available.
3. Inspect the current branch/worktree, diff, commits, tests, CI, review feedback, and relevant repository state.
4. Compare durable records with the current source state. Current source wins for what exists now; durable decisions and objectives remain authoritative for intended semantics.
5. Separate confirmed completed work from attempted, stale, invalidated, or merely claimed work.

## Find the frontier

Determine:

- last confirmed completed outcome;
- current incomplete acceptance criteria or plan steps;
- active blocker or failure cause;
- evidence invalidated by later changes;
- smallest next settled action.

Do not redo completed work unless current evidence shows it is invalid.

## Continue

Route by the recovered frontier:

- missing facts -> `investigate`;
- unresolved user choice -> `grilling`;
- unresolved structural contract -> `architect`;
- missing decomposition -> `plan`;
- code changes -> `implement`;
- independent challenge -> `interrogate`;
- proof -> `verify`;
- PR integration -> `ship`.

When the assignment is a PR or objective, continue that same assignment only. Do not choose a new one after completion; return control to the caller's selection phase.

## Handoff

Before acting, be able to state the recovered objective, confirmed state, remaining frontier, and next action in compact form. If records disagree, resolve the disagreement from authoritative evidence instead of guessing.
