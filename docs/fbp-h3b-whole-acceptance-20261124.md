# H3b Whole Original Acceptance

Base: local `integrate/fbp-foundation-acceptance-20261123` at
`1589ad0d2828fecea99f784ac8e64fe952e821bc`. Worktree:
`/tmp/opencode/fbp-h3b-closure-20261124`, branch
`test/fbp-h3b-whole-acceptance-20261124`. The implementation-stage statements
below are superseded by the final acceptance recorded here.

## Accepted Source

Original H3b is **scoped verified: 78.0% -> 100.0%, +22.0 pp**, accepted source
`e166656379ac8837c7a51e1e0a3e84752ad42e85` on existing draft #1392.
Independent whole-original-scope source review and a separate exact-head
verification pass found no remaining actionable defect or acceptance gap.
This is separate-agent review, not external human approval or a main merge.

All **17 existing workflows**, **23 job/check records** and **six Flake jobs**
are successful on that exact source, all on attempt one. Key hosted evidence:

| Gate | Run | Actual Evidence |
| --- | --- | --- |
| Full exact-head Nix matrix | [45312546532](https://github.com/matthis-k/phenix-ai/actions/runs/45312546532) | All six jobs; aggregate `138174823760`: ten exact-head checks across three shards |
| Session ownership Loom/native | [45312546093](https://github.com/matthis-k/phenix-ai/actions/runs/45312546093) | Loom job `138171572121`: 12 properties / 488 schedules, all `inconclusive=false`; native job `138171572324`: all five new library tests pass |
| Actual wake Loom | [45312546117](https://github.com/matthis-k/phenix-ai/actions/runs/45312546117) | Unchanged five properties / 231 schedules |
| Native notifier retirement | [45312585335](https://github.com/matthis-k/phenix-ai/actions/runs/45312585335) | Exact-source repeated real-thread tests pass |
| In-flight worker recovery | [45312546149](https://github.com/matthis-k/phenix-ai/actions/runs/45312546149) | Exact-source cancellation/replacement/late-return tests pass |

Hosted default Kernel job `138171882984` and integration/no-default job
`138171883344` explicitly run all five `host::h3b_closure_tests` and the new
reused-worker test successfully. The latter executes 20 real jobs on one OS
worker, not 20 workflow repetitions. Separate isolated Graph no-default tests
and doctests pass; workspace feature unification is not mistaken for isolation.
The source, schedule counts and test-to-invariant map were independently
inspected rather than inferred from green aggregate status.

The original kernel-owned error/execution boundary and its lifecycle refinements
are accepted, not universal native deadlock freedom. The exclusions below are
the existing coordinated-native assumptions and do not excuse any kernel-owned
lock, lease, notifier, identity or TLS bug. C7 remains unchanged and gains no
sandbox credit. Later reconciliation edits require their own exact-head checks.

## Phase 0: Constraints

The original finding introduced by `4f6391a77` was **H3b Session/drain recursive
callback reentrancy**: readiness under the session mutex and recursive
`Producer::poll -> drain` under the whole-drain gate. It required a separate
error contract or execution boundary, not support for arbitrary recursive work.
This package maps that whole finding, its accepted lifecycle refinements, and
the remaining finite compositions. It is not a new narrower replacement issue.

The accepted [reentrancy design](fbp-h3b-reentrancy-design-20261112.md) and
[callback boundary](fbp-callback-boundary-20261113.md) select fail-fast rejection,
exclusive mutable ownership, and coordinated native lifetimes. Historical
blocking-session/waiter and recursive-drain-panic proposals are superseded by
the actual typed, try-only API. Preserve:

- Same-session reentry returns `Reentrant`; cross-thread contention returns
  `Busy`/`WouldBlock`, preserving the rejected input. No session wait queue.
- Producers run without the inventory mutex. The whole-drain gate still spans
  polling and eager admission. External publication/drop waits for that gate.
- Manual drain scopes may coexist while serializing on the gate. A driver owns
  polling exclusively for its entire lifetime. Rejected entries release nothing.
- Self-retirement rejects a late poll result; admission permitted before eager
  readiness retires its host may finish. No subsequent producer callback runs.
- Producer/event/process/code ownership survives callbacks and teardown. Cleanup
  is outside the retiring thread's gate/inventory/TLS scopes.
- `KernelHost::publish` does not wait for an explicitly leased old session.
- No production changes, test hooks, replacement scheduler, dependencies, or
  changed exclusions unless an actual-source test exposes a kernel defect.

`WakeHandle` has **one** outer `SessionNotifier` attachment. Supported hosting
coordinates attachment/driver lifetimes; this package does not promise arbitrary
simultaneous drivers of distinct kernels sharing that handle. Independent kernels
remain valid with their own handles. A caller that violates the documented
topology is not a proof about this topology, but a bug within this topology
cannot be excused by C7. C7's native trust boundary does not waive kernel-owned
lease, lock, notifier, or TLS correctness.

## Phase 1: Design

Keep the actual implementation. Add five native unit tests in
`packages/phenix-kernel/src/h3b_closure_tests.rs`, loaded only by `cfg(test)`
from `host.rs`; one reused-worker public integration test in `worker_guards.rs`;
and two small Loom properties importing the unchanged production
`exclusive.rs`. No production behavior or production test hook changes.

| Alternative | Disposition |
| --- | --- |
| No change, merely credit Q5 | Reject: the original H3b compositions need their own source/test map and review. |
| Small actual-source composition tests plus explicit source-order proof | Selected: retain the execution/error boundary, expose supported callback cutpoints, separate what each evidence type proves. |
| Force every cutpoint using new production hooks or a simulated driver | Reject: changes the execution boundary or verifies a surrogate. |
| Unlock per poll or replace the gate with `try_lock` | Reject: weakens admitted whole-drain retirement and ordinary manual-drain serialization. |
| Add a callback executor or universal plugin lock analyzer | Reject: outside the accepted fail-fast/native design and unnecessary for these finite obligations. |

The queued-drain matrix covers Busy/Full crossed with publish/drop. The real
ownership word must reach two while the first poll holds the gate. The first
drain has two iterations, so a lost pending event would cause a forbidden
repoll even if retirement wins before the second drain. Both possible next gate
winners are legal; no FIFO assertion or promise that both orders ran is made.
The event and producer each dispose once, no successor receives it, the owner
word returns to zero, and retained code-lease sentinels survive both disposals.
The sentinel exercises actual `Kernel::code_lease` retention, not dynamic library
unloading; existing real plugin/application lease tests remain necessary.

The reused-worker test sends 20 real `ActivationJob`s to **one** joined OS
worker: rejection, guard-entry panic with reentering payload cleanup, token-drop
reentry, token-drop panic, and clean operation, each followed by a clean job,
both with and without an inherited outer session identity. It asserts the same
thread identity, exact `Reentrant` errors during callbacks, independent-kernel
success, correct session failure state, continued outer rejection after an
inherited token drops, and ordinary same-kernel access after every job.

## Phase 2: Critique

This section records the implementation author's adversarial pass, **not an
independent review**. Main must independently challenge the whole original map,
cutpoint arguments, assertions, exclusions, and final diff before acceptance.

| Challenge | Disposition |
| --- | --- |
| A channel sent before `drain` proves gate admission | Rejected. The test reads the actual owner word equal to two while the first callback is held. |
| The retirement thread's start signal proves it is queued inside `Mutex::lock` | Rejected. It proves only that external retirement has started; there is no mutex waiter introspection or FIFO claim. |
| Checking payload Drop count alone proves retention | Insufficient. The first drain retries while still holding the gate; a discarded pending event causes a repoll and fails the test. |
| A destructor's global `try_lock` failure proves it was called under a lock | False with other admitted threads. The concurrent test checks this thread's TLS and reenters inventory; source order and uncontended destructor tests establish release of its own gate. |
| Host destruction in poll/set-waker covers eager readiness | False. The new readiness callback actually destroys the host after the admission permission check; injection succeeds and the graph becomes Runnable, but the next producer never runs. |
| A channel inside poll proves the driver is sleeping | False. Existing live-driver retirement tests prove arrival at poll and bounded eventual retirement, not entry into the Condvar wait. See the source proof below. |
| The entire Kernel or native listener was model checked | False. Session Loom imports the two production ownership primitives. Wake Loom imports actual `wake.rs`, but native `WakeListener` is `cfg(not(loom))`; its check-to-wait composition requires source proof plus native listener coverage. |
| Rejection/cleanup on fresh threads proves worker reuse | False. The new test reuses exactly one worker and probes both new and inherited markers after every failure. |
| Any caught panic satisfies a panic-path assertion | False. Event retirement has an out-of-callback reached-retirement assertion. Worker callbacks record independent-kernel results for assertions outside containment, with exact callback-attempt counts. |
| RAII early release on one thread proves it cannot overwrite a later owner | Insufficient. New Loom holds a mutated successor lease while the old lease releases again and drops, asserts Busy, then checks the successor's state. |
| More Loom properties imply completion despite hitting caps | Reject. Current schedule reports must all say `inconclusive=false`; unchanged counts remain historical, new counts are measured below. |

The initial new Loom harness tried to spawn a second sequential contender with
`max_threads=2`; Loom bounds total created threads, not just simultaneously live
ones. Another initial property had one observable schedule and was rejected by
the report helper. The corrected properties coordinate a single contender's
failure/retry and hold a real successor during old-lease Drop. These were test
harness failures, not production defects; neither original properties nor
production primitives were changed.

## Phase 3: Implementation And Verification

### Requirements Map

Paths below are relative to `packages/phenix-kernel` unless stated otherwise.
`L` = bounded exact-source Loom; `N` = native actual-source execution;
`S` = source-order/ownership argument, not schedule enumeration.

| Whole-original obligation | Actual source boundary | Retained/new evidence and cutpoint | Split |
| --- | --- | --- | --- |
| Readiness/session callbacks cannot recursively wait for their own session | `src/lib.rs`: `with_session`, `ActiveSessionCall`, `KernelExecutionGuard`; `src/exclusive.rs`: `try_take`/lease | `native_sessions::producer_admission_readiness_callbacks_run_outside_producer_lock_and_reentry_fails_fast`; library `recursive_session_calls_fail_fast_without_discarding_input_ownership`; new readiness-retirement test | N/S, slot ownership L |
| Contending session callers do not form the old callback lock inversion; retry preserves values | `try_lease_session`, `with_session`, `inject`, `run_steps` | Library `concurrent_session_calls_fail_fast_without_kernel_lock_inversion`, `producer_callback_never_waits_for_session_held_by_another_thread`; native Busy/retry and Full tests; new Busy/Full retirement matrix | N/S/L |
| Recursive poll/drain, wake/drain, wake/driver, and same-drain publication reject without losing owners | `KernelLockScope`, `drain`, `register_producer_wakers`, `host::publish` | Library `recursive_drain_in_poll_is_typed_and_preserves_producer`, `recursive_drain_in_wake_registration_is_typed_without_losing_producer`, `recursive_run_until_during_wake_registration_is_typed_and_non_destructive`; host publication-reentry test | N/S |
| External retirement waits for the whole admitted drain, not only its current poll | `drain_owned`; `host::publish`/`Drop` gate then inventory | `generation_retirement::blocked_poll_finishes_its_drain_before_publication_retires_the_source`; new queued-drain Busy/Full x publish/drop matrix | N/S |
| Two admitted direct drains retain driver exclusion while queued | `src/producer_ownership.rs`: direct counter, driver sentinel, RAII; `drain` admits before gate | Library `gate_queued_direct_drain_keeps_driver_excluded_until_both_exit`; new matrix observes actual count two; original gate-queued Loom property | L/N/S |
| A blocking driver and direct drain never overlap polling ownership, including zero budgets | `enter_driver`, `DirectDrainScope::enter`, `DriverScope::enter` | Library in-flight drain, live driver, competing/rejected driver tests; seven unchanged owner properties | L/N/S |
| Self-retirement from poll/wake stops later callbacks and discards late poll results | `host::Drop` same-thread branch, `producers_retired`, reconciliation and `take_retired_producers` | `host::tests::host_drop_during_producer_callbacks_defers_retirement_and_destructors` covers poll, supported/unsupported/panicking wake, later source, destructor locks | N/S |
| Self-retirement during eager readiness after permission may finish only this admission | `drain_owned`: retired check before `session.inject`, next loop retired check | New `eager_readiness_self_retirement_finishes_permitted_admission_only`: actual custom policy destroys host, exact one successful event, two producer Drops, Runnable session, no later poll | N/S |
| Startup/retirement notification cannot leave a supported driver asleep forever | `run_until`: driver/wake scopes, listener preflight, retired checks, notifier attachment, registration; `notify_driver_retirement` | New `actual_driver_startup_retirement_and_busy_exit_restore_scopes` reaches real blocked set-waker, checks direct rejection, externally drops host, gets Retired without shutdown; retained host publish/drop tests; startup source proof below | N/S; owner L |
| Retirement after polling/reporting and check-to-wait is observable | `run_until` final retired check; `WakeListener::wait_timeout`; `WakeSignal` epoch mutex/Condvar | New `actual_driver_event_cutpoint_retires_without_gate_and_restores_error_paths`; retained live-driver retirement, native `runtime::wake::tests::listener_observes_notification_across_registration_to_wait_race`; source proof below | N/S; signal operations L, not native listener L |
| Session release restores ownership before notifying, including unwind | `SessionLease::drop`: release before notify; `ExclusiveLease::Drop` | New `session_release_notifies_after_restore_on_success_and_unwind`: native listener latch, success/panic, ownership/TLS restoration; new mutation/unwind Loom property | N/S/L for slot only |
| Guard entry rejection/panic and reverse token cleanup keep exact worker identity, without stale/inherited-marker removal | `ActiveSessionCall::inherit`/Drop; `KernelExecutionGuard`; graph `ActivationJob::run` ordered guard entry/reverse exit | Retained joined-worker, application Drop, panic-payload Drop tests; new reused OS-worker matrix with post-job access and next job; independent kernel remains usable | N/S, no TLS Loom claim |
| Callback panics reconcile inventory, release gate/driver scopes, retain terminal one-error semantics | `catch_unwind` in poll/wake/event and graph guard boundaries | Library producer/wake/panic-payload/destructor tests, `callback_errors.rs`, `kernel.rs`; new event-retirement normal/panic variants and Busy startup cleanup | N/S; owner unwind L |
| Exact event disposal and executable lifetime, no successor admission | Value `with_lease`, kernel `code_lease`, detach-before-Drop, host retained generation Arc | New retirement matrix with sentinel; `generation_retirement.rs`, `activation_artifacts.rs`, `code_leases.rs`, `generations.rs`, `worker_guards::retained_wrong_type_admission_error_and_value_pin_application_and_plugin_code`, native owned-job/capture test | N/S; no dynamic-loader proof |
| Mutated session state survives unwind; old early-release/Drop cannot republish over new owner | Unchanged exact `src/exclusive.rs` imported by verifier | Two new properties below; original three slot and seven producer owner properties retained | L plus native unwind |
| Existing FIFO, bounded polling, round-robin, request/ticket/capture/wake identities remain intact | Actual graph/kernel runtime, no new scheduling authority | Full existing workspace tests, including handoff/public composition, wake operation histories, native sessions, typed admission, generations, wake host, and storage worker guards | N, retained bounded models, no universal liveness claim |

### Startup And Wait Proof

The relevant order in `Kernel::run_until` is:

1. Acquire the driver owner, register `driver_wake`, then exclusively read the
   session and create its listener/notifier. A Busy error drops wake registration
   before the driver owner. Terminal session results retain their existing
   precedence over producer retirement.
2. Check producer retirement, attach the native notifier to the outer handle,
   then register producer wakers under the gate. Registration checks retirement
   before each callback and after reconciliation. Self-retirement restores the
   checked-out producer before detaching it and drops producers outside the gate.
3. In the loop, check retirement before polling and again after session reporting.
   Event callbacks run after `drain_owned` has released gate/TLS, and may retire
   the host. The new event test verifies both ordinary and caught-panic exits.
4. Only an inactive nonterminal report reaches `listener.wait_timeout`. The
   listener predates all work inspection. `WakeListener` compares its observed
   epoch under `WakeSignal.state`; `wait_timeout_while` atomically releases that
   mutex with the wait. Retirement's native notification changes the same epoch
   under the same mutex, so a change between the final retired check and wait
   cannot disappear. Notification after the check is either visible before the
   wait or wakes it. Session release similarly publishes the lease before notify.

External retirement before `driver_wake` registration can miss that slot, but
startup's later retired check observes it. Retirement between that check and
native attachment can notify an unattached outer handle; registration's retired
check still catches it. After attachment, native notification is latched. These
arguments assume coordinated ownership of the single notifier attachment.

There is no callback between the final retirement check and wait, nor between
owner admission and wake-slot registration. Those exact instruction midpoints
cannot be held by a public callback. We do **not** add a hook or claim a channel
inside poll captures them. Native tests reach real registration/event/poll/session
release boundaries; the inaccessible midpoint is covered by this source-order
argument, native listener registration-to-wait tests, and bounded exact-source
signal operations. This is not whole-driver Loom, exhaustive native scheduling,
proof of scheduler fairness, or measured sleeping-state introspection.

### Current Model Accounting

Session verifier: Loom 0.7.2, preemption bound 2, max branches 1,000, max
permutations 20,000, checkpoint interval 1, **no duration cap**. All current
properties report `inconclusive=false`; conservative MAX-1 cap rejection retained.

| Session/Ownership property | Completed schedules |
| --- | ---: |
| Competing session callers | 56 |
| Busy session caller retry | 4 |
| Early exclusive-lease release | 1 |
| Two live direct scopes | 258 |
| Two driver contenders | 56 |
| Direct/driver active-mode gauges | 90 |
| Gate-queued direct scope retains union | 4 |
| Rejected entries preserve live guards | 1 |
| Caught unwind restores owner RAII | 1 |
| Defensive direct-count numeric boundary | 1 |
| **Original ten subtotal, unchanged** | **472** |
| Mutated exclusive-lease unwind and concurrent retry, new | 10 |
| Early release cannot republish over successor, new | 6 |
| **Current twelve total** | **488** |

The unchanged wake verifier completes five properties / **231 Loom schedules**:
old notify/retirement 42; retired owner/replacement 71; notifications/scheduler
drains 32; external plugin mutex/notifier/drain 54; invalidation 32. These are not
the **13,440 native legal wake operation histories**, nor the session schedules.
The wake helper retains its existing bounds (3 threads, preemptions 2, branches
1,000, permutations 20,000); it does not implement the session helper's explicit
`inconclusive` report. Neither verifier models arbitrary application locks.

### Local Verification

Local toolchain: `nix develop`, rustc 1.96.1
`31fca3adb283cc9dfd56b49cdee9a96eb9c96ffd`, x86_64-unknown-linux-gnu,
LLVM 21.1.8; Loom 0.7.2. All commands below are from this separate worktree,
using `--locked`, with native and Loom Rust flags kept separate.

| Gate | Local result |
| --- | --- |
| Targeted kernel library + native_sessions + worker_guards + generation_retirement + wake_host | Pass; final target inventory: 42 + 16 + 5 + 2 + 10 tests, also covered by the complete default workspace run |
| Same targeted native tests, `--no-default-features` | Pass; kernel currently declares no default feature, not a different implementation |
| `cargo test --workspace --locked` | Pass, including doctests; existing ignored manual measurements remain ignored. First quiet run hit the 200-second tool timeout in the independently compiled fixture area; full rerun with a 600-second allowance passed. |
| `cargo test --workspace --locked --no-default-features` | Pass, including doctests; existing ignored manual measurements remain ignored. |
| Session exact-source Loom `--lib --locked --offline -- --nocapture --test-threads=1`, `RUSTFLAGS='--cfg loom'` | Pass; twelve properties / 488 schedules |
| Wake exact-source Loom, same flags and independent manifest | Pass; five properties / 231 schedules |
| Strict workspace all-target Clippy, default and no-default | Pass |
| Strict session verifier Clippy, tests, locked/offline/no-deps | Pass |
| Strict wake verifier Clippy, tests, locked/offline/no-deps | Pass |
| Formatting: workspace and both independent verifier manifests | Pass |
| New native unit tests and reused-worker integration test, repeated 20 times | Pass; 120 test invocations, not exhaustive interleaving enumeration |
| Independent whole-original-scope review and exact committed-candidate CI | Completed on `e166656379ac8837c7a51e1e0a3e84752ad42e85`; final evidence above supersedes this implementation-stage gate |

Reproduction uses `nix develop --command` before each Cargo command:

```sh
cargo test --locked -p phenix-kernel --lib --test native_sessions --test worker_guards --test generation_retirement --test wake_host
cargo test --locked --no-default-features -p phenix-kernel --lib --test native_sessions --test worker_guards --test generation_retirement --test wake_host
cargo test --workspace --locked
cargo test --workspace --locked --no-default-features
cargo clippy --workspace --all-targets --locked --no-deps -- -D warnings
cargo clippy --workspace --all-targets --locked --no-default-features --no-deps -- -D warnings
cargo fmt --all -- --check
env RUSTFLAGS='--cfg loom' cargo test --manifest-path verification/fbp-session-loom/Cargo.toml --lib --locked --offline -- --nocapture --test-threads=1
env RUSTFLAGS='--cfg loom' cargo test --manifest-path verification/fbp-wake-loom/Cargo.toml --lib --locked --offline -- --nocapture --test-threads=1
env RUSTFLAGS='--cfg loom' cargo clippy --manifest-path verification/fbp-session-loom/Cargo.toml --tests --locked --offline --no-deps -- -D warnings
env RUSTFLAGS='--cfg loom' cargo clippy --manifest-path verification/fbp-wake-loom/Cargo.toml --tests --locked --offline --no-deps -- -D warnings
cargo fmt --manifest-path verification/fbp-session-loom/Cargo.toml -- --check
cargo fmt --manifest-path verification/fbp-wake-loom/Cargo.toml -- --check
```

### Final Disposition

No production defect was exposed. The whole-original map, finite kernel-owned
compositions and source-order proof received independent acceptance, followed
by all required exact-source gates. No broader lifetime criterion is deferred
under the original H3b label; arbitrary native-lock and universal schedule
proofs were never the selected error/execution-boundary contract.

Retained exclusions are arbitrary plugin-owned thread/join/mutex cycles,
nonreturning callbacks, preemption/rollback of started native external effects,
unbounded scheduling/fairness/liveness proof, arbitrary shared-handle driver
topologies, and dynamic-loader/uncoordinated native unload guarantees. These are
not newly substituted H3b requirements, nor excuses for kernel-owned deadlocks.
