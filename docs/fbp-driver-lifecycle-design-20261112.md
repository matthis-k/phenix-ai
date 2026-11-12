# H3b follow-up: cross-thread callback lock cycle and single driver ownership
Status: Phase 0/1 design, 2026-11-12. Baseline: `72d56e413321b58204280b8b6b1d29d6aa5e3f1a`, verified by CI 43185486539.

## Phase 0 — problem rather than implementation
The verified H2/H3b PR corrected **same-thread** recursive guard lifetime, host callback
panic handling, and completed-session shutdown. The older issue ledger still
lists cross-thread callback cycles and no exclusive blocking-driver ownership.

Two-thread cycle: thread A holds Kernel::with_session(session mutex) and calls
Kernel::drain (waits for producer_drain); thread B holds producer_drain during
Producer::set_waker (or poll) and calls with_session (waits for session).
Thread-local recursion protection cannot detect a distinct thread's lock.
Changing the drain/retirement serialization would weaken generation safety.

Concurrent independent run_until invocations can both register producers, replacing
wake handles. Because each driver subsequently waits on a different listener, one
driver may miss a producer notification until its 60-second fallback. There is no
explicit single-driver contract.

Do nothing: two-thread deadlocks and competing consumer ownership remain possible.
Existing scopes protect only a subset of the callback boundary. Preserve the
plugin-owned process model, bounded event admissions, producer retirement and
the existing direct (non-driver) drain API.

## Phase 1 — compare alternatives

| Option | Benefits | Cost | Decision |
|---|---|---|---|
| No change | No new API | Proven cross-thread cycle and waker replacement | Reject |
| Ban any producer callback from session API | Simple cycle prevention | Breaks useful uncontended callback reads or injection | Reject |
| Move callbacks outside whole-drain gate | Permits blocking callbacks | Requires new in-flight callback accounting, retirement/snapshot lease protocol | Defer |
| Make every session call nonblocking | Cannot wait on session | Changes ordinary cross-thread behavior and fairness | Reject |
| **Use try_lock only while this thread owns producer_drain** | Retains uncontended callback session access; guarantees kernel session APIs do not wait on session behind a held gate | Explicit busy error when contended; still no arbitrary external synchronizer guarantee | Select |
| **One per-kernel atomic driver lease** | Reject concurrent driver registrations before wake mutation; frees on all exits/unwind | Single-driver rather than multiple simultaneous drivers | Select |

The session guard is private. Keep the existing thread-local `KernelLockScope`
for same-thread recursion and teach it to check whether this thread owns
`producer_drain`. Session access uses `try_lock` only under that guard.
`with_session`/`run_steps` return an actionable string on contention.
`inject` returns the original `Value` with an explicit
`ConnectionError::WouldBlock`; never mislabel as a full transport queue.
Ordinary session calls keep their blocking mutex semantics.

`run_until` must probe same-thread producer-drain reentry **before** any
session access; then obtain its session preflight **before** entering the
drain TLS scope, so ordinary driver preflight retains blocking behavior.
Acquire the driver lease after the reentrant check and before doing anything
to the provided wake handle. `DriverError::AlreadyRunning` is typed.
Retain session-before-actual-drain lock order. Release driver lease by RAII,
including errors, caught callback panics and completed sessions.

## Acceptance
A deterministic two-thread barrier/channel regression reproduces session-vs-
registration lock inversion without sleeping. Callback session API calls must
fail promptly under contention; original injected values remain owned.
Uncontended callback session access must still succeed. A concurrent second
driver must return AlreadyRunning without altering either wake handle and must
work after first driver exits. Assert a finite live graph that transitions to
Completed during run_until returns Ok without external shutdown. Run existing
whole-drain publication/retirement and callback panic regressions, strict
format/Clippy and exact-head six-job Nix CI.

## Explicit limitations
This is not an arbitrary wait-for graph analyzer. A plugin may deadlock on its
own mutex, hold external synchronization across threads or cause cycles not
using these kernel session APIs. Host Drop reentrancy has no Result channel and
remains open for its own policy. External callback side effects are not
transactionally rolled back. Single-driver restriction is per Kernel, not a
global scheduler, and does not prevent direct Kernel::drain on other threads.

## Phase 3 implementation note
Producer callbacks use `try_lock` for three Kernel session entrypoints
(`with_session`, `inject`, `run_steps`) and report a distinct busy condition
without returning a success. Kernel::drain's own internal admission already
used nonblocking `try_lock`. A per-kernel atomic RAII lease bounds one
blocking-driver lifetime and preserves the existing specific `ReentrantDrain`
error for same-thread nested calls. The implementation adds no worker thread,
scheduler, incompatible aliases or fallback retry loop.
