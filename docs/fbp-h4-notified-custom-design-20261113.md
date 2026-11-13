# H4: targeted policy notifications and opt-in custom readiness

## Phase 0 — baseline and correctness constraints

Verified base: `43564cfacc6905e218fc49f11ba3485d63655668` (#1289), six-job CI and seven performance-control workflows green. The hybrid scheduler maintains `always_poll` for every `Readiness::Custom`, because a user-defined policy may consult independently mutable external state without any queue event. Consequently one hundred idle custom policies are scanned at every unrelated packet arrival or process wake. Existing unaddressed `SessionNotifier::notify()` forces a global scan; `WakeHandle::wake()` targets a process but **also** changes its `WakeSlot.pending`, potentially resuming a process in `WaitWake` rather than merely revisiting a policy.

Invariants:
- Legacy custom policy evaluation must remain unchanged by default. No arbitrary mutable state observation can be skipped without an explicit contract.
- Process notifications must never grant a ticket, resume `WaitWake`, create packet provenance or mark an absent input closed. Scheduler reevaluation must still call `selection()` / validate all reservations.
- Notification is addressed to the physical process path in the exact session. Stale handles after finish/recovery must not target a replacement process. A generic/unaddressed notification still forces the safe full scan.
- All packet/EOS/producer/request-seal/termination and staged waits remain valid. `WaitInputs(Custom)` is always polled when dynamically installed to preserve existing staged semantics.
- `WakeListener` must observe targeted and global signals, including interleavings with refresh; no lost wake notification or second lifetime/permission system.

## Phase 1 — options and chosen smallest change

| Choice | Ergonomics | Safety | Cost |
| --- | --- | --- | --- |
| Leave all Custom permanently polled | Existing API | Proven | O(custom) every refresh |
| Assume all Custom are pure/packet-driven | No new code | Invalid if external state changes | Cannot accept |
| New `Readiness::NotifiedCustom` enum variant | Explicit at composition | Safe if notifications delivered | Duplicated variant across every policy match |
| **Opt-in defaulted trait method + process-targeted notifier** | Existing `Readiness::Custom`, 1-line opt-in; host obtains `session.process_notifier(path)` | Default legacy polling; opt-in establishes notification contract | One target handle; no new scheduler state |

Implement a defaulted `ReadinessPolicy::notification_driven(&self) -> bool { false }`. A policy returns `true` only when it guarantees every **external change affecting `select`** triggers `ProcessNotifier::notify`; packet, writer EOS and request life-cycle changes remain the scheduler's responsibility. Do not auto-discover external dependencies or infer this from custom policy behavior. All existing policies use the default and continue polling.

Expose `NetworkSession::process_notifier(path: &str) -> Option<ProcessNotifier>` alongside `wake_handle`. The notifier stores the same private wake slot, signal and stable process index, and publishes its index under the **same WakeSignal mutex** used by `WakeHandle`; unlike `wake`, it never sets the slot's pending bit. Stale notifier handles fail if the process was finished/replaced or the session terminal. `WakeListener` sees both kinds of targeted notifications through the existing aggregate epoch.

Change the `always_poll` admission predicate to include `Custom(policy)` only when `!policy.notification_driven()`. A notified policy can be reached by a sealed scope with request-local closure and no packet; include it in the existing `scoped_optional_poll` lifecycle index (rename to `scoped_readiness_poll` to reflect this generalization). A notified policy not reachable from a sealed scope is packet/EOS/target-driven. Do not change default custom policy semantics or the `Readiness::Custom` enum variant.

## Phase 2 gates / targeted tests
- Legacy mutable custom without opt-in remains polled when no queue event occurs.
- Opt-in custom with queued packet + external gating does not poll during unrelated ingress, does select after `ProcessNotifier::notify()`.
- Process notifier does **not** resume a `WaitWake` process. `WakeHandle` still does. One notifying process must not activate another.
- Wake listener catches process-specific notifier, generic notifier still performs the full scan. If notification arrives after refresh drains, it remains pending.
- Terminal/finished/recovered notifier fails without affecting replacement; previous session's notifier cannot influence another.
- Sealed request reaching custom notified node keeps request-local closure reevaluation and overlapping/sealed lifecycle checks. Existing custom-invalid-selection panic/error containment continues unchanged.
- Exact-head six-job CI and existing H4/H5/EOS/wake paired controls against #1289; new candidate-only mixed custom readiness benchmark contrasts default-polled vs opt-in notified policy on 1/16/64/256 unrelated nodes (do not claim same-source baseline speedup for an API unavailable on the old commit).

## Accepted tradeoffs
Opt-in code can violate the notification contract and strand work; such a plugin is defective, not a kernel guarantee. Existing default is safe. The host must obtain a target handle for each process path; no generic-notifier narrowing without a physical target. Staged policies and arbitrary unaddressed notifications still use fallback polling. No replacement for M7 agent acceptance or memory/concurrency interleaving proof.
