---
name: ship
description: "Take a completed change through PR readiness and integration. Use when the user asks to ship, finish a PR, prepare or update a PR, handle review feedback, get CI green, rebase for merge, make a branch merge-ready, or merge completed work. Use after implementation; do not use to select the next unrelated PR or objective."
---

Integrate the exact assigned change. Shipping does not broaden scope.

## Start

1. Recover the PR or branch objective, acceptance criteria, dependencies, current head, base branch, review state, and prior verification.
2. Compare the branch with its base. Identify unrelated changes, missing commits, stale base assumptions, and unresolved review threads.
3. Confirm the PR description still states the implemented contract and dependencies accurately.

## Ready the PR

1. Ensure substantive validation runs against the actual head. If draft state suppresses required CI, mark actively worked PRs ready before relying on CI results.
2. Rebase or update onto the intended base when required. Re-verify evidence invalidated by the new source state.
3. Run or inspect all required CI jobs. Treat skipped, cancelled, action-required, or missing jobs according to the repository's actual policy; do not call them green by omission.
4. Resolve review findings inside the assigned scope. Route implementation changes through `implement` and rerun `verify` afterward.
5. Keep the PR description current with dependencies, completed scope, and decisive validation evidence.

## Merge gate

Merge only when:

- the assigned objective is complete;
- required acceptance criteria pass;
- required CI is green for the exact merge candidate;
- blocking review findings are resolved;
- the branch is based on the intended parent/base;
- no unrelated work has entered the change.

If any gate fails, report the concrete blocker and keep working the same PR when repair is possible.

## Boundary

Do not select, open, or advance to another PR after shipping. PR selection belongs to the caller's startup/coordination phase. One worker stays on one selected PR for its lifetime.
