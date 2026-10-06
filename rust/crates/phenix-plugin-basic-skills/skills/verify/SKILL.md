---
name: verify
description: "Prove whether a change or artifact satisfies its contract. Use when the user asks to verify, validate, confirm, prove, test, check CI, check completion criteria, determine whether work is done, or before making a final completion or merge claim for nontrivial implementation. Verification observes and judges; it does not repair failures."
---

Verify the exact state that will be claimed as complete.

## Process

1. Recover the objective, acceptance criteria, relevant decisions, and exact source or artifact revision.
2. Build an acceptance matrix: criterion -> evidence needed -> evidence observed -> verdict.
3. Inspect implementation and tests where needed to confirm the feedback actually exercises the claimed behavior.
4. Run the narrowest decisive checks first, then the surrounding deterministic suite required by the changed boundary.
5. Check negative and failure behavior when the contract includes rejection, authority, recovery, persistence, or lifecycle semantics.
6. Re-check any evidence invalidated by later source changes.

## Evidence rules

- A passing command proves only what that command exercises.
- Existing green tests do not prove new behavior unless their assertions cover it.
- A mocked path does not prove a real integration boundary when the criterion requires integration.
- Manual inspection is valid evidence for structural claims when the inspected property is explicit and stable.
- CI status is evidence only for the exact commit it ran against.

## Verdict

Each criterion is one of:

- **pass**: decisive evidence establishes the criterion;
- **fail**: evidence contradicts the criterion;
- **blocked**: required evidence cannot currently be obtained;
- **unproven**: no decisive evidence exists yet.

Completion requires every required criterion to pass. Report exact commands, tests, revisions, and observations that support the verdict.

If verification fails, return the failure evidence to `implement`. Do not weaken the criterion or test to manufacture a pass.
