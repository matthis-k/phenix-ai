# FBP follow-up: admission and startup purity

Status: Phase 1 design note; against commit `16e77f6c69e31d7ddb4be042737e7d493f214284`.
Companion critique: [fbp-followup-adversarial-20261111.md](fbp-followup-adversarial-20261111.md).

## Phase 0 — constraints and actual problem

The accepted [FBP design](fbp-design.md) requires one native network runtime, persistent bounded queues, single-owner process state, explicit authority, replayable composition, and no privileged agent path. This change must not introduce a scheduler, reorder FIFO packets, alter grants, or execute application code while holding producer inventory locks.

Two observable problems in the delivered implementation:
1. `Kernel::drain` holds `ProducerState` while calling `NetworkSession::inject`. Injection calls `refresh`, which can synchronously invoke user `ReadinessPolicy::select`. Reentrant readiness that inspects producer state can deadlock even if admission was otherwise nonblocking.
2. `NetworkSession::new` spawns factories before validating static session options (unknown process paths, invalid readiness, invalid discard outputs). Invalid configuration can therefore execute plugin factory code unnecessarily before returning a failed session.

No change option: leave both, documenting restrictions on callbacks and factories. This permits preventable lock inversion and startup side effects, and is not acceptable for primary plugin use cases.

## Phase 1 — comparison and selected design

| Option | Behavior | Tradeoff | Decision |
|---|---|---|---|
| Baseline | Eager refresh inside admission; factories before validation | Simple code, but permits callback under producer lock and unnecessary side effects | Reject |
| Extension A | Add a second deferred-admission API exclusively for Kernel | Preserves eager external inject, but duplicates entry semantics and can drift | Reject |
| Extension B | Split producer polling and admission into unlocked phases | Avoids both callback and poll reentry, but requires a new producer-in-flight/retirement authority | Defer to separate review |
| Proposed | Admission only changes owned FIFO state; all readiness selection occurs when explicitly driving/querying session. Preflight static options against compiled declarations before spawning any factory | Readiness failure becomes observable on drive rather than during `inject`; startup keeps the failed-session API | Accept |

**Invariants:**
- `inject` and `inject_request` atomically enqueue or return the original value on Full/error. Neither calls process factories, readiness policies, or process steps.
- `state`, `next_job`, `run_until_idle` and the existing driving/closing paths continue to call `refresh`; delivery, fairness, wakeup, and EOS semantics remain unchanged.
- Static configuration validation (readiness names/threshold/required/optional uniqueness, resource paths, discard targets) precedes all factory spawns. Failed sessions report meaningful diagnostics as before.
- The existing producer poll callback still executes under the producer mutex. **This limited change does not claim all plugin callbacks are reentrant-safe.**
- Host composition and kernel retirement remain exactly as before. Existing custom readiness may not reenter the session mutex itself.

**Acceptance evidence:** regression asserting admission never invokes custom readiness but the next scheduler drive does; regression proving invalid options cause zero factory spawns; existing streaming, request, kernel, storage and workspace tests; exact-commit CI.

## Deferred independent design requirements

The [FBP migration ledger](fbp-migration.md) and prior review identify two substantial remaining requirements, not waived by this patch: (1) bounded correlation/demultiplexing for crossed request streams while preserving FIFO, and (2) explicitly authorized nested edit points or public subgraphs, without making private composite internals writable by default. Each needs its own baseline/extension/redesign comparison, adversarial review and executable agent-loop use case. No unsound generalization is authorized by this note.

## Phase 4 gate

Review the actual diff against every invariant, check unexpected changes to response timing, no duplicate authorities, no new APIs/compatibility paths, meaningful tests, and exact-head CI. Any new limitation outside the Phase 1 scope returns to design.
