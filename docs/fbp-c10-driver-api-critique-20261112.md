# C10 Phase 2 — adversarial critique

Status: separately written challenge to the C10 design before implementation.

1. A call to `run_until_complete` in a test does not mean the runtime actually
   waited until completion; replacing the name must not modify the program's
   bounded progression.
2. Do not weaken assertions: `Completed` checks after EOS must remain,
   and other tests should retain `Waiting` where applicable.
3. Search all graph sources, examples, integration tests and dependent crates;
   a leftover call is a compilation defect, not an accepted limitation.
4. Do not leave a deprecated forwarding wrapper — that defeats consolidation.
5. Do not implement real blocking completion in this cleanup. Proper waiting
   requires a separate host wake/driver design and explicit cancellation.
6. Avoid claiming this resolves shared-session head blocking, per-request
   isolation or H3b callback reentrancy.

Verdict: accepted for Phase 3 if the sole production change is alias removal
and every caller is migrated. Final-head CI must pass before marking C10 closed.
