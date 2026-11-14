# Kernel

`phenix-kernel` composes Rust-only, in-process plugins into one native FBP network
using `PluginCatalog`, `PluginSnapshot`, `ContributionHandle`, the graph compiler,
and a persistent `NetworkSession`. It has no storage dependency or second scheduler.
See the [campaign record](fbp-hardening-campaign-20261114.md) and authoritative
[issue tracker](fbp-issue-progress-estimates-20261113.md) for current evidence and gaps.

```text
catalog / plugin stages -> candidate snapshots -> resolve imports/dependencies
    -> compose contexts + candidate artifacts -> compile/check actual graph
    -> bind host adapters -> start producers -> publish generation
    -> declared producer notice -> bounded ingress -> persistent NetworkSession
```

## ABI Boundary

Factories, `Any`, Rust trait objects and callbacks are statically linked,
in-process contracts, not shared-library ABI interfaces. Schema agreement does
not make Rust vtables ABI-safe or codecs honest. There is no native loader,
dynamic-library unload or sandbox. Plugin reload stages in-process revisions;
leases retain old code/implementation ownership until their graph users die.

## Run

```sh
cargo run -p phenix-kernel --example basic
cargo run -p phenix-kernel --example storage
cargo test -p phenix-kernel
```

`examples/basic.rs` composes a typed increment process and drives the native session.
`examples/storage.rs` uses memory and in-memory SQLite, with change notices routed
into the observer network. Subscriptions are acquired on start, not composition.
These are runnable entry points, not a claim that these commands were rerun here.

## Composition API

- Register `Contribution { id, requires, routes, patch, prepare }` using
  `Registrar::add`. The immutable `NetworkPatch` carries edits, not write authority.
  Trusted `BuildOptions.composition` supplies exact grants and priorities.
- Dependencies use owner-qualified `Key::new(owner, id)`. All contributions compose
  in topological order, with lexical owner/id tie-breaking. Duplicates, missing
  dependencies, cycles and duplicate snapshot owners fail before composition.
- Register `ContractDefinition { id, contract }`, `NetworkOffer { id, producer,
  unit, contract }`, and `ContractRequirement { id, consumer, contract }` as ordinary
  typed contributions. Producer/consumer keys must belong to the registering plugin.
- `Kernel::build_with_options` applies `BuildOptions.imports`, the trusted
  `ImportPolicy` selections and optional
  allow/deny pairs. Denial wins. A unique eligible compatible offer is selected
  automatically; missing, ambiguous or invalid selections fail closed.
- Selected providers become dependencies of their consumers. A consumer calls
  `context.import(local_requirement_id)?`, then `ResolvedImport::bind(compiled)` to
  project actual claimed endpoints. Connect them explicitly with Graph APIs.
  Consumers need not capture provider paths; automatic edge insertion is not provided.
- `CandidateArtifacts::activate(factory)` registers one candidate-local producer factory.
  There is no `Contribution.activate` field. The zero-argument factory captures
  the exact composed state and returns an owned `Box<dyn Producer>` after compile.
- `CandidateArtifacts::export(value)` registers one typed candidate-owned export. Bundle
  multiple values in a struct; retrieve using `Kernel::export::<T>(&key)`.
  Missing keys/wrong types return `None`. Shared snapshot slots are not candidate exports.
- Build validates all offers, including unused offers, all registered contract
  definitions against compiled claims/requests, exact contribution origins, and
  declared routes before constructing one native session with the supplied
  `ProcessDispatcher`. `Kernel::build` uses default options; both builders return
  `ResolutionError`. `BuildOptions.execution` holds graph-owned `SessionOptions`.
  The same options pass through `KernelHost::prepare` and `DerivedHost::prepare`.

Graph owns actual shape and semantic matching. Kernel does not copy purported
implementation shapes into offers. Per-composer origin children distinguish
contributions within the exact loaded plugin revision; PluginSnapshot binding
checks revision ancestry, while Kernel offers check exact producer identity.
Native composers retain low-level graph access: selected imports are not a sandbox
or proof that every arbitrary closure wires the selected provider.

## Producer Lifecycle

`Kernel::build` does not activate producers. `Kernel::start` activates registered
factories in dependency order; a second successful start is rejected, even with
no producers. Failure drops already-created producers and leaves the same composed
artifacts available for retry. Factories must clean up partial acquisitions;
producers must stop/release resources on drop. Producers die before graph/actions,
artifacts and contribution/snapshot code leases; shutdown is not reverse dependency order.

`Producer::poll` must not wait for events. It returns at most one input event, idle,
or a terminal error. A poll error is reported once and retires that producer;
transient unavailability returns `Ok(None)` and wakes when ready to retry. Routes
are checked against compiled public inputs before admission. `drain(max_polls)`
polls round-robin across calls; idle polls count toward the bound, full inputs retain
one event, and admission contention retains rather than repolls that event.
`ProducerError` distinguishes poll, undeclared-route and admission errors.

Neither `poll` nor `set_waker` runs while the producer-state mutex is held. The
verified whole-drain gate serializes callbacks; concurrent manual drains serialize
on it and recursive drains return `DriverError::ReentrantDrain`. A blocking driver
owns polling for its whole lifetime: competing manual drains return
`AlreadyRunning`; driver startup during a manual drain returns `DrainInProgress`.
This includes `drain(0)`, which polls nothing but still checks polling ownership
and participates in the whole-drain gate. The initial no-wait/empty-return
competing-drain policy is discarded, preserving the verified baseline contract.
Retired drains return empty. Poll panic is a terminal poll error;
wake-registration panic is a `DriverError`. `run_until(&WakeHandle, on_event)`
requires wake registration from each producer, waits when idle, and checks sticky
shutdown between bounded iterations. There is no background thread or new async
runtime. Poll count bounds admissions, not callback duration. Shutdown cannot
interrupt a callback already executing.

`with_session` and `run_steps` exclusively lease the native session without holding
the kernel session mutex across callbacks. Same-thread re-entry through that Kernel
fails fast (`inject` returns `ReentrantSessionAccess` with the original value).
Ordered `SessionOptions::execution_guards` propagates this identity to dispatched
process workers. Kernel identity enters first, followed by the StorageGraph host
guard and application/dispatcher guards. `ActivationJob::with_guard` appends;
reverse token teardown keeps identity active through application-token destruction
and process panic-payload cleanup. All Kernel session calls fail fast on
cross-thread contention: `with_session`/`run_steps` report busy, `inject` returns
`WouldBlock` with the original value, and the blocking driver reports
`SessionBusy`. Callers can retry after release. `lease_session` always uses
`try_lease_session`; `ExclusiveSlot` is only `Mutex<Option<T>>` with `try_take` and
idempotent release/RAII restoration, without Condvar or waiters. This preserves
the verified lock-inversion contract for ordinary callers, not just producers;
initial waiting claims are historical and superseded. A caller holding a plugin
lock must not wait behind a session callback that needs it. This does not prevent
arbitrary plugin-vs-plugin deadlocks, plugin-spawned thread/join cycles, or
resource-lock cycles.

Producer admission retains the whole-drain gate but injects without the producer
inventory mutex, using nonblocking session access. External publication waits for
the entire admitted drain. Host self-destruction instead marks retirement within
a callback: a later poll result is discarded, while admission already permitted
before self-retirement may finish. Callback count is not a preemption or
shutdown/publication-duration bound.

Only `SessionOptions.deferred_timeout` configures expiry, opt-in with default `None`.
Existing `overdue_jobs(now)` is passive inspection; `supervise_deferred(now)` mutates
the session. C9 `state(&self)` remains a passive snapshot, not a supervision call.
Deferred handoffs expire at cooperative supervision boundaries; late results cannot
publish. A direct `next_job` call starts timing at handoff, while the built-in driver
starts timing only after `DispatchResult::Deferred`. `run_until` wakes at the
nearest deadline via `RunReport::next_deferred_timeout`, subject to callbacks
returning cooperatively. Timeout cannot stop external work or release its leases, and
restart can overlap the timed-out worker's still-running external effects.

## Host Publication

`host::KernelHost` owns the mutable PluginSet and active generation pointer.
`plugins()` permits staging through shared access, not committing the host's set.
`prepare` validates the entire staged batch and exact post-commit
snapshot inventory, builds a Kernel, runs the binding callback, then starts
producers without polling them. `Candidate` is opaque and bound to host identity
and expected epoch; it exposes no engine or generation admission handle.

The binding hook receives `&Binding` before producer startup. This phase-specific
capability exposes typed exports, action node identities and trigger targets, but
no Kernel, Engine, query handle or admission API. Kernel remains independent of
Storage. For derived queries use storage-graph's `DerivedHost`: it always installs
the store-owned nested-query guard on dispatch and worker threads. Validate with
`QuerySpec::validate_binding(binding)` during preparation, then call
`DerivedGeneration::query(&spec)` after publication. Arbitrary native callbacks
and exported application values can still cause external effects; those are not
sandboxed or rolled back.

`publish` rejects stale/foreign candidates and waits for the old generation's
admitted **whole producer drain** before committing the validated plugin batch,
marking its producers retired and replacing the active pointer. An in-flight drain may finish
all polls/admissions before retirement; no new drain can poll after retirement.
Same-drain publication returns `LifecycleError::ReentrantDrain`, rather than
waiting on its own gate. A nonreturning producer callback can delay external
publication indefinitely; the initial candidate no-wait claim is superseded.
Already-started external work is not revoked. A retained
`Arc<Generation>` pins its old graph, exports and leases; explicit old session calls
remain valid, but retired `drain` calls return empty. Host drop also retires active
producers. Host self-destruction from a producer callback marks retirement without
waiting on its own gate, discards a late poll result, and defers destruction until
the enclosing drain/registration releases ownership and locks. Detached producer
destructors complete outside the state lock and before old leases are released.

`Generation::with_kernel` gives scoped access to compiled metadata, exports, and
explicit session operations. It is not a read-only security capability: `Kernel`
uses interior mutability. Explicit operations do not hold the producer mutex or
block publication. The supported derived adapter queries within this scope.

## Merge Evidence

PR #1302 on `fix/fbp-lifecycle-request-lanes-20261114` first committed `0d145f3`
after incorrectly relying on stale local remote-tracking impl `16e77f6c6`.
Actual remote impl `89e00d0` includes later foundations. After fetching, exact
verified PR #1300 head `516e0883875d5ec483e3595d50b77b3d5e8979c4` is now
being merged, retaining the whole-drain and exclusive-driver gates and all other
scoped acceptance obligations. `generation_retirement` tests whole-drain waiting;
`host.rs` tests self-retirement, and the original
`concurrent_session_calls_fail_fast_without_kernel_lock_inversion` regression has
no synthetic producer callback marker. `native_sessions` requires busy on ordinary
cross-thread contention and successful retry after release. The three revised
session Loom properties cover competing `try_take` exclusive ownership, busy
callers unable to steal ownership plus successful retry, and idempotent early
release/state publication. Their exact merged-head acceptance is still pending.

Local workspace, no-default-feature, Clippy, Loom and Nix passes on initial
`0d145f3` are not acceptance for the new merged head. PR #1302 remote workflows
initially conflicted; manual [Flake #43588157944](https://github.com/matthis-k/phenix-ai/actions/runs/43588157944)
targets that initial commit, not the final merge. Full new-head acceptance remains
pending. No tracker score increases, issue closures or M7/M8 completion are claimed.
The new merge head's performance is unmeasured; old microbenchmarks and scoped
stack measurements are not new-head timing, allocation or RSS acceptance.
Optional `BoundedLanes` isolates connection/ingress/capture queue credit only, not
process state; no missing production-agent or OS-isolation adapter is finished.

## Limits

Build/bind/start failure leaves the active inventory/generation unchanged, and
dropping abandoned candidates releases owned resources. Preparation can acquire
external resources before publication. This is **not guaranteed transactional
rollback of arbitrary external callback effects**, nor panic/process-failure
atomicity. Native callbacks, partial-acquisition cleanup and destructors are trusted.

There is no automatic retry, durable delivery, distributed transaction, native
ABI guarantee or live permission revocation. Claims establish declared semantic
membership, not behavioral correctness, scheduling or authorization. Composition,
activation, producer polling and host binding accept `CallbackError` (an owned
`Box<dyn Error + Send + Sync>`). Their phase envelopes retain contribution identity
where applicable and expose the original cause through `Error::source`, without
a string-only activation wrapper. Plugin failures support owned sources and record
the plugin and construct/contribute/bind phase; binding also records the contribution
id. Staging rejects a replaced registrar owner before accepting its inventory.
Driver setup errors distinguish not-started, callback contention, re-entry and
unsupported/panicking wake registration. Invalid metadata and runtime diagnostics
still have string-based paths; callback errors do not reconstruct types already lost
by a caller. Schema identities use reclaimable `Arc<str>` ownership with no
global interner. Graph resources are explicitly supplied and granted;
storage authorization and backend lifetimes remain external adapter responsibilities.
