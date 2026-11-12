# H3b producer ownership — Phase 0/1 design note

2026-11-12, baseline: PR #1272 at `081c439c7cc5146c5b3f6cb3275f08913e2d995f`, all six CI checks green.

## Phase 0 — problem, constraints

The blocking `Kernel::run_until` holds `driver_active` for its lifetime but `Kernel::drain` ignores that lease. A second caller can poll the driver's producers, intercept their events, compete for admission and alter producer round-robin scheduling behind the driver's back. Although `producer_drain` serializes any individual producer callback, it does not enforce exclusive *lifetime* producer ownership by the driver. A race-free solution must also handle the reverse ordering: a direct drain already in flight when a driver attempts to acquire ownership. A mere atomic read of `driver_active` before locking the drain gate has a race.

Requirements:
- No new privileged agent-loop role; kernel just enforces exclusive producer ownership.
- Preserve concurrent manual drain serialization, producer fairness, callback and destructor lock hygiene, source ownership and event reporting.
- No silently dropped poll results or silent empty reports on denied admission. On collision, return an explicit typed error.
- Preserve same-thread recursion prevention and the blocking driver's existing typed `AlreadyRunning` result for two competing drivers.
- Avoid holding `producer_drain` across the blocking driver's session preflight or entire lifetime; avoid any possible session/drain lock inversion.
- Clean latest API only; no extra compatibility alias.
- Retired producers remain retired, while explicit session access on retained generations is unaffected.

## Phase 1 — alternatives

| Option | Strength | Weakness | Decision |
| --- | --- | --- | --- |
| No change | No churn | Hidden poll/event interception and inconsistent driver ownership | Reject |
| Check `driver_active` before/after the drain gate | Minimal local diff | Driver may acquire its lease while a manual drain is already inside its callback; overlapping ownership | Reject |
| Use `Mutex` + `Condvar` for owner modes | Could wait for manual drains | New lock/wait path around callback reentrancy and potential cross-thread wait cycles | Reject |
| **Single atomic owner word, exclusive-driver sentinel + direct-drain reference count** | Race-free linearizable mode admission, no waiting and preserves serialization of concurrent direct drains | Changes `drain` to typed Result; driver may fail fast if a manual drain is active | Accept |

Implement one `AtomicUsize` owner state: 0 idle, `usize::MAX` active blocking driver, 1..MAX-1 in-flight direct drains (including callers queued behind `producer_drain`). A blocking driver CASes 0→MAX; returns `AlreadyRunning` for a driver owner, `DrainInProgress` for active direct drains. Direct drain atomically increments the count only when not driver-owned, RAII-decrements after producer callbacks and destructor cleanup, and returns `AlreadyRunning` when driver-owned. Same-thread recursive `drain` returns `ReentrantDrain` before acquiring any mutex or count. All existing direct drains preserve the existing drain gate; the driver uses the same private gated drain body without taking a second manual lease.

Public `Kernel::drain` and `Generation::drain` return `Result<Vec<ProducerEvent>, DriverError>`. This is an intentional latest-only API correction, not an additional `try_drain` compatibility path. All examples/integration tests must be updated, and usages other than producer drain (such as subscription.drain()) must not be changed.

## Acceptance and limitations

1. A live `run_until` holds the owner lease from admission until exit (including shutdown/error/unwind); external direct drains get typed denial, do not call producers or consume queued admission, and cannot steal events.
2. A direct drain already inside producer callback owns the state; a competing driver fails fast with `DrainInProgress` *before* wake registration or session mutation. Once callback completes, driver starts normally.
3. Concurrent manual drains remain serialized as before, with ordering determined by the original drain gate.
4. Same-thread recursive direct drain gets `ReentrantDrain` without panicking/poisoning the callback; a deliberate caller can report it as an ordinary callback error.
5. All former native and kernel storage tests pass and all six exact-final-head CI checks are green.
6. No fairness guarantee beyond the existing round-robin polls; no resolution of H7 saturated-output backpressure, arbitrary plugin user-lock cycles, or asynchronous delegated job cancellation.

API tradeoff: direct drains now have a checked `Result`. Concurrent direct drains are not rejected; driver startup during active manual drain fails fast instead of blocking. This limitation does not violate the primary use case because callers can retry after a finite direct-drain call.
