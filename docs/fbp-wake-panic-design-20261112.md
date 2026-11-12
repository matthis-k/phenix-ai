# H2 — Native producer wake-registration panic boundary

Status: Phase 0/1 design, 2026-11-12; baseline `4f6391a77c24571f2a5b23b2a0827a921f2d5042`.

## Phase 0 — problem and invariant check

`Kernel::run_until` synchronously invokes the plugin-owned
`Producer::set_waker`. It already temporarily extracts the producer from
the inventory, invokes it under the whole-drain gate but **outside** the
inventory mutex, restores its slot, and catches unwinding. On panic it then
resumes unwinding, so this callback alone escapes the public driver's
structured error boundary. This differs from the existing handled
factory, process, readiness, poll, preparation and activation failures.

No new scheduler, poll protocol, producer ownership concept or second runtime is
needed. Preserving full-drain serialization against generation retirement,
restoration of the producer slot, code leases, and no arbitrary claim of
reentrant safety or rollback are non-negotiable.

## Phase 1 — baseline, extension and redesign

| Option | Benefits | Disadvantages | Verdict |
| --- | --- | --- | --- |
| Baseline: resume unwinding | No API change | Plugin bug panics across a public recoverable driver interface | Reject |
| Return `UnsupportedWake` | No new enum case | Lies about the failure cause; undermines diagnostic and retries | Reject |
| **Extend DriverError with `WakeRegistrationPanicked { producer }`** | Preserves exact owner and phase; reuses existing catch and slot restoration | New enum case for exhaustive matches | **Selected** |
| New general callback supervisor | Could unify all plugin lifecycle behavior | Excess policy, allocation and new runtime concepts | Reject |

On a panic, return the new error *after the producer has been restored*.
Do not resume the plugin panic and do not re-poll or start another producer.
Do not propagate arbitrary panic payload objects into public errors.
Do not promise that a producer's own state is valid after it panics; the host
may retire the generation or choose a new driver policy. The whole-drain gate
must release on exit, so host publication is not indefinitely blocked.

Acceptance: an intentionally panicking wake registration returns the typed
failure rather than unwinding, does not start the driver, retains the same
producer for explicit handling and leaves lifecycle/resource ownership intact.
The original typed poll error path must remain distinguishable. No change to
`on_event` (host callback) panic policy is implied.
