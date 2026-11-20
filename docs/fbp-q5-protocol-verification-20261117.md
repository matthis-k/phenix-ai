# Q5 Protocol Verification Design

## CURRENT Implementation Checkpoint — Local Evidence, Acceptance Pending

Date: 2026-11-17. This section supersedes the original Phase 0/1-only status
while preserving the requirements and design analysis below. The assembled tree
contains implementation and focused local evidence, but no exact committed-head
acceptance is claimed. **Q5 remains OPEN. Final independent reviews report no
findings after the public identity/composition extension and evidence reconciliation.
H3b is outside this
Q5-specific checkpoint and receives no automatic closure, credit, or status
change. No ledger or score update is made.**

### Implemented core and locally verified counts

| Slice | Current source and focused local checkpoint | Claim boundary |
| --- | --- | --- |
| Production ownership extraction | `packages/phenix-kernel/src/producer_ownership.rs` now owns the atomic word, `DriverScope`, `DirectDrainScope`, and private `DriverAdmissionError`. `lib.rs::enter_driver` maps the private errors to the existing public `DriverError` variants without changing their direction or guard order. | This is exact-source primitive verification, not a model of `Kernel::run_until`, retirement, wake registration, or arbitrary graph atomics. |
| Exact-source Loom | Seven new producer-ownership properties plus the three original `ExclusiveSlot` properties: **10 properties, 472 completed schedules** (411 new + 61 original). Every property reports `inconclusive=false`. Bounds are preemption 2, 1,000 branches, 20,000 permutations, checkpoint interval 1, and **no duration cap**. | Bounded source-level schedules only; not exhaustive arbitrary concurrency, whole-Kernel verification, or a fairness/liveness proof. The conservative `MAX-1` cap test did not trigger. |
| Native ownership and retirement | **61 tests passed locally**: 33 Kernel library tests (29 root + 4 host), 16 `native_sessions`, 2 `generation_retirement`, and 10 `wake_host`. The gate-queued case observes ownership **2 -> 1 -> 0** only as the first callback exits and the queued callback later exits. | While either direct owner remains, driver admission returns `DrainInProgress`; wake version/notifier, `driver_wake`, and producer registration remain untouched. The final valid driver is then admitted. This is a real two-drain/two-callback witness, not a general schedule proof. |
| Private real-handoff model | `process/handoff_model.rs` has **7 passing tests and 21 anchors**, including `PostCompleteIdle`. Each anchor has 73 suffixes: **1,533 histories, 2,856 operation attempts, 611 accepted and 2,245 refused**. Each of the eight operations has **357 attempts**. | Real `ProcessHandoff`, jobs, completions, failures, contexts, and leases are checked after every frame. This is a finite sequential source matrix, not Loom and not arbitrary thread interleaving coverage. |
| Execution outcome matrices | All eight real execution outcomes (`Emit`, `Yield`, `WaitInput`, `WaitInputs`, `WaitWake`, `Complete`, `Finished`, and typed failure) each exercise rightful rejection, fresh redispatch, foreign unrun-job return, foreign completion return, and rightful accepted completion. The same-cancellation matrix has **8 intact stale returns and 8 exact return-drop cleanups**. | These are real owned-object executions in the private model. They do not by themselves prove every public session/worker composition or external effect policy. |
| Public handoff/transport history | `packages/phenix-graphs/tests/fbp_handoff_history.rs` has **8 passing tests** for repeated foreign/stale refusals, rightful acceptance, publication acknowledgement, real transport backpressure, typed panics/cleanup, guards, and leases. A real `ConnectionSet` returns `Full` on four retries, commits once after capacity is released, then returns `AlreadyCommitted`; the session backpressure case retries without new steps/publications or repeated effects. | No cloned/reconstructed completion or replayed process action is used. This verifies the named concrete schedules, not every transport or session interleaving. |
| Existing worker baseline | The original 48 persistent-session rounds remain split 16/16/16 across replacement-first, old-return-first, and fresh-dispatched orders. Two leased publication/recovery orders add `E<P<R<F` and `E<R<P<F`; these are the workflow's **five deterministic baseline frames**. | Old external effects survive cancellation exactly once, old output cannot enter the replacement, and returned old objects retain process/context/code leases. Repetition is not proof of coverage. |
| Workflow reproducibility wiring | The session workflow now creates a verified-empty runner `CARGO_HOME`, runs explicit `cargo fetch --locked`, then offline formatting, strict Clippy, and Loom tests. The in-flight workflow hashes the handoff model, runtime/transport/failure/value sources and both test files, runs them with `--nocapture`, and repeats the worker target 12 times with a five-baseline-frame coverage summary. | Workflow source inspection is not hosted success. Exact-head hosted runs and artifact review remain pending. Additional public-extension tests must not be hidden by the older five-frame summary. |

The Loom schedule counts observed locally are:

| Property | Schedules |
| --- | ---: |
| Competing session callers | 56 |
| Busy session caller retry | 4 |
| Early exclusive-lease release | 1 |
| Two live direct scopes | 258 |
| Two driver contenders | 56 |
| Direct and driver active modes | 90 |
| Gate-queued direct scope | 4 |
| Rejected entries preserve live guards | 1 |
| Caught unwind restores owner RAII | 1 |
| Defensive direct-count numeric boundary | 1 |
| **Total** | **472** |

The handoff anchor-state distribution is **Idle=3, Ready=6, Dispatched=2,
PublicationPending=1, WaitingInput=2, WaitingWake=1, Finished=2, Failed=2,
Cancelled=2**, totaling 21. Every operation's accepted and refused counts are
checked against an independent transition distribution, not merely summed from
actual outcomes.

### Corrected intermediate review finding

The earlier non-blocking, medium-severity concern about the old **two-Idle**
anchor assertion was raised against an incomplete test-authoring intermediate.
`PostCompleteIdle` is now an explicit anchor and changes the correct distribution
to **three Idle anchors and 21 total anchors**. Its dedicated reentry test proves
that each logical `Complete` disposes that activation's context while retaining
the process, increments activation/sequence correctly on reentry, and permits
final `finish_idle` cleanup. The corrected distribution and complete execution
matrix pass locally. **This was not a production defect and no production fix is
claimed.**

### Public identity/composition extension — independently reviewed

The public extension is not accepted as finished merely because new tests appear
in the working tree. At final inspection, the worker target contains locally
passing cases for:

- wrong compiled-index return with equal visible activation/sequence numbers;
- the same compiled path in two sessions, including completion, wake, and
  notifier locality;
- cancellation of retained `PublicationPending` output under both queue policies;
- one non-emitting old `Complete` result remaining foreign through five fresh
  checkpoints before admission, after fresh dispatch, after accepted/retained
  emission, after transport publication, and after logical settlement.

These cases passed the focused local worker target and final independent
adversarial review found no remaining fixture or production findings. Workflow
summaries now account for seven tests: the five baseline frames plus four added
identity/composition tests, including both queue policies where required.
Same-cancellation, replacement, and unrun-job ownership remain covered by the real
handoff identity matrices and existing direct tests; no result is inferred from
object cloning or reconstructed tickets.

### Focused local checks completed for this checkpoint

Under `nix develop`, using isolated target directories:

- the 10 Loom tests passed with the schedule reports above;
- all 7 `process::handoff_model` tests passed with the asserted history and
  operation distributions;
- all 8 `fbp_handoff_history` tests and all 7 currently present
  `fbp_inflight_worker_recovery` tests passed;
- all 61 native ownership/retirement tests listed above passed;
- strict `--no-deps -- -D warnings` Clippy passed for the isolated Loom verifier
  and for both graph integration targets.

These checks passed on the implemented worktree. The parent additionally passed
full default/no-default workspace tests, isolated Graph no-default tests and
explicit doctests, strict workspace Clippy, formatting, and all ten checks on
untracked-inclusive snapshot `/nix/store/v6d1vcjn5fnh5l00r5cnqb84mp00kfwg-source`.
An initial Nix compilation failed on host disk exhaustion; the same immutable
snapshot passed after approved temporary-target cleanup. Exact committed-head
Flake, both dedicated Loom jobs, native repeated workflows and their artifacts
remain **PENDING**. No acceptance, Q5 closure, H3b inference, or numerical progress
claim follows.

## Scope And Retained Boundaries

The [authoritative scorecard](fbp-issue-progress-estimates-20261113.md) and
[remaining-work map](fbp-remaining-work-20261115.md) remain the status authority;
this document does not modify them. Q7 is separately scoped verified at
`6f6e751b36f77273da8799900f665bc026447418`; its typed-failure protocol is a
constraint to preserve, not a finding reopened or advanced by this work.

C7 remains the user-approved **TrustedNativeScope** exclusion with no sandbox
credit. Nothing here isolates untrusted native code or proves freedom from
arbitrary plugin-, host-, or capability-owned lock cycles. No H1/H7 correlation,
isolation, or fairness claim, Q4 static-liveness claim, or performance/allocation
conclusion follows. Green benchmark execution is not performance acceptance.

## Original Requirements And Design

The sections below retain the original requirements and alternatives, updated to
the actual implementation filenames and corrected counts. The CURRENT checkpoint
above, not future-tense wording below, determines which local evidence exists.

## 1. Actual Contracts And The Missing Evidence

### 1.1 Kernel ownership and driver wiring

The current source establishes the following non-negotiable behavior:

| Boundary | Actual contract that must remain unchanged |
| --- | --- |
| Exclusive session ownership | `exclusive.rs` uses `Mutex<Option<T>>`, `try_lock`, owned `ExclusiveLease`, idempotent early `release`, and RAII restoration. All concurrent Kernel session callers fail fast with `Busy`/`WouldBlock`; same-kernel callback and worker reentry also fail fast. There is no session waiter or Condvar protocol to reintroduce. Existing wake/driver Condvars are a different, retained mechanism. |
| Producer ownership word | `DRIVER_OWNER == usize::MAX` denotes the whole-lifetime blocking driver. Zero denotes free; every other value counts admitted or gate-queued direct drains. `DriverScope` uses compare-exchange; `DirectDrainScope` uses bounded increment and RAII decrement. Multiple direct scopes can coexist, while the separate `producer_drain` mutex serializes actual producer callbacks. The word is a mode-exclusion protocol, not proof that only one direct scope exists. |
| Public error direction | A direct drain during a driver returns `AlreadyRunning`, including `drain(0)`. Driver admission during a direct drain returns `DrainInProgress`. A rejected entry must not construct a guard whose `Drop` releases the existing owner's authority. |
| `run_until` admission order | Started check; same-thread drain and session reentry checks; `DriverScope`; `DriverWakeScope`; fail-fast session preflight; existing terminal result; retirement check; session-notifier attachment; producer wake registration under the whole-drain gate. The second driver or direct-drain conflict is rejected before mutating the proposed wake or calling `set_waker`. Reverse RAII declaration order clears `DriverWakeScope` before releasing `DriverScope`. |
| Whole drain and retirement | `drain_owned` holds the existing whole-drain gate across the admitted poll/admission sequence. External publication waits for that whole gate, not merely one poll or explicit session work. Same-drain publication fails; self-retirement rejects the late poll event and defers destruction. No owner-word model may weaken this separate retirement gate. |
| Typed failures | Early driver errors, callback panics, and handoff outcomes retain the current Q7 `Failure` causes, context, cleanup order, and known leases. No String compatibility layer or error flattening is permitted. |

The original three `ExclusiveSlot` Loom properties did not exercise
`DriverScope`, `DirectDrainScope`, their shared atomic word, or the public
`Kernel::drain`/`run_until` wiring. The seven additional properties recorded in
CURRENT close only that primitive-evidence gap. Even together, the models do not
establish guard order, whole-drain retirement, wake behavior, or arbitrary
concurrency correctness; the native wiring tests remain necessary.

### 1.2 Process handoff and real worker recovery

`ProcessHandoff` is an owned, `&mut` control protocol, not a lock-free or
internally synchronized scheduler. `ActivationJob` moves the process, context,
guards, and lease to a runner; `CompletedStep` moves them back with exactly one
`Result<Step, Failure>`. Jobs and completions are intentionally not Clone.
Queue selection, readiness, routing, retained fanout, and publication belong to
`NetworkSession`, not to the handoff primitive.

The valid non-cleanup-panic result mapping in `process.rs` is:

| Result | Handoff state | Required continuation |
| --- | --- | --- |
| `Emit` | `PublicationPending` | Only a committed publication returns to `Ready`; publication failure is terminal. A full queue retries the retained session transaction without rerunning the step. |
| `Yield` | `Ready` | Same activation, next dispatch sequence. |
| `WaitInput` | `WaitingInput` | Exact additional reservation through `resume_input`; returned owned input on rejected resume. |
| `WaitInputs` | `WaitingInput` | Same handoff state as `WaitInput`, while the session separately retains the group policy and permanent polling obligation. |
| `WaitWake` | `WaitingWake` | Only wake authority calls `resume_wake`; readiness-only notification must not resume it. |
| `Complete` | `Idle` | Logical activation ends; a later admission is a new activation and the process remains available. |
| `Finished` | `Finished` | Physical process ends; no later admission or dispatch. |
| `Err(Failure)` | `Failed` | Preserve the original typed cause and cleanup; no retry of the failed step. |

Cleanup panics can turn an otherwise successful result into `Failed`; a base
transition model must use non-panicking owners and test that Q7 cleanup case
separately rather than hiding it in the table's normal edges.

Cancellation revokes the current handoff before deferred destruction and returns
the outstanding ticket as runner inventory. It cannot preempt a worker or undo
its effects. On the **same cancelled owner**, a late return is
`StaleCompletion`. `RestartAffectedProcess` installs a **new owner identity** at
the same compiled index, preserving activation/sequence accounting but installing
a fresh `WakeSlot`; an old completion offered through `complete_job` is then
`ForeignCompletion`. Old output cannot publish into the replacement, and the old
job's process/context/code lease survives until that returned object is dropped.

### 1.3 Wake authority and its actual boundary

`WakeHandle::wake()` sets `WakeSlot.pending`, inserts the compiled target, and
advances the aggregate listener epoch under the `WakeSignal` mutex.
`ProcessNotifier::notify()` performs the same targeted scheduler invalidation
without setting `pending`; `SessionNotifier::notify()` advances the global epoch
for a full scan. `retire_target` revokes the exact old slot and removes its queued
target under that same lock. Targets and pending bits coalesce; they are not an
event queue promising one execution per call.

The existing actual-source wake Loom verifier instruments `wake.rs`, not the whole
graph. Its `WakeListener::wait_timeout_while` omission remains covered only by
native tests. The new composition work must preserve all five wake properties,
the 13,440 serialized operation histories, and the native listener/retirement
tests rather than replacing them with a handoff model.

## 2. Existing Evidence Inventory — No Duplicate Credit

This inventory describes the original baseline obligations and historical
evidence, not new passes on the source-inspection head. Overlapping workspace
groups, focused test runs, and repeated workflows are not additive unique-test
counts. CURRENT separately records the new local checkpoint.

| Existing layer | Actual coverage already claimed | What must not be claimed again as new work |
| --- | --- | --- |
| Original `verification/fbp-session-loom` | Three exact-source `ExclusiveSlot` properties: competing callers, busy/no-theft plus retry, and idempotent early release. The original helper had preemption two, at most 20,000 permutations, a 20-second duration cap, and no per-property schedule report. | It did not cover the producer owner word, direct/driver union, public driver guard order, handoff, or graph scheduling. CURRENT records the replacement helper, seven added properties, and new counts without relabeling the original three as new. |
| `verification/fbp-wake-loom` | Five properties over included production `wake.rs`: old notify/retirement, fresh same-index slot, notification/drains, external-plugin-mutex composition, and terminal invalidation. At most three threads, preemption two, and 20,000 permutations per property; each property logs a count and requires more than one completed schedule. | These are bounded primitive schedules, not full graph instrumentation, listener deadline verification, arbitrary plugin lock-freedom, or handoff coverage. |
| `runtime::wake::tests` | Exactly 13,440 legal serialized histories with state checked after every event; 128 native targeted-notify/retire races; 24 races for each of four listener notification modes. | The serialized model is not Loom. The native races do not provide systematic atomic schedule enumeration. |
| `fbp_notifier_retirement_interleavings` | 96 total persistent-session rounds split across before, after, and racing schedules; stale notifier/wake rejection and fresh-only `WaitWake` resumption. The workflow repeats the target eight times. | Do not describe this as 96 rounds per schedule or as instruction-level exhaustive verification. |
| `fbp_plugin_lock_order` | Twelve deterministic challenges per callback kind per run for readiness, dispatcher, and recovery factory; eight workflow repeats produce 96 per kind and 288 total. | These are three trusted-native lock-order witnesses, not universal arbitrary-plugin deadlock freedom or C7 isolation. |
| Original `fbp_inflight_worker_recovery` | 48 total rounds split equally among replacement-first, old-return-first, and fresh-dispatched orders; twelve workflow repeats historically produced 576 rounds. The old worker returns `Emit`; its late completion is foreign after replacement, its output is rejected, and old/new effects each occur once per round. | Do not fold the new public-history/model cases into this historical count or call deterministic repetition exhaustive verification. |
| Kernel driver/session tests | `rejected_second_driver_keeps_first_lease_active`, `competing_drivers_reject_before_wake_registration_and_release_on_exit`, `live_driver_exclusively_owns_polls_and_releases_manual_drain_after_exit`, `in_flight_direct_drain_denies_driver_without_registering_wake`, and the native session busy/retry tests anchor both error directions and wake non-mutation. | Repeating those two-party cases is not a new multi-direct union proof or a systematic atomic interleaving model. The new gate-queued witness is counted separately. |
| Whole-drain retirement | `generation_retirement::blocked_poll_finishes_its_drain_before_publication_retires_the_source`, host retirement/drop tests, and `wake_host` composition retain whole-drain and old-session behavior. | The owner-word proposal cannot take credit for, or replace, these integration tests. |
| Handoff, timeout, and Q7 suites | `fbp_owned_steps`, `fbp_failure_protocol`, `fbp_deferred_timeout`, `fbp_lifecycle_c5_cooperative_20261116`, `fbp_resources`, and stream/readiness suites already cover many individual transitions, foreign/stale returns, publication retry, cleanup order, guards, and typed leases. | The new value is systematic transition-order composition and missing concurrent compositions, not another count of the same examples or re-acceptance of Q7/C5. |
| Native driver wakes | `native_sessions::blocking_driver_observes_native_process_wakes_and_external_shutdown`, deferred-deadline wake, terminal-driver tests, and `wake_host` retirement/publication tests cover their named host scenarios. | They do not by themselves account for every setup/drain/sleep/recovery frame or prove full driver/handoff/wake composition. |

## 3. Alternatives And Selected Design

| Alternative | Benefit | Reason for decision |
| --- | --- | --- |
| Add more native barrier repetitions only | Exercises real code without extraction | Useful but samples an unknown subset of atomic schedules; retain as a complement, not the systematic addition. |
| Instrument the entire graph and Kernel with Loom | Superficially broad coverage | Rejected: production-wide `cfg(loom)` does not instrument arbitrary dependencies or plugin code, owned runtime/resource behavior is not reduced to atomics, and the listener API remains unsupported. It would add substantial complexity without a sound whole-graph claim. |
| Copy the owner word or handoff into a standalone mock | Easy state-space exploration | Rejected: a copied protocol can diverge from production and cannot validate real owned jobs, failures, leases, or public wiring. |
| Extract `ProcessHandoff` into a synthetic lock-based protocol | Makes threaded Loom construction look simple | Rejected: its actual authority is safe `&mut` ownership, not a shared lock. Adding a lock would verify a different design and could hide object-lifetime and public-session composition defects. |
| **Extract only the producer ownership control primitive for exact-source Loom; enumerate operation histories against the real public handoff; add native public API composition schedules** | Each method matches the actual abstraction and preserves existing contracts | **Select.** The claims remain explicitly bounded and complementary. |

The implementation checkpoint extracts only `DriverScope`, `DirectDrainScope`,
the owner sentinel, and their atomic/RAII method bodies into private
`packages/phenix-kernel/src/producer_ownership.rs`. Private
`DriverAdmissionError` is mapped by actual `lib.rs::enter_driver` to the existing
public `DriverError`; there is no test-only replacement error authority. The
exact-source claim applies to the atomic and RAII method bodies, not to
`Kernel::run_until`, producer inventory, retirement, wake registration, or the
Kernel as a whole.

`ProcessHandoff` remains in `process.rs`. Its bounded checker is the test-only
`process/handoff_model.rs` module and instantiates the real type with real
jobs/completions, `Value`, `Failure`, guards, and leases. Public ownership-return
and transport complements live in `tests/fbp_handoff_history.rs`; real worker and
session recovery lives in `tests/fbp_inflight_worker_recovery.rs`. Same-index
recovery and scheduler-owned side effects use public `NetworkSession` APIs rather
than synthetic public access to private replacement hooks.

## 4. Verification Architecture And Independent Oracles

### Lane A: exact-source bounded Loom for producer ownership

Compile the new private module with only Loom's `AtomicUsize` substituted under
`cfg(loom)`, mirroring the existing `exclusive.rs` approach. Test the real
compare-exchange, fetch-update, fetch-subtract, guard construction, and drop
bodies. No production mutex, Condvar, waiter, test hook, unsafe block, deprecated
alias, or public compatibility shim is added.

The independent oracle is a three-state abstract union:

```text
Free
Direct { admitted_or_queued: 1..=DRIVER_OWNER-1 }
Driver
```

It must not inspect the production atomic to decide the expected state. After
every frame, assert entry result, actual word, live guard census, and the union
invariant. A failed entry has no guard and no release effect. Multiple direct
guards are legal; `Driver` is exclusive with every direct count and every other
driver. Only the final direct release returns that union to `Free`.

### Lane B: deterministic operation permutations on the real public handoff

Use a separate, small reference interpreter for legal host operations and
expected states. Generate bounded histories from the reference, then replay each
history against real `ProcessHandoff`/`NetworkSession` objects. After every
operation compare state, ticket activation/sequence, script position, returned
object kind, effect counts, and expected failure class. Expectations come from an
independently authored interpretation of the public contract, never from calling
production transition code or from actual results when selecting histories.

This is exhaustive only over the explicitly selected finite operation alphabet,
depth, scripted outcomes, and precedence constraints. It is not Loom, not a Rust
memory-model exploration, and not a model of transport/readiness internals.

### Lane C: native public API schedules

Use real threads only where the API transfers ownership safely: a worker runs an
`ActivationJob` while the main thread owns and mutates the session/handoff. Do
not wrap `ProcessHandoff` in a new mutex or simulate concurrent `&mut` calls.
Barriers and bounded channels establish schedule frames; timeouts are failure
watchdogs, not ordering mechanisms or liveness proof. Assertions cover public
results, output provenance, wake authority, original object return, effects, and
leases.

## 5. Falsifiable Coverage Matrix

This remains the requirement map. CURRENT records local evidence for the core;
it does not mark every row complete or discharge the pending public extension.
A row passes only when its named oracle and required native complement both pass
where applicable and final review accepts their scope.

### 5.1 Ownership, driver, and retirement

| ID | Property and bounded evidence | Required native/public corroboration and failure condition |
| --- | --- | --- |
| O1 | Driver versus direct contenders in exact-source Loom: no direct entry while the driver lives; rejected entries do not alter `DRIVER_OWNER`; direct and later driver entry succeed only after release. | Real `run_until`/`drain` calls retain `AlreadyRunning`, zero-budget rejection, no competing-wake mutation, no `set_waker`, and post-exit reuse. Any poll by a competing direct call fails the test. |
| O2 | Two simultaneous direct scopes plus a driver contender: both direct admissions may coexist, the driver sees `DrainInProgress`, and only the last direct release frees the union. | Real direct drains still serialize on `producer_drain`; a non-`Sync` producer and its retained event are never concurrently polled or duplicated. Any driver wake mutation before admission fails. |
| O3 | Bounded union oracle over enter/drop operations, including retry, rejected-entry, numeric-boundary, unwind, and gate-queued frames. The reference checks every intermediate union, not just the final word. | The actual Kernel wiring preserves the exact `DriverError` direction and RAII cleanup after success, typed error, callback panic, and unwind. The real gate-queued test must show 2 -> 1 -> 0 without a free window or wake mutation. |
| O4 | Exact-source model of a rejected second `DriverScope`: repeated rejection cannot release the first guard; drop/reacquire publishes the free state. | Preserve same-thread `ReentrantDrain`/`ReentrantSession` precedence over ownership admission and actual `DriverWakeScope`-before-`DriverScope` teardown. Existing unit tests remain mandatory. |
| O5 | Existing three `ExclusiveSlot` properties remain unchanged and run with the new owner properties; schedule accounting is added without substituting a waiter design. | Native `with_session`, `run_steps`, and `inject` still fail fast under contention, return original injected values, and succeed on retry. Any new session Condvar/waiter or blocking session acquisition is rejection grounds. |
| O6 | No new model claim for whole-drain retirement. | Re-run blocked-poll publication, host self-retirement, external publish/drop driver wake, and old explicit-session tests. An owner-word proof cannot compensate for a shortened drain gate or changed retirement order. |
| O7 | No new model claim for `run_until` source order merely because its scopes were extracted. | Actual-library tests prove admission before wake mutation, terminal/retirement precedence, registration errors, guard release, and subsequent valid reuse. Omitting `lib.rs` from triggering or execution fails acceptance. |

### 5.2 Every step result and handoff edge

| ID | Real transition exercised by bounded histories | Independent assertions and native complement |
| --- | --- | --- |
| H1 `Emit` | Dispatch returns one emission; retained publication is retried; separate histories commit and fail publication. | `PublicationPending` blocks reexecution/admission. Full retry leaves handoff, script position, and effect count unchanged; commit returns `Ready`; failure returns `Failed`. Output and typed leases remain owned exactly once. |
| H2 `Yield` | Complete with `Yield`, dispatch again, then continue the same script. | `Ready`, unchanged activation, increased sequence on the next job, no duplicate or skipped process effect. |
| H3 `WaitInput` | Complete with exact wait, reject an invalid/duplicate resume, then resume with disjoint owned input and dispatch. | `WaitingInput` cannot dispatch; rejected resume returns its `ProcessContext` unchanged; valid resume returns `Ready` without replacing prior reservation. Actual session also preserves exact-closure behavior and permanent staged polling. |
| H4 `WaitInputs` | Exercise at least one built-in group and one Custom group through the real result, then resume and continue. | It must not be collapsed into `WaitInput` at the session layer: `waiting_policy` and `always_poll` remain correct, and group EOS/custom semantics are unchanged. The handoff-only model claims only `WaitingInput`/`Ready`. |
| H5 `WaitWake` | Complete with `WaitWake`; attempt non-wake scheduler invalidation; resume only with wake authority; continue. | `ProcessNotifier` causes zero continuation steps, while a live `WakeHandle` permits exactly one resume. A stale/foreign wake cannot resume this or a replacement activation. |
| H6 `Complete` / post-Complete idle | Finish a logical activation, admit another, and compare tickets. Include `PostCompleteIdle` as a separate anchor. | State is `Idle`, each context cleanup occurs once, process state persists as contracted, and the next admission increases activation and sequence rather than reusing the old ticket. Final idle finish remains available. |
| H7 `Finished` | Finish the physical process and attempt admission, dispatch, resume, and publication. | Remain `Finished` unless explicit cancellation is the operation under test; no further process call, all owned cleanup attempted once, and invalid operations return their existing typed errors without resurrection. |
| H8 process failure | Complete with a `Failure` wrapping an original non-Clone concrete cause; include one separate cleanup-secondary case and a caught step/guard failure in native coverage. | State is `Failed`, no retry occurs, original cause identity/context and ordered Q7 cleanup survive, and known code/input/guard leases live through destruction. No String-only oracle. |
| H9 rejection and redispatch | Return an unexecuted job through `reject`, then dispatch it again; also attempt foreign and post-cancellation rejection. | Valid rejection restores `Ready` and the same activation, allocates a fresh sequence next time, and has not executed process effects. Invalid rejection returns the original job intact and cannot clear the live outstanding ticket. |
| H10 stale completion | Cancel a dispatched same-owner handoff and offer its real completion more than once. | Every late admission returns the completion intact with `StaleCompletion`; no state/publication mutation occurs. Effects that already ran stay exactly once and the code lease survives until final return drop. |
| H11 foreign completion | Offer owner B's real completion/rejected job to owner A, then return it to B; include a wrong session and a wrong compiled index. | A remains unchanged and returns `ForeignCompletion` with the original object; B can still settle it exactly once. A foreign return cannot clear A's deadline or outstanding inventory. |
| H12 cancellation states | Include cancellation from every reachable reference state, with and without an outstanding job, and cancellation during retained publication/waits. | Exact ticket inventory and terminal cancellation state are visible before destruction; no new admission is accepted. Session-owned prepared packets are disposed by the session, not falsely attributed to the handoff. |
| H13 same-index replacement | Through `RestartAffectedProcess`, replace each applicable old continuation at the same compiled index; separately keep its original job running for the late-result matrix. | New owner and `WakeSlot`, old completion `ForeignCompletion`, old wake/notifier false, no inherited pending wake or old output, fresh progress only after its own admission/wake, and old/new effects exactly once. |
| H14 dispatch return composition | Compose `Deferred`, `Completed`, `Rejected`, dispatcher panic, valid late return, and invalid late return with cancellation/recovery. | Expected cancelled/terminal returns are disposed without becoming a new dispatcher protocol failure; unexpected invalid returns preserve the returned object and existing session failure semantics. No second scheduler is introduced. |

### 5.3 Required native concurrent schedule frames

| Schedule | Existing coverage retained | Public composition requirement |
| --- | --- | --- |
| Replacement settles before old worker returns | Existing old-`Emit` case | The existing cases prove the three baseline orders. The real non-emitting `Complete` case separately exercises after-fresh-settlement foreign return under both queue policies. |
| Old worker returns before replacement starts | Existing old-`Emit` case | The real non-emitting `Complete` case separately exercises before-fresh-admission foreign return under both queue policies. |
| Fresh job dispatched before old completion returns | Existing old-`Emit` case | The real non-emitting `Complete` case separately exercises fresh-dispatched, retained-emission and published-but-unsettled foreign returns under both queue policies. Other late `Step` kinds are covered by the real private handoff matrices; no speculative full public cross-product is required because owner/ticket authentication precedes extraction of their result payload. |
| Same owner, no replacement | Existing handoff/deferred tests supply examples | Systematically pair cancellation with each late result and both return APIs where applicable; all are stale on that owner and returned intact. |
| Two owners/sessions | Existing direct foreign-completion examples | Actual worker return crossing a same-path/same-index session boundary verifies only the originating owner can settle it. The locally visible case remains pending final review. |
| Publication backpressure | Existing capacity-one and map retry tests | Compose retained `Emit` with cancellation/replacement and, separately, a dispatched original job with cancellation/replacement. Neither publication retry nor a late return may increment process effects or create a second emission. Both queue policies are required for retained-old-output disposal. |
| Wake identity | Existing same-index old/fresh notifier tests | Foreign-session same-path wake and late-result composition. A foreign wake may affect only its own live session; an old revoked wake affects neither owner. |
| Five public fresh boundaries | Existing leased `E<P<R<F` and `E<R<P<F` frames | Keep one real non-emitting old result foreign before fresh admission, after fresh dispatch, after retained emission, after transport publication, and after logical completion, under both queue policies. The locally visible `Complete` case remains pending final review; other outcomes are not inferred from it. |

The three baseline orders retain their old-`Emit` native evidence. All eight
outcomes have real private accepted/rejected/foreign/stale completion matrices.
Public native composition additionally uses a real old `Complete` through the five
meaningful fresh authority boundaries under both queue policies; this is not a
claim that every other outcome needs a redundant public cross-product, since
foreign owner/ticket authentication precedes extraction of that result payload.
Exact case counts are emitted by the test and reconciled to this matrix rather
than summed with historical rounds.

## 6. Wake Frames: What “Not Lost” May Mean

A blanket axiom that every notification is delivered once is false for this
implementation. Accepted targeted calls coalesce in a set/bit, a drain may consume
them, and retirement/terminal invalidation intentionally revokes them. The
composition invariant is instead:

> Within one owner lifetime, every accepted notification must be observed by a
> scheduler/listener drain or remain queued/pending for a later drive, unless an
> explicit retirement or terminal transition revokes that owner. No accepted old
> notification may become authority for a replacement or another session.

The current code leads to the following frame-specific obligations:

| Frame | Source-derived analysis | Required test/oracle |
| --- | --- | --- |
| Outer Kernel wake before its session notifier is attached | `phenix_kernel::WakeHandle::wake` advances its own epoch/Condvar, but cannot forward to a session notifier that is not yet installed. This must not be hidden by claiming the session listener saw it. | Prove the actual fallbacks: sticky shutdown is checked after registration, retirement is rechecked, and the initial producer poll/registration contract discovers ready input. A pre-registration ordinary wake alone is not execution authority. |
| After notifier attachment and during producer registration | The session listener was created in preflight, and the native notifier is attached before `set_waker`; later notifications advance its epoch. Registration and retirement errors retain precedence. | Existing registration/retirement tests plus a real transition from registration into polling and exit. No notification-dependent test may rely on an arbitrary sleep winner. |
| During drain, process dispatch, guard, or recovery frames | Wake methods do not run under the session lease's inventory mutex. A target inserted after `take_scheduler_wakes` remains for the next refresh; aggregate listener epoch remains observable. Plugin calls may still block by their own authority. | Native worker/notification schedule plus retained actual-wake Loom. Account for each accepted wake as consumed, retained, or revoked; never require one process step per coalesced call. |
| After the last active/retirement check and before idle wait | The already-registered epoch-latched listener closes the check-to-wait gap. Sticky shutdown and retirement are also checked at their existing boundaries. | Native idle-wait wake and retirement/shutdown tests remain required because this Condvar path is outside the wake Loom model. A missed epoch with no sticky-state explanation fails. |
| Cancellation/replacement at the same index | `retire_target` deliberately removes old authority before the fresh slot is installed. Disappearance of the old target is intended revocation, not a lost live wake. | Old wake/notifier false, fresh continuation cannot run without fresh wake, and late old job results remain foreign. |
| Foreign session or finished/terminal owner | Signals and slots have session/registration identity; terminal invalidation rejects later notifications and wakes listeners. | Same path and index in another session receive no authority; native terminal/listener tests and existing invalidation Loom property remain green. |

These tests must not introduce a production pause hook inside `wake.rs` or
`run_until`. Where public APIs cannot force a midpoint deterministically, the
acceptance claim must name the untested midpoint rather than converting a finite
witness into a universal no-lost-wake proof.

## 7. Bounds, Independence, And Schedule Accounting

### Exact-source Loom

- Keep Loom pinned to `=0.7.2` in the isolated verifier with its own lockfile.
  The implemented helper uses preemption bound two, at most 1,000 branches and
  20,000 permutations per property, checkpoint interval 1, and no duration cap.
  Maximum thread counts are per property and currently at most three.
- Count completed schedules with an ordinary counter outside the Loom model and
  print one line per named property. Concurrent properties require more than one
  completed schedule; a genuinely single-thread property must run exactly once.
- Loom 0.7.2 does not return an explicit completion event. Because it checks the
  cap before iterations, the helper conservatively treats
  `schedules >= MAX_PERMUTATIONS - 1` as inconclusive. All current properties
  report `inconclusive=false`; no time- or permutation-truncated result is
  promoted to systematic bounded completion.
- Schedule counts are evidence for this pinned source/toolchain, not universal
  constants across arbitrary Loom versions or a claim of exhaustive concurrency.

### Sequential union and handoff histories

- The implemented handoff alphabet has eight operations and 73 suffixes per
  anchor: empty, eight singletons, and all 64 ordered pairs. There is no RNG or
  sleep. The corrected anchor count is 21, producing exactly 1,533 histories and
  2,856 attempts.
- The independent transition distribution expects 611 accepted and 2,245 refused
  attempts. Each operation has 357 attempts; cancellation is accepted in all 357,
  while all other operations have independently derived acceptance/refusal counts.
  Tests compare every operation and the aggregate against that distribution.
- The independent reviewer must rederive the anchor-state and transition counts,
  including `PostCompleteIdle`, rather than trusting the implementation's totals.
  Missing states or outcome cells fail even when all executed histories pass.
- Actual state is checked after every frame, including rejected operations and
  returned owned objects. Final-state-only comparison is rejection grounds.

### Native schedules

- Report exact executed rounds per named frame, not elapsed repetitions as a
  proxy for interleaving coverage. Preserve the original 8- and 12-repeat workflow
  counts separately from new cases.
- The current in-flight workflow's five-frame summary means 48 legacy rounds plus
  two publication-order frames at one round each. Its 12 additional executions
  are deterministic witnesses, not exhaustive interleaving coverage. New public
  identity cases require their own count reconciliation before final acceptance.
- Native passing schedules do not imply unbounded coverage, fairness, deadlock
  freedom, or a complete happens-before proof. Conversely, Loom and sequential
  counts do not substitute for actual OS-thread and real-owner execution.

## 8. Required Workflow And Nix Wiring

### Session ownership workflow

`.github/workflows/fbp-q5-session-ownership.yml` retains
`packages/phenix-kernel/src/lib.rs` and includes the actual ownership, host, and
native witness paths:

```text
packages/phenix-kernel/src/exclusive.rs
packages/phenix-kernel/src/lib.rs
packages/phenix-kernel/src/producer_ownership.rs
packages/phenix-kernel/src/host.rs
packages/phenix-kernel/tests/native_sessions.rs
packages/phenix-kernel/tests/generation_retirement.rs
packages/phenix-kernel/tests/wake_host.rs
verification/fbp-session-loom/**
```

Its two jobs are separate. The Loom job sets `RUSTFLAGS="--cfg loom"`, creates a
previously nonexistent runner-local `CARGO_HOME`, records that fact and source
hashes, performs explicit `cargo fetch --locked`, then runs offline formatting,
strict Clippy, and model tests. The native job unsets `RUSTFLAGS`, hashes and
compiles the real Kernel sources, strictly lints graph/Kernel libraries, and runs
the actual 61 ownership/gate/host/retirement tests. `lib.rs` remains required
after extraction because it owns real guard order and error mapping.

### Handoff and worker workflow

`.github/workflows/fbp-q5-inflight-worker-recovery.yml` triggers on and hashes the
actual files:

```text
packages/phenix-graphs/src/process.rs
packages/phenix-graphs/src/process/handoff_model.rs
packages/phenix-graphs/src/runtime/execution.rs
packages/phenix-graphs/src/runtime/recovery.rs
packages/phenix-graphs/src/runtime/scheduling.rs
packages/phenix-graphs/src/runtime/wake.rs
packages/phenix-graphs/src/connection.rs
packages/phenix-graphs/src/failure.rs
packages/phenix-graphs/src/value.rs
packages/phenix-graphs/tests/fbp_handoff_history.rs
packages/phenix-graphs/tests/fbp_inflight_worker_recovery.rs
```

It runs `process::handoff_model`, `fbp_handoff_history`, and the worker target
once with `--nocapture`, then repeats the worker target 12 additional times with
`--nocapture --test-threads=1`. Logs and artifacts must retain per-model,
per-history, and per-frame counts. The existing five-frame summary is limited to
48 legacy rounds plus two publication-order rounds; public identity/composition
extensions must be explicitly included in the final summary rather than silently
assumed covered by that wording.

`nix develop` supplies the toolchain for the standalone verifier. It does not make
the verifier a workspace member and does not instrument the full graph or all
atomics. Ordinary `nix flake check` uses normal `std::sync` code and does not run
the isolated Loom models merely because Loom was run elsewhere under Nix. The
actual-wake Loom workflow remains a separate mandatory gate with its own exact
source and lock hashes.

## 9. Phase Gates And Acceptance

### Phase 2: independent critique and final review requirements

The original requirement for independent pre-code critique is not retroactively
claimed as evidence. With implementation now present, a final independent
reviewer must accept the actual source and remaining extension before acceptance.
The review must specifically:

1. validate the three-state ownership oracle, exact history counts, and the claim
   that direct-scope multiplicity is not confused with producer-callback mutual
   exclusion;
2. verify that extraction and the actual `DriverAdmissionError` mapping do not
   turn “exact source” into an overstated whole-Kernel claim;
3. rederive the 21-anchor distribution, including `PostCompleteIdle`, all 1,533
   histories and 2,856 attempts, the eight outcome matrices, and the corrected
   intermediate finding without inventing a production defect;
4. check every H-row and late-result cell, stale versus foreign identity, original
   job return/effect/lease assertions, and Q7 preservation;
5. challenge the wake-frame invariant, especially pre-notifier outer wakes,
   coalescing, retirement revocation, and the native-only Condvar boundary;
6. review the public wrong-index/two-session, same-cancellation/replacement/
   unrun-job, both-queue publication-pending, and non-Emit five-checkpoint cases
   without treating local source presence or repetition as final acceptance;
7. confirm that native schedules use safe public ownership transfer rather than a
   new handoff lock or unsafe sharing;
8. audit all workflow triggers and executed commands, including `lib.rs`, the new
   private module, `fbp_handoff_history.rs`, and both model/worker paths, with
   native/Loom `RUSTFLAGS` separated and coverage summaries synchronized.

No acceptance occurs with unresolved blocking findings. Independent final reviews
covering the public extension, whole scope, model counts, workflows and actual
fixtures completed with no remaining findings. Earlier evidence-matrix and summary
mismatches were reconciled without weakening the implemented coverage. Exact
committed-head hosted acceptance remains pending and is not inferred from these
local passes.

### Phase 3: minimal implementation constraints

- Move only the narrow producer ownership control protocol; preserve public API,
  error precedence, RAII/drop order, whole-drain retirement, and fail-fast session
  behavior. No unsafe code, deprecation layer, new scheduler, production test
  hook, or session Condvar/waiter.
- Keep the bounded owner models and union oracle, real handoff permutation
  checker in `process/handoff_model.rs`, public history tests in
  `fbp_handoff_history.rs`, and real worker schedules in
  `fbp_inflight_worker_recovery.rs`. Extend existing tests only for a named
  missing cell, not to inflate duplicate counts.
- Keep real process/Value/Failure/guard/lease objects. Do not make jobs or
  completions Clone to simplify the model, and do not replace typed failures with
  strings.
- Preserve all existing wake models/tests and native recovery suites. A discovered
  production counterexample requires a separately reviewed runtime fix and new
  regressions; it must not be hidden by editing the reference to match behavior.

### Phase 4: falsifiable acceptance checklist

Acceptance remains pending until all of the following identify the same exact
committed assembled head:

1. Independent final review accepts the actual scope and public identity
   extension, with all counterexamples and coverage-count derivations recorded.
2. Focused owner/handoff/native tests pass with every O/H/schedule ID accounted
   for, exact deterministic history counts asserted, Loom schedule counts logged,
   and no truncated exploration presented as systematic completion.
3. Formatting, strict Clippy, default and no-default workspace/graph tests and
   doctests pass. The complete six-job Flake CI passes with every shard and the
   aggregate successful.
4. Session-ownership Loom plus its native job, actual-wake Loom, eight-repeat
   notifier/plugin/wake workflow, and twelve-repeat in-flight worker workflow all
   pass. All other existing exact-head workflows required by repository policy
   also remain green; their success supplies no performance inference.
5. Artifacts contain compiler/toolchain, exact commit, source and verifier-lock
   hashes, model bounds/counts, native case ledgers, and logs. Earlier-head passes
   are not relabeled.
6. Existing fail-fast/retry, whole-drain retirement, exclusive-driver guard order,
   terminal precedence, wake listener, Q7 cause/cleanup/lease, and real old-worker
   effect tests all remain green.
7. Public identity/composition cases and their actual counts are represented in
   the workflow summary and artifacts. The older five-baseline-frame wording is
   not used to conceal omitted new cases or to imply that repetition is proof.

Failure to trigger or execute any required workflow/test, missing schedule
counts, a missing matrix cell, or substitution of a copied model for actual source
blocks acceptance. Even after complete scoped acceptance, **Q5 remains OPEN
unless its authoritative finding receives separate explicit acceptance**. H3b is
not evaluated or closed by this Q5 checkpoint. No automatic closure, score
increase, C7 sandbox credit, H1/H7/Q4 conclusion, universal arbitrary-plugin
deadlock-freedom claim, or performance claim is part of this design.
