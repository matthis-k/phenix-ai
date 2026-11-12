# H3b — reentrant host destruction during producer callbacks

Date: 2026-11-12. Parent: verified native FBP candidate at `1e7beeaab13c1885035d7db34a8f2875ffad2299` (PR #1267).

## Phase 0 — problem, baseline and invariants

`KernelHost::drop` always locks `producer_drain`. `Producer::poll` and `Producer::set_waker` execute while their kernel's drain gate and `KernelLockScope` are held, but outside the producer inventory lock. A callback can transitively destroy its owning `KernelHost`; waiting on its already-held non-reentrant drain gate deadlocks the thread. Existing fixes cover session/drain cycles and nested drivers, not host destruction.

Doing nothing retains a reproducible synchronous hang. Desired behavior: destruction must not block on its own gate; retirement must prevent any subsequent producer admission; all remaining producer destructors must run **after** gate and inventory locks are released, without losing code leases. Keep external `Arc<Generation>` and explicit session capabilities unchanged. Do not invent revocation policy for retired generations (C3).

## Phase 1 — choices and design

| Approach | Strength | Rejected weakness |
| --- | --- | --- |
| Baseline blocking `Drop` | Simple normal path | Deadlocks inside callback |
| `try_lock` then skip retirement on failure | Avoids immediate deadlock | Leaves active producers after host ownership disappears |
| Spawn an asynchronous disposer | Can wait on gate elsewhere | Introduces unsupervised lifetime/thread scheduler and nondeterministic disposal |
| **Defer within existing drain scope** | No new scheduler, preserves serialization and destructor ordering | Needs every producer callback exit path to flush deferred retirement |

When the same thread already holds the current generation's drain gate, host destruction marks `ProducerState.retired` under the short inventory mutex and returns without emptying `producers`: the in-flight callback's slot is temporarily checked out and must be restored first. Existing `drain` and `run_until` wake-registration paths check `retired`, detach the full inventory once callbacks finish, and only **drop** those producer objects after releasing both locks and the TLS drain scope. Normal cross-thread destruction keeps its existing blocking retirement semantics. A small private helper performs the detach; no new public API or scheduling abstraction is introduced.

Wake registration must not return early on unsupported/panicking callbacks before the deferred retirement is flushed. After restoration, it records the original typed error, stops registration, flushes retirement, and returns that same error. Once retirement is requested, later producers must not receive further wake-registration callbacks.

## Acceptance

- A producer drops its host inside `poll`; no deadlock, no extra poll, `retired=true`, no surviving producer slots, destructor executes with gate/inventory unlocked.
- A producer drops its host inside `set_waker`; no subsequent registration, cleanup executes outside locks, no hang; initial graph must be live so registration runs.
- Ordinary host destruction still retires synchronously; standard CI, format/Clippy and existing lifecycle tests remain green.
- Any limitations not closed here remain open: external cross-thread synchronization cycles, direct-drain versus blocking-driver ownership, implicit driver shutdown on retirement, C3/C6/C7, and H7 request fairness.
