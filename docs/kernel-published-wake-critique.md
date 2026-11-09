# Kernel published-generation wakes — Phase 2, adversarial critique

**Review independence:** same author in a separate pass, not independent. **Initial note rejected:** signaling publication after arbitrary old-producer destruction can strand an already-sleeping host or delay liveness indefinitely. **Revised note:** signal after visible active-pointer replacement, before destructors and without holding the producer lock. Conditionally accepted for implementation, not fully verified.

## Falsifying consumer

A driver captures `let token = wake.version()` then obtains generation A; thread B publishes generation B while A is being drained. The driver must observe the changed token and reacquire `host.active()`, never sleep on A. Synchronous candidate `set_waker` is equally adversarial: it must not cause candidate admission, and publication must signal again. A stalled destructor must not postpone that second signal.

## Reject-list audit

| Reject criterion | Specific disposition |
| --- | --- |
| Abstraction without distinct responsibility | No new runtime type; existing wake state owns condvar/sequence/shutdown |
| Conceptual distinction mirrored as structure | Reactor remains an application loop, not a new kernel object |
| Wrapper for flawed model | No `GenerationDriver` that hides the pointer swap |
| Duplicate existing capability | Reuse Graph fire, Producer set_waker, existing condvar |
| Harder trivial code | Kernel build/start/fire, manual host unchanged |
| Runtime computation resolvable earlier | Candidate registers producers before publish |
| Edge cases solved individually | Early source wake and publication use same sequence protocol |
| Many call sites changed for a one-off | Opt-in constructor; no migration of existing callers required |
| Second way to do the same thing | Static `run_until` applies one kernel; reloadable host loop follows active pointer |
| Complexity without subtraction | Centralize producer wake registration; no second queue or scheduler |
| Fails foreseeable use case | Initial design failed stalled-destructor wake ordering; revised design signals first |

The stronger existing-abstraction alternative (`Generation::run_until`) is real and useful for static generations, but fails the hot-reload use case unless wrapped in a host loop anyway. Do-nothing remains preferable for manual polling.

## Limitations and acceptance authority

| Affected use cases / limitation | Rationale | Consequence | Accepted by |
| --- | --- | --- | --- |
| Opt-in sources without waker support | Cannot guarantee blocking liveness | Preparation rejects; choose manual mode or a timer-backed adapter | Protocol default: wake-driven case requires notification |
| Exactly-once wake counting, replay, durability | Wake is a hint; source owns pending notices | Signals may coalesce, host drains to idle; no replay guarantee | Protocol default: source notice semantics unchanged |
| Interrupting already-running polls or waves | Existing Graph cooperative execution and lifecycle guarantee | Publication may wait old drain; external side effects not rolled back | Protocol default: established behavior |
| Kernel-owned reactor/background threads | An application can implement the loop without a second scheduler | Embedder supplies thread, synchronization, yielding, shutdown | Protocol default: no primary scenario requires built-in thread |

**No default acceptance** for requirements of preemptive interruption, poll-only source wakeup, or rollback of external effects. Such requirements return to Phase 0. All accepted limitations must be rechecked against the actual diff in Phase 4.

## Evidence needed before acceptance

Run on exact final head: fmt; all workspace tests (default and no-default); strict Clippy; Nix flake check; count and inspect affected `run_until`, `set_waker`, `KernelHost`, `LifecycleError`, `WakeHandle` call sites; look for compatibility/parallel paths; compare static/manual execution before and after; verify candidate nonadmission, wake token races, unsupported and shutdown failure isolation, stale generation, stalled producer teardown, and storage bridge observations. No independent review is claimed by this file.

## Second critique — revision required (shutdown starvation)

The separate diff pass found that the representative host loop never calls `wait_since` under continuous load. No public shutdown query existed, so sticky shutdown was not observable without blocking a busy reactor. The revise-first correction adds the already-existing internal `is_shutdown()` as a public observation, and updates the consumer example. It is not a new kernel mechanism. The static driver also regressed from graceful shutdown to `WakeShutdown` error when the handle was shut down during registration. Rather than adding a special-case error translation, keep private `register_wake` purely about registration and put shutdown policy in `KernelHost::prepare` (before and after). This restores static `Ok(())` and preserves candidate rejection, both tested.

The stalled-destructor signal test proves notification precedes destructor entry, **not** that a reactor can acquire an application-level `Mutex<KernelHost>` during a destructor. That cannot be claimed as nonblocking activation. A requirement to make progress before destructor exit is not accepted by protocol default and needs its own Phase 1 redesign; the scoped requirement remains eventual wake after a publication.

**Review independence remains unmet:** separate critique passes were made by the implementer. This follows the protocol's allowed fallback but does not claim independent approval.
