# Lifecycle Policy Acceptance Candidate

Date: 2026-11-16. Audited production baseline:
`0c0bbb1a57c9f8c0a98de2a0b8a1a0a26d70ef6e`.
Disposition: **C3 LeasedArchival and C5 Cooperative are scoped verified at
`89171a843`. C7 TrustedNativeScope is an
approved scope exclusion, not an implemented or verified sandbox.**

## Approval And Original Requirements

The owner explicitly approved these choices in the task that commissioned this
audit, on 2026-11-16:

| Finding | Original issue / acceptance dependency | Approved choice | What this record does not claim |
| --- | --- | --- | --- |
| C3 | Retired generations remain callable; choose archival, drain-only, or strict revocation across generations, compiled sessions, and jobs | **LeasedArchival**: retire host-owned producer polling; retained explicit generation/session/job capabilities continue under their leases | No strict old-generation revocation, drain-only admission rule, automatic migration, or forced code unloading |
| C5 | Deferred jobs can strand a session; approve supervision lifetime and verify detection/recovery | **Cooperative**: opt-in while the application drives or the native blocking driver is active; no autonomous service when abandoned; no callback/worker preemption | No always-on watchdog, real-time deadline guarantee, ownership reclamation, or rollback of effects |
| C7 | Native plugins share process privileges; implement isolation or approve exclusion | **TrustedNativeScope**: trusted native execution only; untrusted native isolation excluded | No process-isolation adapter or untrusted-code protection; never award 100% verified-sandbox credit for this exclusion |

Original references are [C3 alternatives](fbp-c3-c4-design-20261111.md#c3--retired-generations-can-still-execute),
[C5 design](fbp-c5-deferred-watchdog-design-20261112.md),
[C5 critique](fbp-c5-deferred-watchdog-critique-20261112.md),
the [reconciled lifecycle semantics](fbp-hardening-campaign-20261114.md#lifecycle-policy-decisions),
and the [remaining-work requirements](fbp-remaining-work-20261115.md#all-22-findings).
The older notes' pending-policy wording is historical relative to this explicit
approval, not evidence of missing strict revocation or autonomous supervision in
the newly approved scope. The approval changes the acceptance target, not runtime
behavior. It does not itself close a finding or supply test evidence.

Only this document and two uniquely named acceptance-test files belong to this
work. No existing production/runtime, ledger, or CI file was edited. Concurrent
changes to `flake.nix`, `fbp_authoring_equivalence.rs`, and
`restart_persistence.rs` were observed and left alone. Their changes/results are
not acceptance evidence here. No commit, push, ledger reclassification, or score
increase was made.

## C3 Contract Audit

Source references below are relative to `packages/phenix-kernel` unless stated
otherwise. The review includes both `KernelHost` retirement paths, manual drains,
the static blocking driver, explicit session entry, and executable descendants.

| Boundary | Actual behavior and evidence |
| --- | --- |
| External publication | `src/host.rs:274-329` authenticates host allocation and expected epoch before publication. It acquires the old whole-drain gate before inventory, commits the staged inventory, marks retirement and detaches producers, replaces the active pointer, releases locks, wakes drivers, then destroys producers while retaining the old generation lease. Failed/stale publication cannot replace the active pointer. |
| External host destruction | `src/host.rs:333-367` uses the same gate-before-inventory ordering, retires producers, releases locks, and notifies the driver before producer destruction. |
| Whole drain vs retirement | `src/lib.rs:1637-1759` holds the gate for the entire admitted drain. External publication cannot split its polls. An old session admission already accepted remains executable; a later drain cannot poll retired inventory. A polled event retained on Full/contention is not yet an accepted session packet: retiring its producer inventory may dispose that unadmitted event, not migrate it. |
| Self-destruction in producer code | The held-drain branch marks retirement without reacquiring its own gate. The enclosing drain/registration restores its checked-out slot and detaches producers before dropping them outside locks. A poll result returned after self-retirement is discarded. Admission that passed the retirement boundary can finish, including eager readiness callbacks. |
| Recursive publication / ownership | Same-drain publication returns `ReentrantDrain` rather than self-waiting. Direct drains and a live static driver cannot own polls simultaneously. Session contention stays fail-fast; producer admission retains one pending event rather than waiting for an explicit session lease. |
| Static driver | `src/lib.rs:1841-1952` observes retirement before new polling and before idle sleep; retirement wakes its existing notifier without shutting down the application handle or cancelling the session. A live driver returns `Retired` after cooperative boundaries. Already-running callbacks/drive operations are not preempted. Initial Completed/Failed/Cancelled results retain terminal precedence; registration rejection/panic retains its specific error. |
| Explicit generation calls | `src/host.rs:77-114` delegates `with_kernel`, `with_session`, `inject`, typed inject, and `run_steps` without a retirement revocation check. `src/lib.rs:1511-1582` retains startup, reentrancy, and exclusive-session checks. A retained old handle can admit **new** work to the old session, not just drain previously accepted work. It cannot thereby access the successor session. |
| Detached executable capabilities | `CompiledNetwork::start_with_options` clones the compiled network. Graph `src/runtime/mod.rs:369-372`, `src/network.rs:1171-1185`, and `src/process.rs:571-580` retain compiled/factory/process/session/job leases. A compiled clone obtained before or after retirement can start another explicit old-code session. Independent jobs can execute after both host and session drop. |

### Identity And Leases

These identities are distinct, not interchangeable generation counters:

- `Generation::epoch()` is a host-local publication sequence. A refresh with no
  plugin reload still creates a new compiled network/session and increments the
  host epoch; another host may have the same numeric epoch.
- Compiled bound input/output handles authenticate the compiled allocation token
  (`phenix-graphs/src/network.rs:1292-1306`). Cloning or attaching a lease preserves
  that token; a fresh compilation does not, even for the same plugin revision.
- `RequestId` has private session/generation/sequence fields. `check_request`
  checks the owning session before the session generation and live registry
  (`phenix-graphs/src/runtime/request.rs:249-256`). Equal request sequence numbers
  across old/new or same-compiled sessions do not grant cross-session authority.
  Released IDs remain stale. Session metadata's `generation` field is not the
  host's publication epoch; sessions start with their own generation value zero.
- `StepTicket` authenticates a private handoff owner plus exact generation,
  activation, and step. `ProcessHandoff::validate` rejects foreign owners and
  non-outstanding tickets. An archived live owner's return can still succeed;
  retirement alone does not invalidate it. Request restart creates a new owner,
  so old returns are foreign; cancellation without replacement makes them stale.
- Retention is intentionally unbounded in time if an application retains a
  capability. Leases prevent premature unload, not memory/resource retention by
  an uncooperative holder. Metadata inspection alone is not execution authority.
  Native code can also share explicitly captured external state; session identity
  is not a guarantee of isolation of arbitrary external effects.

No successor grants were widened, and old grants were not mutated or revoked.
Old capabilities continue with their original compiled declarations and resources.
The new fixture supplies the same fixed host grant set to both builds and an
explicit semantic claim for its bound-handle checks.

### Interleaving Argument

For external retirement, either a drain wins the gate and completes all its polls
before retirement, or retirement wins and the drain sees `retired` before checkout.
The flag cannot change externally while that drain owns the gate. Self-retirement
is the deliberate exception: the callback marks the flag in-place, then the
post-poll admission check rejects its late result and the next iteration stops.
Registration likewise stops before the next producer callback. Destructors run
after the gate and inventory locks are released, with the generation/code leased.
Explicit session leases are not acquired by publication and therefore cannot
make publication wait for a running explicit callback.

This is a source-ordering argument backed by real-thread tests, not an exhaustive
model of all Rust/plugin/external lock interleavings. It does not close the broader
H3b/Q5 verification findings or promise publication can preempt a blocked producer.

### C3 Test Inventory

| Existing target / test | Obligation |
| --- | --- |
| `tests/generation_retirement.rs::blocked_poll_finishes_its_drain_before_publication_retires_the_source` | Blocked first poll, all three admitted drain polls precede publication, later old drains do not poll, accepted packets run afterward |
| `tests/generations.rs::publication_pins_old_engines_but_stops_old_producers_and_batches_unload` | Old explicit inject/run through several reloads and unload; producer stop and exact old-code retention |
| `tests/generations.rs::publication_does_not_wait_for_old_explicit_wave_or_callback` | Publication independent of old explicit session/callback ownership |
| Other `generations` and `generation_retirement` tests | Stale/foreign candidates, unchanged active generation after failures, fresh publication epochs, abandoned candidate and producer-before-plugin destruction |
| `src/host.rs` unit tests | Self-drop in poll and all wake-registration outcomes; external publish/drop wakes live static driver; explicit old session still usable; same-drain publish rejected |
| `tests/wake_host.rs`, especially `wake_host_retirement_still_notifies_native_driver_and_keeps_explicit_old_session_usable` | Application wake hosting does not break static driver retirement or explicit archival; publication notification precedes stalled destructor |
| `tests/native_sessions.rs`, `tests/worker_guards.rs`, kernel unit tests | Fail-fast contention/reentry, RAII restoration, worker guard propagation, owned jobs/captures retain code after kernel drop |
| `tests/code_leases.rs`, graph `fbp_declaration_leases`, `fbp_owned_steps` | Detached compiled exports/factories/processes/sessions, application and plugin leases, escaped values, cancellation and job-drop lifetime |

**Specific gap filled:** existing tests covered the pieces but not a retained
request/job across real host replacement with same-compilation vs fresh-compilation
authentication. Added only
`packages/phenix-kernel/tests/lifecycle_c3_leased_archival_20261116.rs` (one test,
two cases: same plugin revision republished, and reloaded revision).
It checks bound handles resolve against old clones but reject the successor;
old/new request sequence collisions remain foreign; a wrong-session completion
returns the original ticket intact and still completes in its archived owner;
compiled clones taken before/after retirement start detached sessions; old explicit
new work still runs; and a final detached job alone pins old plugin/application
code through host/session destruction. It does not duplicate the already-covered
next-producer-after-retirement test.

## C5 Contract Audit

Source references here are relative to `packages/phenix-graphs` unless qualified.

| Obligation | Implementation and executed coverage |
| --- | --- |
| Opt-out | `SessionOptions::default().deferred_timeout == None`; no age entries inserted for this mode. `fbp_deferred_timeout::default_timeout_accepts_direct_job_and_preserves_payload_provenance` and `fbp_streams::deferred_job_supervision_invalidates_ticket_without_reclaiming_owned_process` show no expiry and successful ordinary return. |
| Passive observation / abandoned session | `overdue_jobs(now)` only observes; `state`, request status/work/failure/diagnostics do not supervise or execute readiness/steps. `fbp_request_diagnostics::observation_does_not_call_readiness_execute_steps_or_supervise_overdue_jobs` covers both queue policies and exact overdue ticket, leaving it pending until explicit supervision. No timer thread or lifetime service is installed. |
| Explicit handoff timing | Public `next_job` arms at return (`src/runtime/mod.rs:1404-1410`), including jobs never given to a dispatcher. Direct zero-timeout reject/complete/complete_job return tests preserve the rejected job/completion and fail before accepting publication. |
| Dispatcher handoff timing | `run_until_idle` uses unarmed `next_job_inner`, supervises existing siblings before/after dispatch, and arms this exact ticket only after `Deferred` returns (`:1864-1943`). Synchronous Completed/Rejected callbacks do not acquire their own timeout. The reconciled 2026-11-14 contract makes this distinction explicit; the earlier draft's generic physical-handoff wording is not used to time synchronous callbacks. |
| Exact outstanding tickets | The age map is per physical process, but expiry reads and rechecks its exact outstanding ticket. Successful validated complete/reject removes its age; foreign returns cannot clear it. Request termination removes affected entries; session stop clears all. `fbp_deferred_timeout` covers re-dispatch with fresh sequence and removal, registry reuse, and exact cancelled-job inventory. New multi-job test covers foreign rejection/completion with both live timers retained. |
| Deadline meaning | `now.saturating_duration_since(admitted) >= timeout`; equality expires, ZERO expires at the next supervision boundary, and MAX does not require overflowing an Instant addition internally. Completion must be accepted before the deadline, not merely executed by the worker before it. `next_deferred_timeout` returns the minimum remaining outstanding duration, saturated at zero (`:702-714`). |
| Recovery and late jobs | `supervise_deferred` fails the owning request via existing policy or fails the unscoped session. FailSession invalidates all active work; RestartAffectedProcess fails only the affected request and spawns fresh affected process state. Late returns cannot publish or replace fresh state, but external workers still own and may run their jobs/effects. `fbp_deferred_timeout`, `fbp_streams`, `fbp_inflight_worker_recovery`, and the new cross-product test cover these boundaries. |
| Both queue / failure policies | Existing passive diagnostics covers SharedFifo and BoundedLanes with FailSession. Existing timeout tests cover SharedFifo restart and BoundedLanes restart, with both queues also in late-return cleanup cases. The new 2x2 matrix directly exercises expiry, exact inventory, terminal vs local outcome, late return, unrelated request progress, and credit/process reuse under each queue/failure combination. |
| Return cleanup and siblings | Existing `sibling_expiry_during_dispatch_does_not_fail_replacement_request` covers Completed/Rejected/panic after a sibling deadline, queued and unqueued replacement work. `late_timeout_returns_contain_destructor_panics_and_keep_the_exact_job_lease` covers both queue policies, scoped/unscoped work, Completed/Rejected cleanup, destructor panic containment, and exact job lease order. |
| Native deadline wake | `phenix-kernel/src/lib.rs:1945-1950` bounds idle waits by the report's earliest deferred deadline (and the existing 60-second cap). `native_sessions::blocking_driver_wakes_for_deferred_job_deadline_without_worker_notification` exits Failed with timeout cause while the worker still holds the job, with no worker wake or shutdown needed. A blocked synchronous callback or contended exclusive session lease is not a service guarantee. |
| Exclusions / first terminal | WaitWake and retained publication backpressure have no external outstanding job and are not timed. Existing zero-timeout tests prove both. Explicit request cancellation serialized before supervision wins; timeout/failure followed by cancellation keeps the initiating terminal result/cause. Terminal cleanup/transition tests and the new cancellation-first test verify those rules. |

**Specific gaps filled:** added only
`packages/phenix-graphs/tests/fbp_lifecycle_c5_cooperative_20261116.rs`, four tests:

- `timeout_recovery_and_late_returns_cover_both_queue_and_failure_policies`:
  the direct 2x2 timeout/recovery matrix described above.
- `multiple_deadlines_survive_foreign_returns_and_clear_only_the_validated_ticket`:
  two outstanding tickets, foreign return preservation, earliest report bounded
  by instants around the first handoff, and the second deadline after the first
  completes. Timely rejection removes the remaining timer.
- `deferred_deadline_starts_after_synchronous_dispatch_returns_not_when_it_enters`:
  dispatcher takes longer than the configured timeout before returning Deferred;
  its own deadline starts afterward, not during the callback. Passive future-time
  inspection leaves the abandoned session waiting; explicit supervision invalidates
  the ticket, but the retained job can still perform its effect.
- `explicit_cancellation_before_supervision_wins_even_when_job_is_already_overdue`:
  ZERO-timeout job, both queue policies, cancellation first, no later timeout cause
  or resurrection by the late worker return.

The approval deliberately excludes a host that never drives/supervises again.
Neither a wake hint nor a passive status query is service. `with_session` itself
does not supervise an arbitrary user closure: its invoked session operations
determine whether a supervision boundary occurs. Application reactors must drive
or explicitly supervise while idle; a reactor waiting indefinitely only for worker
wakes does not gain the native driver's deadline-wait behavior automatically.

## C7 Trusted-Native Boundary

Typed imports, exact host compose/inspection grants, authenticated
request/capture/handoff ownership, resource capability passing, worker guards,
contained Rust unwinds, and code leases protect API authority/lifetime and reduce
accidental misuse by **cooperating trusted native code**. They do not constrain
native code's use of filesystem, network, environment, process memory, threads,
global state, or independently retained capabilities. Panic containment is not
crash isolation and does not catch aborts or make unsafe native code safe.

This applies to PluginInstance, preparation/activation, producers, factories,
processes, readiness policies, dispatchers, codecs, resource callbacks, and native
destructors. A trusted callback may block indefinitely; neither C3 nor C5 adds
preemption. Existing `prepare_inspection`, native grant tests, and compile-fail
doctests are capability-boundary evidence only, never hostile-plugin sandbox
tests. No untrusted native process adapter, serialized isolation protocol, OS
resource limit, crash containment, or secure external cancellation was implemented
or verified here. **Approved exclusion must never receive 100% verified-sandbox
credit**, nor be counted as implementation of the original untrusted-isolation
requirement.

## Executed Verification

All commands used the Nix development shell and locked offline Cargo dependencies.
The following are the final focused target sets (each name means `--test NAME`):

```bash
KERNEL_TESTS=(
  --test generations --test generation_retirement --test code_leases
  --test native_sessions --test lifecycle_c3_leased_archival_20261116
  --test worker_guards --test wake_host --test callback_errors
  --test prepare_inspection
)
GRAPH_TESTS=(
  --test fbp_deferred_timeout --test fbp_lifecycle_c5_cooperative_20261116
  --test fbp_request_diagnostics --test fbp_streams --test fbp_owned_steps
  --test fbp_request_session_isolation --test fbp_terminal_transitions
  --test fbp_terminal_cleanup --test fbp_declaration_leases
  --test fbp_inflight_worker_recovery
)
nix develop --command cargo test --locked --offline -p phenix-kernel --lib "${KERNEL_TESTS[@]}"
nix develop --command cargo test --locked --offline -p phenix-kernel --no-default-features --lib "${KERNEL_TESTS[@]}"
nix develop --command cargo test --locked --offline -p phenix-graph --lib "${GRAPH_TESTS[@]}"
nix develop --command cargo test --locked --offline -p phenix-graph --no-default-features --lib "${GRAPH_TESTS[@]}"
nix develop --command cargo test --locked --offline -p phenix-kernel --doc
nix develop --command cargo test --locked --offline -p phenix-graph --doc
nix develop --command cargo test --locked --offline -p phenix-graph --no-default-features --doc
```

| Run | Actual result |
| --- | --- |
| Kernel focused, default | 80 passed: 32 library, 48 integration; zero failed/ignored/filtered |
| Kernel focused, no-default | Same 80 passed; zero failed/ignored/filtered |
| Graph-only focused, default | 161 passed: 18 library, 143 integration; zero failed/ignored/filtered |
| Graph-only focused, no-default | Same 161 passed; zero failed/ignored/filtered |
| Kernel doctests | 9 passed, including 7 compile-fail |
| Graph-only doctests, default | 26 passed, including 17 compile-fail |
| Graph-only doctests, no-default | Same 26 passed, including 17 compile-fail |
| Clippy, new kernel acceptance target | Passed with `-D warnings` |
| Clippy, graph-only no-default new acceptance target | Passed with `-D warnings` |
| Formatting / whitespace | `rustfmt --check` passed for both new tests; `git diff --no-index --check /dev/null PATH` passed for all three owned files |

Clippy commands were `nix develop --command cargo clippy --locked --offline -p
phenix-kernel --test lifecycle_c3_leased_archival_20261116 -- -D warnings` and
`nix develop --command cargo clippy --locked --offline -p phenix-graph
--no-default-features --test fbp_lifecycle_c5_cooperative_20261116 -- -D warnings`.
The two new files were formatted with Nix `rustfmt --edition 2024`.

Feature verification used `cargo tree --locked --offline -p PACKAGE
--no-default-features --edges features` (depth 1 for graph, 2 for kernel), also
inside `nix develop`. Kernel has no own feature toggles and still enables graph
`default`/`derive` through its dependency. **Kernel/workspace no-default is not
derive-free graph coverage.** The separate graph-only invocation has only
`phenix-schema` at depth 1, with no derive dependency. Every selected test target
actually ran tests in both modes; no zero-test feature-gated target is counted.
Counts are executions of these targets, not numbers of distinct lifecycle
properties or whole-workspace acceptance.

Intermediate fixture errors were corrected only in the new C3 test: BuildOptions
is not Clone, and binding requires an explicit semantic contract claim, not merely
matching shape. Both final kernel target sets passed after those corrections.
Nix reported transient ignored evaluation-cache contention during parallel runs;
the final commands completed successfully. These are local `nix develop` test
runs, not sandboxed flake-check derivation builds or hosted workflow runs.

## Local Audit Disposition

The following candidate judgments record the local audit before the hosted
acceptance documented in the next section.

**C3 acceptance candidate:** the entire approved archival authority split has
source and executed evidence, including the newly filled integrated
request/clone/job-authentication gap. No production bug or remaining missing test
blocking this approved scope was identified. Strict revocation is neither
implemented nor silently substituted for archival.

**C5 acceptance candidate:** the approved opt-in cooperative contract has source
and executed evidence across the matrix above, including the four specific gaps
filled here. No production defect or remaining missing test blocking this approved
scope was identified. Autonomous service for an abandoned application and
synchronous callback/worker preemption remain deliberately absent, not verified
features.

**Remaining acceptance gate:** independent final source/evidence review found no
actionable defect and supports the entire approved C3/C5 scope. Full assembled
workspace/default/no-default, isolated graph no-default, formatting and strict
Clippy checks also pass. A new committed head containing these files must still
pass hosted CI. Local testing and review do not replace that gate. The
source-ordering review and selected real-thread tests are not exhaustive concurrent
verification; broader H3b/Q5 obligations remain open. The timing-specific new test
uses a 250 ms synchronous delay against a 200 ms timeout; like existing live-driver
and sibling-expiry tests, it assumes the test process is not indefinitely descheduled
at the handoff boundary and is not a real-time latency benchmark.

The original broader targets remain unavailable if requested: C3 strict revocation
would require a different capability policy, C5 autonomous supervision would require
an owned service lifetime, and C7 untrusted isolation still needs a real external
security boundary. Those are not blockers to the explicitly approved C3/C5 scope;
they must not disappear from comparisons with the original requirements. No whole
C7 sandbox closure or numerical closure credit is warranted. C6, A4, Q6, all
unrelated ledgers, and production acceptance remain outside this work.

## Exact-Head Acceptance

Implementation `89171a8437c6fc2aebcd9029eb119d8c7505d389` passed all fifteen
hosted workflows, including:

- [Full six-job Flake matrix 43804652273](https://github.com/matthis-k/phenix-ai/actions/runs/43804652273).
- [Actual-source wake Loom 43804651650](https://github.com/matthis-k/phenix-ai/actions/runs/43804651650).
- [Session ownership Loom 43804651581](https://github.com/matthis-k/phenix-ai/actions/runs/43804651581).
- [Eight-repeat native notifier/plugin-lock gate 43804995462](https://github.com/matthis-k/phenix-ai/actions/runs/43804995462).
- [Twelve-repeat in-flight worker recovery 43804995157](https://github.com/matthis-k/phenix-ai/actions/runs/43804995157).

Independent final source/evidence review accepted the whole approved C3/C5 scope,
along with C6's separately recorded durable-restart proof. C7 is classified as an
approved exclusion, not a verified implementation; its prior numerical estimate
is unchanged. This documentation follow-up requires its own exact-head checks.
