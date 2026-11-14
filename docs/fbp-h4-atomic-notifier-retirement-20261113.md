# H4: atomic retirement of process notifier registrations

## Phase 0 — observed race

Base: full H4 candidate stack through [#1291](https://github.com/matthis-k/phenix-ai/pull/1291), exact `15714d283b45f54ac69cd2a9830feceed1ea4225`. [#1290](https://github.com/matthis-k/phenix-ai/pull/1290) introduced `ProcessNotifier` alongside `WakeHandle`; both signal a compiled process index under `WakeSignal.state` mutex after checking their `WakeSlot.active`. But `finish(index)` and request recovery currently flip `WakeSlot.active=false` **without the same mutex**, and recovery installs a new slot for the same index.

A notifier can therefore read old `active=true` under the mutex, be preempted, and publish its process index **after** the driver has replaced the slot. The old handle's delayed wake can trigger unrelated readiness checks of the replacement process. For `WakeHandle`, the old pending bit cannot resume the new slot, but the stale dirty target remains. Existing sequential stale-handle tests do not establish linearizability.

## Phase 1 — options

| Option | Additional abstraction | Correctness |
| --- | --- | --- |
| Leave atomic flag as-is | None | Old notifier may publish after replacement; poor contract |
| Generation/token on every target | New target key, token validation, lifecycle state | Correct but duplicates existing WakeSlot identity |
| **Retire under the existing WakeSignal mutex and remove the target** | One internal helper, no added runtime state | Notifications linearize before or after retirement; old queued targets cleared |

Choose the existing-lock retirement. One `WakeSignal::retire_target(process, &WakeSlot)` critical section sets old `slot.active=false`, clears old `slot.pending` and removes queued targeted index. `WakeSignal::notify_target` remains unchanged and checks slot.active under that same lock.

`finish(index)`: perform the existing input-abandonment check and process handoff transition, take an `Arc` clone of the old slot, call `retire_target` before writer/reader closure propagation. `terminate_request`: on each cancelled/restarted process, retire the old slot after owner/handoff disposal and before factory replacement. Global session cancellation/failure already calls `WakeSignal::invalidate`, which marks the entire signal inactive and clears all targets under its mutex.

## Phase 2 — independent adversarial critique

1. **No stale cross-generation delivery:** if notify wins the lock before retirement, the retired target is cleared; if retirement wins, notify observes inactive slot and returns false. Any already-drained earlier target is treated as an event from the *old* process and request recovery already uses a full-scan fallback. A new slot cannot be issued to another host before the owning session finishes recovery.
2. **No lost live notifications:** retirement only applies to processes being irrevocably finished or intentionally restarted. For a live process, the old target set and queue remain unchanged. Fresh handles get a new active slot after replacement.
3. **No target erasure for new process:** `NetworkSession` owns process replacement with `&mut self`; a notifier for the new slot cannot be obtained until recovery is complete. Removing the old index inside retirement is therefore safe.
4. **No new lock hierarchy:** use only the pre-existing WakeSignal mutex; don't call process-selection callbacks or transport while holding it. Do not obtain a second lock or create a new queue.
5. **Terminal path:** global `invalidate` already serializes notifier rejection; keep `stop_processes` behavior unchanged, since a cancellation's externally visible commitment is global signal invalidation.
6. **Observer behavior:** process retirement does not itself notify `WakeListener` (legacy behavior unchanged); request cancellation issues a global notification after recovery.
7. **Potential overhead:** finish and active-request recovery acquire a mutex once per retired process and remove a BTreeSet entry. This can regress high-volume EOS chains; paired EOS and agent benchmarks must quantify it. Do not accept a meaningful regression without a functional correctness requirement and an explicit alternative.
8. **Panics:** mutex poisoning behavior is the same as the existing notification lock; no new panic boundary.
9. **Disposal order:** do not retire a process until after `finish` checks for unconsumed input and the handoff finishes; on failed finish, retain existing global failure path.

**Phase 2 default:** accept as a correctness fix if exact-head tests and mixed benchmark pass, and EOS/wake controls show tolerable overhead. All limitations documented above must be reviewed again in Phase 4.

## Phase 3/4 evidence gates

- Add a session-level test with pending scoped work, `RequestFailurePolicy::RestartAffectedProcess`, a stale `ProcessNotifier`, and a replacement process. Verify stale notify is rejected and new notifier works after recovery.
- Retain existing 64-process/8-thread notification coalescing test and all request, closure, EOS, wake, backpressure regressions.
- Exact-head six-job Flake CI + paired EOS/wake/agent mode benchmark. Record throughput and resource impact.

No public API, legacy behavior shim, new scheduler state or new generation identifier is required.
