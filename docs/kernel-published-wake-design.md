# Kernel published-generation wakes — Phase 0 / Phase 1

**Status:** revised design after adversarial critique, implemented provisionally on a branch; not accepted until Phase 4.
**Baseline:** `5469c55b27ff0e6e9072f9ca923f29a854f3bafd` (#1256).
**Scope:** wake-driven published generations, *not* a new kernel runtime, Graph scheduler, or store subsystem.

## Phase 0 — requirement and smallest baseline

Required use case: a long-lived embedding application must sleep without busy-polling, wake on a producer notice **or publication**, drain the current published generation and continue after a reload. Existing `Kernel::run_until` serves static kernels, but `Arc<Generation>` exposes only `drain`. Doing nothing remains correct for a manually polled host, but cannot supply notification-driven idle without a separate application polling timer.

Read and compared: `phenix-kernel/src/{lib,host}.rs`, `tests/{kernel,generations,generation_retirement,callback_errors}.rs`; `phenix-storage-graph/src/lib.rs`, `tests/host_derived.rs`; `phenix-plugins/src/manager.rs`, `phenix-graphs/src/{lib,authoring}.rs`, `docs/debt-cleanup-20261109.md`. Existing convention: typed error causes; explicit leases; no callbacks under shared producer/registry locks; Graph owns execution/contract checks; optional bridges connect Storage; Rust 2024. One graph, one executor, one plugin catalog, one wake state.

### Consumers (trivial, representative, falsifying)

Trivial unchanged:

```rust
let mut kernel = Kernel::build("test", snapshots, executor)?;
kernel.start()?;
kernel.fire("run", vec![Value::local(41_i64)])?;
```

Existing manual host unchanged:

```rust
let candidate = host.prepare(stages, options, executor, "next", |_| Ok(()))?;
let generation = host.publish(candidate)?;
for event in generation.drain(64) { handle(event); }
```

Required sleeping, reloadable host (**new API**):

```rust
let wake = WakeHandle::default();
let host = Arc::new(Mutex::new(KernelHost::with_wake(plugins, wake.clone())));
loop {
    if wake.is_shutdown() { break; } // includes continuously busy producers
    let token = wake.version(); // before active lookup
    let generation = host.lock().unwrap().active(); // unlock immediately
    let mut busy = false;
    if let Some(generation) = generation {
        for event in generation.drain(64) {
            busy = true;
            handle(event); // outside host mutex
        }
    }
    if !busy && !wake.wait_since(token) { break; } // sticky shutdown
}
```

The falsifying race is publication between reading `active` and waiting: token compare must not sleep. Early `set_waker` notices cannot admit an unpublished candidate; publication must wake *again* after swapping active and before old producer destructors.

## Concepts and mapping

| Concept | Representation / invariant owner |
| --- | --- |
| Node/port/wave/trigger/contract | Existing Graph; not mirrored or scheduled in Kernel |
| Plugin and its revision/code lease | Existing Plugin; Kernel pins exact chosen handles |
| Active generation/published pointer | Existing `KernelHost` and `Generation` |
| Admission, producer routing and retirement | Existing `Kernel` |
| Wake token, wake wait, sticky shutdown | Existing `WakeHandle` (three small public methods) |
| Driver thread/loop | Embedding application, **not** a new kernel type |
| Storage notice | Existing `StorageBinding` -> `Producer` optional bridge |

## Timing / ownership

Registration happens on a private, exclusively mutable candidate after startup but **before** publication, without locking the producer mutex when invoking `set_waker`. It checks shutdown and returns `DriverError::WakeShutdown` or `UnsupportedWake { producer }` through `LifecycleError::Driver`; preparation failure does not change the published generation. Publish commits inventory, retires old producers, releases producer lock, replaces active pointer, signals the common wake, **then** calls old producer destructors. Source wakes and publication share one sequence/condition variable. No wake itself admits graph work. The application owns its host mutex and must release it before waiting, polling, or handling events.

## Alternatives

| Design | Strength | Failure / cost |
| --- | --- | --- |
| A: no change | Zero cost; correct for manual polling | No wake-driven sleep across reload |
| B: `Generation::run_until` | Familiar static driver | Pins one generation while host reloads; locking registration can reenter; new driver path |
| C: opt-in host `WakeHandle` + host-owned loop | Same notification for publication and source; reuse producer contract | Small public wake-token API, host thread/control responsibility |

**Choice C** for reloadable, sleeping hosts only. A wins when that use case does not exist.

## Subtraction / complexity budget

`Kernel::run_until` and host preparation now share one *private* wake-registration helper. No `GenerationDriver`, `EventBus`, `KernelRuntime`, new scheduling queue, implicit timer, registration framework, or duplicate contract/persistence mechanism. Static `Kernel::build/start/fire` and manual `KernelHost::new` remain unchanged. The new API is one kernel opt-in constructor (plus a thin existing storage-derived adapter), two wake-token methods, one shutdown predicate, and one typed error variant. No bounded migration coexistence is needed: direct and published drivers have distinct lifecycles rather than duplicate execution engines.

The accepted tradeoffs and a separate reject-list audit are in [kernel-published-wake-critique.md](kernel-published-wake-critique.md).

## Phase 1 revision after diff pressure test

A continuously ready source means the caller never reaches `wait_since`; without a public read of sticky shutdown, the canonical host loop cannot stop. Expose the existing internal `WakeHandle::is_shutdown()` predicate publicly, used at the top of each host iteration. Do **not** add a reactor service or a second event channel. `wait_since` still returns false if shutdown happens while idle. This is one shared semantic cause (termination independent of idleness), not source-specific exception handling.

The pre-existing static `Kernel::run_until()` returned `Ok(())` if shutdown was already set or happened during `set_waker`. Wake registration must remain a mechanism, not a lifecycle policy: private `register_wake` only validates startup state and producer notification support. `KernelHost::prepare` checks shutdown immediately before and after registration, rejecting candidates with `LifecycleError::Driver(WakeShutdown)`. The static driver simply registers and observes shutdown in its loop, preserving `NotStarted`, `UnsupportedWake`, and graceful termination. Add regressions for both static cases. Candidate preparation continues to fail closed on shutdown.

**Important liveness limit:** publication signals before old-producer teardown, but applications holding `Mutex<KernelHost>` while calling `publish` still block other threads from reading `active()` until that mutex is released. Do not claim the event loop can make progress while the destructor is stalled; this change guarantees notification ordering and eventual wake when the mutex is released, not nonblocking publication. Achieving independent access to the active pointer would require a separate design note/host ownership redesign.

## Phase 1 revision — optional derived host integration

The nearest analogous integration caller, `phenix-storage-graph::derived::DerivedHost`, owns a private `KernelHost`. Without a corresponding opt-in constructor, a primary storage-derived observer/query host cannot use wake-driven generation publication without bypassing the adapter's mandatory `DerivedRuntime::guard_executor`. Extend this existing wrapper with `DerivedHost::with_wake(plugins, wake)` delegating only to `KernelHost::with_wake`, keeping `prepare`'s guard unchanged. Do not introduce a new type, driver, or subscription mechanism. The extra public constructor is justified by a concrete storage integration call site, and old `DerivedHost::new` remains unchanged. Test publication and wake signaling through the guarded adapter.
