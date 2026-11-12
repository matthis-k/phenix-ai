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


## Phase 1 revision — regression counterevidence

The first implementation of the proposal changed every public `inject` and
`inject_request` into transport-only admission. Exact-head Nix checks exposed
three existing behavioral regressions in `fbp_streams`:
`default_any_rotates_between_available_ports`,
`cancellation_disposes_owned_queues_but_does_not_claim_to_preempt_runner`,
and `active_cancellation_defaults_to_fail_session_and_host_failure_can_opt_into_restart`.
Eager reservation during ordinary injection is an established behavioral guarantee
for capacity and active-request cancellation. **The original proposal is rejected.**
These are not historical tests to weaken, and there is no accepted tradeoff to
remove their behavior.

Revised option comparison:
- Restore eager refresh for ordinary `inject` and `inject_request`, preserving
  the existing public contract. No new session mode or source policy.
- Introduce one explicitly named transport-only `enqueue` admission operation.
  It delegates to the **same** `inject_scoped` validation and atomic fanout,
  but does not run `refresh`. This is a timing choice, not a second transport
  implementation, scheduler, source of authority, or changed readiness policy.
- `Kernel::drain` alone uses `enqueue` under its producer mutex. Kernel's
  regular external `inject` remains eager, as no producer lock is held.
  `Kernel::run_steps` and `NetworkSession::state` already drive `refresh`;
  the kernel notifies its wake listener after accepted enqueues.

New acceptance gates: pre-existing `fbp_streams` tests must pass without edits,
a direct test shows ordinary inject eagerly evaluates readiness, another shows
enqueue does not, and the kernel lock regression checks explicit scheduling.
`enqueue` must preserve failed ownership, schema, scope and queue credit exactly.
This is a narrowly justified public operation; do not introduce more entry paths.

The first implementation is superseded by this revision, not accepted retroactively.


## Phase 1 revision 2 — kernel capacity regression

The revised transport-only `enqueue` eliminated callback-under-lock but initially
changed `native_sessions::full_retains_exactly_one_event_and_does_not_repoll_or_execute`:
the kernel admitted only one event instead of two (one actively reserved input
plus one queued input). This is an established producer-admission contract, not
an optional throughput gain. **Simply replacing `inject` with `enqueue` is
insufficient.**

Revised implementation: keep the same `enqueue` operation and shared transport
authority, but move `ProducerState` locking inside each polling iteration.
After a successful enqueue, release that lock and call the existing
`NetworkSession::state` refresh under the session mutex **before polling the next
event**. On Full, retain the single event and do not repoll. Idle polls still
consume poll budget; terminal failures still detach and drop producers after
their lock is released. `KernelHost::publish` may retire between iterations;
the next iteration checks `retired` before admitting work.

This change must preserve both the existing two-admission boundary and the new
no-readiness-under-producer-lock test. There is no second scheduler, no bespoke
capacity reservation, and no extra producer-in-flight authority. Producer::poll
itself remains a callback under its own inventory lock; that separate limitation
is explicitly not addressed here.


## Phase 1 revision 3 — restore the proven kernel boundary

Actual exact-head regression `generation_retirement::blocked_poll_finishes_its_drain_before_publication_retires_the_source` rejects per-poll release of `ProducerState`. Retirement must wait for the **whole** active drain call, including all its polls. The short unlock/refresh model cannot preserve this guarantee, irrespective of single-event capacity/fairness.

Decision: **reject and revert the producer-lock experiment as a whole.**
Restore the baseline `Kernel::drain`, public eager `NetworkSession::inject`,
and eager `inject_request` unchanged. Remove the temporary `enqueue` entry path
and lock-specific test. Retain only the independent, static-options startup
preflight, which prevents invalid configuration from invoking plugin factories
without changing runtime scheduling or retirement.

Phase 0 recheck: The lock inversion is a real issue, but solving it safely
requires a separately reviewed whole-drain ownership/lifecycle design that can
exclude plugin callbacks from critical sections *without* permitting retirement
mid-drain or adding a competing scheduler. Neither naive lazy injection nor
per-poll lock release meets established invariants. This is an unresolved defect,
not an accepted limitation or claimed completed fix.

Final acceptance for this iteration: unchanged historical Graph/Kernel behavior,
zero factory spawns on invalid static configuration, ordinary valid session
spawns, strict rustfmt/Clippy, full workspace test/Nix matrix, and exact-head CI.
