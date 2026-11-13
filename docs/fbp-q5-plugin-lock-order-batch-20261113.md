# Q5/H3b — Plugin callback lock-order contract and wake drain interleavings

Date: 2026-11-13. **Baseline:** verified but unmerged [PR #1299](https://github.com/matthis-k/phenix-ai/pull/1299), exact head `c76e82dfeb7139fe9d0d8dfcd1b23b96ac1ede1f`, [six-job Flake CI #43427604234](https://github.com/matthis-k/phenix-ai/actions/runs/43427604234), [Loom real-wake verifier #43427603995](https://github.com/matthis-k/phenix-ai/actions/runs/43427603995), and the [native Q5 eight-repeat workflow #43427604436](https://github.com/matthis-k/phenix-ai/actions/runs/43427604436) green. All stacked PRs remain drafts/unmerged, and rejected #1295 production optimization is excluded.

## Phase 0 — Problem and constraints

The last Q5 work established actual `WakeSignal` Mutex/AtomicBool ordering under bounded Loom schedules, but **not** the lock-order boundary where the FBP runtime calls *plugin-owned* code. A readiness policy, process dispatcher, or factory may acquire a plugin mutex while another thread holds that same plugin mutex and calls a `SessionNotifier`. If any of these user callbacks execute under the wake-signal mutex, the two locks invert and can deadlock. This is more consequential than adding a few more notifier permutations.

Desired guarantees, separately:
1. **Kernel-enforceable:** `WakeSignal` operations do not call plugins. The session calls custom readiness selection, dispatchers and replacement factories without holding the wake-signal mutex. A plugin may safely hold its own mutex and call `SessionNotifier::notify`, even while another thread is trying to enter that mutex from a callback.
2. **Not kernel-enforceable without further design:** Arbitrary external plugin/user locks can deadlock with *each other*, with host locks, or with synchronous callbacks even if the kernel releases its locks. Kernel cannot inspect application lock order or revoke an in-progress callback.

No new public runtime lock, callback API, plugin contract trait or scheduler layer is justified by the current evidence.

## Phase 1 — design choices

| Option | Benefit | Risk / drawback | Decision |
| --- | --- | --- | --- |
| Just document 'do not deadlock' | No runtime impact | No executable proof of the kernel's lock boundary | Reject |
| Add runtime lock-order enforcement/tracking globally | Detect some cycles dynamically | Invasive, cost on every hot path, cannot observe arbitrary foreign mutexes | Reject for Q5 slice |
| Clone and instrument private session state in Loom | Full test control | Doesn't execute actual plugin and driver code; false fidelity | Reject |
| **Three real-session plugin lock inversion challengers plus two new original-source Loom properties** | Covers readiness, dispatcher, recovery factory; true OS threads plus instrumented wake method interleavings | Bounded scenarios, not arbitrary lock graph | Select |

## Phase 2 — adversarial acceptance conditions

All three real-session scenarios in `fbp_plugin_lock_order.rs` deliberately manufacture the dangerous ordering using a user-provided `Arc<Mutex<()>>` and a bounded channel sequence:

```text
Thread A (plugin notifier)         Thread B (session driver)
  lock(plugin_mutex)                enter plugin callback
  wait for callback entered         signal "entered"
  notify session (WakeSignal lock)   lock(plugin_mutex) ← blocked on A
  unlock(plugin_mutex)               callback completes; session progresses
```

If thread B still held `WakeSignal` while invoking a plugin callback, thread A would block in notify while B blocked on the plugin mutex: a deterministic lock cycle. Main thread waits for progress under a deadline but holds no locks. Each native test repeats 12 times; the eight-repeat CI workflow therefore exercises **96 rounds per callback type**, **288 callback challenge rounds total**.

- **Custom readiness:** the ready process has a real input, the plugin callback selects it after acquiring the external lock, and one activation completes; tests that external notification is not blocked by an internal lock held across `select()`.
- **Custom dispatcher:** an actual `ActivationJob` is dispatched and run; plugin dispatcher is allowed to acquire the external lock; tests that dispatch is not nested under `WakeSignal`.
- **Request recovery factory:** a waiting request is cancelled with `RestartAffectedProcess`; the replacement `ProcessFactory::spawn` takes the external mutex while a holder calls `SessionNotifier`. The old `WakeHandle` must be invalid, fresh generation must execute and settle; tests actual lifecycle/recovery crossing.
- **Loom property 4:** two targeted notifications race two scheduler wake drains; at least one wake must be observed in either drain or retained at the end, with listener epoch exactly two and a pending wake set.
- **Loom property 5:** producer holds an external Loom mutex while notifying; driver drains wake state *then* takes plugin mutex and sends a generic notification. The modeled lock ordering must not deadlock; this property complements, but **does not replace**, the real callback integration tests.

Important limitation: The native test's plugin mutex is deliberately held by one thread and cannot prove the absence of arbitrary future lock cycles involving plugin A, plugin B, host resources or a new callback layer. The Loom tests are bounded at 3 threads, preemption bound two and maximum 20,000 explored schedules/property; the added properties use the **actual included production `wake.rs` methods** but model the host's external mutex call boundary separately. `WakeListener::wait_timeout_while` remains native-thread tested rather than Loom-instrumented because Loom 0.7.2 lacks the same API. This batch **does not** establish an end-to-end plugin sandbox, general deadlock freedom, effect revocation or liveness.

## Phase 3 — implementation and single CI acceptance batch

- Create `packages/phenix-graphs/tests/fbp_plugin_lock_order.rs` with all three callback scenarios and 12 deterministic lock challenges each.
- Add the two Loom properties to the isolated verifier `verification/fbp-wake-loom/src/lib.rs` without changing production `WakeSignal` behavior, the public graph API or the main Cargo dependency graph.
- **Expand the existing** `FBP Q5 Notifier Retirement Concurrency` workflow (eight repeats), rather than launching three new workflows. It runs the new plugin lock test alongside `wake.rs` and existing notifier-recovery tests, saves exact commit/source hashes, runner and all logs.
- Existing Loom workflow automatically checks the expanded 5 properties under the pinned standalone Cargo.lock.
- Final gate: one assembled exact-head six-job Flake CI, combined native Q5, dedicated Loom and H4/H5/H6 benchmarks. Reject any failure or meaningful performance regression. No PR opened until the complete batch and scorecard have been written.

## Phase 4 — pending exact-head results

Accept only after exact final-head CI. Subjective **candidate** issue estimates relative to verified #1299: **Q5 70.0% → 73.0% (+3.0 pp)**, **H3b 74.0% → 76.0% (+2.0 pp)**; all other issues unchanged. These values reflect bounded new lock-order evidence, not a closure of either issue. Keep Q5 Open and H3b Partial. The unweighted issue average is a tracking convenience, **not** M7 completion.
