# Phase 0–1: explicit blocking-driver retirement (H3b)

Date: 2026-11-12. Base: verified integrated H3b/H7 candidate `5d5744b2a257958543c0bf281871883eede5d9f6` (PR #1271).

## 0. Constraints, baseline and problem

`KernelHost::publish` and `KernelHost::drop` mark the old generation's producer state `retired`, but an already-running `Kernel::run_until` can still block for up to its 60-second wait interval on a live network with open ingress. If no external wake/shutdown arrives, it may loop forever. A host-drop during `Producer::set_waker` is also silently treated as normal start when wake-registration succeeded. This is a lifecycle bug, separate from earlier H3b deadlock fixes.

Non-negotiable: retirement stops producer activity without silently cancelling or revoking an explicit session on a retained `Arc<Generation>`; accepted session work and code leases remain valid. No background thread, extra scheduler, global timeout change, or direct-`drain` ownership change. Avoid silently calling `WakeHandle::shutdown` on a user-owned handle.

## 1. Compare designs

| Choice | Advantage | Cost / disposition |
| --- | --- | --- |
| Do nothing | No new state | Hanging retired driver remains: reject |
| Poll retired state every short timeout | Local, easy | Busy/periodic polling, slow or arbitrary latency: reject |
| Cancel the session on retirement | Wake by state change | Violates retained generation explicit work: reject |
| Extend existing driver's wake lease with retirement notification | Event-driven, no new scheduler, typed stop | One short-lived wake registration on Kernel, cleanup on all paths: choose |

Add a private `driver_wake: Mutex<Option<WakeHandle>>` tied to existing exclusive driver admission. Register a cloned wake handle with an RAII scope for the full driver lifetime (including errors/panics). On `KernelHost::publish` / `KernelHost::drop`, after the old inventory is marked retired, signal that existing handle via `wake()` without changing its sticky shutdown flag. The wake's previously registered `SessionNotifier` uses the existing epoch-latched `WakeListener` for lost-wakeup safety.

`run_until` checks producer retirement after initial live-session preflight, after wake registration, before each new producer-poll iteration, and before the idle wait. It returns new typed `DriverError::Retired` without closing public input, cancelling accepted work or mutating the native session. If the session is already Completed/Failed/Cancelled, existing terminal result takes precedence. Typed wake-registration errors continue to take precedence over retirement when the callback itself reported rejection/panic.

## Acceptance

- Retire via `publish` from another thread while `run_until` sleeps: driver gets `Retired` promptly without external shutdown; old session remains callable.
- Retire via `drop` while driver sleeps: same.
- Retire during `set_waker` callback: return `Retired` without relying on a pre-shutdown handle.
- RAII wake registration cleared on success, typed errors and unwind. No dangling notifier retained in Kernel.
- Concurrent drivers still reject before modifying wake state; all prior H3b/H7 tests pass.
- Remaining direct-`drain` vs blocking-driver ownership and arbitrary external cross-thread synchronization remain open.
