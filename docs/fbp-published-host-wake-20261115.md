# Published Host Wake Hints

Status: bounded WIP1258 native implementation, acceptance pending CI. This is not
a general reactor, a replacement for native `Kernel::run_until`, or a new
generation-authority API. It adapts the current native kernel rather than
restoring an earlier Engine implementation.

## Wake Contract

`WakeHandle` keeps a wrapping `u64` version and a sticky shutdown predicate in
one mutex, plus a condition variable. `version()` snapshots that version;
`wait_since(observed)` loops through spurious wakes until the version differs
or shutdown is true, and returns the current version. `is_shutdown()` reads the
predicate. A caller arriving after shutdown never blocks, even when passing
the current version. Use a fresh handle for a fresh lifecycle.

Both `wake()` and `shutdown()` update the version and notify all waiters before
forwarding to an attached native `SessionNotifier`. The epoch mutex is released
before native forwarding. Native blocking-driver forwarding is preserved; it
is not replaced by the condition variable. These versions are change hints,
not event counts, durable delivery, generation identities, or publication
authority. Several hints can coalesce into one inspection. As with any wrapping
version, an entire wrap between inspections is not distinguishable.

Snapshot the version **before** inspecting the active generation and work. A hint
between inspection and wait then makes `wait_since` return immediately. All
waiters inspect their own predicates; there is no consumption of a wake token.

## Preparation And Publication

`KernelHost::new(plugins)` remains manual and does not register wake callbacks.
`KernelHost::with_wake(plugins, wake)` opts into producer and publication hints.
Its preparation order is:

1. Build the exact staged inventory and native kernel.
2. Bind through the existing nonadmitting inspection capability.
3. Activate producers without polling or executing process steps.
4. Reject an already shut down handle as `LifecycleError::Driver(DriverError::WakeShutdown)`.
5. Register the host handle once on each private candidate producer.
6. Check shutdown again before returning the candidate.

The shared private kernel helper performs registration only. It acquires the
whole producer-drain gate, checks out each producer under the inventory mutex,
calls `set_waker` outside that mutex, and restores the slot before handling its
result or disposing a panic payload. It preserves typed unsupported/panic
errors, same-thread reentry detection, and retirement cleanup after releasing
both the gate and its reentry scope. It does not acquire a session, attach a
session notifier, poll producers, or run process steps.

The static driver retains its existing session preflight, terminal-entry
behavior, lifetime producer ownership, graceful shutdown, and error precedence.
It calls the same helper after its own native notifier setup. An already shut
down static-driver handle still follows the existing static registration and
graceful-exit behavior, not the host preparation rejection policy.

Preparation is not free of external effects: activation/subscription may do I/O,
and registration on a subsequently rejected candidate may signal wake hints.
Failure releases owned candidate resources without changing the active pointer,
committed plugin inventory, or host publication epoch. Neither arbitrary
external effects nor the wake version are rolled back.

The shutdown checks are not atomic publication revocation. Shutdown after the
second check does not invalidate a candidate, and publication of an already
prepared candidate remains permitted. Applications needing a stronger policy
must serialize their shutdown and publication decisions themselves.

Publication first validates and commits the candidate, retires old producers,
and replaces the active pointer. It then releases the old inventory and
whole-drain gate/scope and signals the host handle, including on first
publication. This happens before old producer destructors, which can stall.
The existing separate static-driver retirement notification remains intact.
Retained old generations cannot poll retired producers but keep their explicit
native injection, execution, query, export, and session capabilities.

`DerivedHost::with_wake` is a thin forwarding constructor. Its existing prepare
path still installs mandatory storage execution guards before application
guards, including on offloaded workers. No storage dependency is added to the
kernel, and no derived query/runtime behavior is changed here.

## Reactor Boundary

An empty bounded drain is **not an idle proof**. With two producers and a poll
budget of one, the first can be idle while the second has pending work. A
producer event retained for native backpressure or session contention also
returns no admission event. Sleeping at that point can strand it without any
new producer hint.

An application-owned reactor therefore needs a known producer-count bound and
a full sweep, bounded native execution, draining of captures/backpressure,
and retries after progress or an exhausted execution budget. If it splits a
sweep across iterations, it must finish that sweep before considering sleep.
The producer bound belongs to the application's deployment knowledge, not a
new kernel authority API. If that bound is unknown, this interface alone does
not justify sleeping. Budget exhaustion means retry/yield, not idle.

Host versions cover producer calls to the registered handle and publication.
Graph process wakes, worker completion, deferred deadlines, explicit injection,
and release of a contended session lease do **not** automatically advance the
host version. A full producer sweep cannot make those sources safe. General
workloads must keep the native session wake/deadline mechanism or arrange
explicit application coordination and bounded retries for every such source.
Do not replace native `run_until` or a general scheduler with this host wait.

For a restricted deployment with synchronous completion, one session owner, no
deferred jobs/deadlines or independent process wakes, and no uncoordinated
injectors, the application loop has this shape:

```rust,ignore
loop {
    if wake.is_shutdown() { break; } // Also checked on every busy iteration.
    let observed = wake.version();
    let (generation, producer_bound) = {
        let deployment = deployment.lock().unwrap();
        (deployment.host.active(), deployment.known_producer_bound)
    }; // No host/deployment mutex during callbacks, processing, or waiting.
    let Some(generation) = generation else {
        wake.wait_since(observed);
        continue;
    };
    let events = generation.drain(producer_bound)?; // One complete sweep.
    report_events(&events)?;
    let report = generation.run_steps(STEP_BUDGET)?;
    let captures = drain_application_captures(&generation)?;
    check_terminal_state(&report)?;
    if !events.is_empty() || report.steps != 0 || report.publications != 0
        || report.budget_exhausted || report.state == SessionState::Runnable
        || captures != 0
    {
        continue; // Includes retrying any previously retained producer event.
    }
    // Only the restricted, fully coordinated deployment above permits this.
    wake.wait_since(observed);
}
```

The application must not hold a host mutex while waiting or processing. A
publication can race with the snapshot; the retained old generation remains
valid and the publication hint forces another inspection. Shutdown is checked
even when sources are continuously busy, but cannot preempt an in-flight
producer callback, process step, guard, or destructor.

A host reactor and static driver using **different wake handles** on a hosted
kernel must not be implicitly mixed. Static registration overwrites producer
wakers; exiting the static driver does not restore the host's registration.
Even sequential handover back to the host reactor is unsupported with different
handles. This implementation deliberately adds no restore/handover API. Use a
single coordinated mode/handle or prepare a fresh deployment. Existing driver
ownership prevents concurrent manual polling, not this wake-topology mistake.

## Verification Scope

Kernel wake-host tests cover version-before-wait races, multiple waiters, sticky
shutdown, private registration without polling/steps, manual opt-out, publication
without producer hints, unsupported/panicking/shutting-down registration,
candidate cleanup and unchanged committed inventory/epoch, signaling before a
stalled destructor after gate release, native driver retirement, old explicit
usability, partial sweeps, retained backpressure, and shutdown under continuous
events. Existing static-driver regressions remain unchanged.

`idle_reactor_reloads_on_publication_then_processes_source_wake_and_idle_shutdown`
adds the end-to-end restricted-reactor acceptance path. It captures A's output,
settles a full known two-producer sweep, enters `wait_since`, and captures B's
epoch/output after publication alone (registration emits no hint). It then
queues another B event and signals the producer's registered handle, captures
the new B output, and verifies shutdown releases the idle wait. A second
channel-gated schedule forces publication after observing A but before calling
`wait_since` with A's version. Producer callbacks, process execution, and capture
draining assert the host mutex is unlocked; the publisher also acquires it while
the reactor is idle. Channels order transitions, bounded receive timeouts assert
that idle waits do not return early, and no sleeps or public test hooks are used.
This covers synchronous, single-owner source/publication-driven execution only,
not the independent session wake sources excluded above.

Storage host tests use real observations and captured outputs through source
updates and reload, verify private candidates do not execute, verify retired
observers unsubscribe, and exercise mandatory-before-application guard ordering
and reverse teardown on actual local/offloaded workers. Only kernel and storage
graph packages are in this change's test scope. Concurrent native A3 migration
changes are not reverted or included in this implementation's ownership.

Local verification: `cargo test -p phenix-kernel -p phenix-storage-graph` passed,
including both packages' doctests, the original nine kernel wake-host tests, and two
new storage host wake tests. Owned Rust files passed a scoped rustfmt check;
`git diff --check` passed. This is local evidence, not CI acceptance or issue
closure.

After the end-to-end acceptance addition, the focused test and
`cargo test -p phenix-kernel --test wake_host -p phenix-storage-graph --test host_derived`
passed: 10 kernel wake-host tests and 7 storage host tests. The owned test file's
rustfmt check and `git diff --check` also passed. CI acceptance remains pending.

Independent final review identified the missing sleeping/reload acceptance case;
the regression above was added and separately re-reviewed. That finding is closed.
A subsequent assembled resource/terminal/wake review found no actionable integration
defects and supports bounded original #1258 acceptance, conditional on exact-head
hosted checks. Full assembled workspace/default/no-default, formatting and strict
all-target Clippy now pass locally. This does not expand the reactor contract.
