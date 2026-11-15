# Native Network Kernel

The kernel resolves plugin contributions, builds host-authorized native network
patches, and leases a persistent `NetworkSession` without holding the ownership
mutex through plugin callbacks. Contending Kernel session access fails fast.
It adds no scheduler or
legacy execution adapter. Native Rust plugins are not a sandbox or a dynamic ABI.

## Composer Registration

One authoring object registers its composer and offer/import metadata atomically.
The destination registrar supplies the owner; authors never repeat producer or
consumer keys. The private registered payload is not another public authoring API.

```rust,ignore
Contribution::new("service", static_patch)
    .after([Key::new("dependency-plugin", "main")])
    .route(["external-input".into()])
    .prepare(|candidate| Ok(CandidateArtifacts::default()))
    .offer("service-offer", "actual-service-path", definition_key)
    .import("dependency", dependency_definition_key)
    .register(registrar)?;
```

`after` and `route` append. Offers/imports accumulate; duplicate IDs reject the
entire bundle without installing a composer, metadata or reserved names. A repeated
`prepare` replaces the authoring callback. Offer/import IDs retain owner-wide,
per-kind namespaces, so host `ImportPolicy` keys do not change. Distinct offer IDs
remain distinct providers even if one composer creates both. No public standalone
offer/requirement records or `Registrar::add(kernel::Contribution)` route remain.

## Candidate Artifacts

```rust,ignore
pub type Prepare = dyn Fn(&CandidateContext<'_>)
    -> Result<CandidateArtifacts, CallbackError> + Send + Sync;
pub struct CandidateArtifacts {
    pub patch: Option<NetworkPatch>, // Replaces the static patch for this candidate.
    pub activate: Option<Box<Activate>>,
    pub export: Option<Box<dyn Any + Send + Sync>>,
}
pub type Activate = dyn Fn() -> Result<Box<dyn Producer>, CallbackError> + Send + Sync;
```

`CandidateArtifacts::default()`, `activate(factory)` and `export(value)` provide
single-slot registration with duplicate detection. The prepare callback receives
read-only `CandidateContext` with `import(id)` metadata and
`bind_import(id)` scoped projection, never a raw compiled topology or mutable
candidate/builder. It may construct its own definitions or an owned patch. Replay
is exclusively native `Composer` work. Self-imports are checked against a compiled,
host-authorized preview of the static patch; they cannot certify fabricated shapes.

Host `BuildOptions` contains `imports: ImportPolicy`, `execution: SessionOptions`
and `composition: BTreeMap<Key, CompositionPolicy>`.
Host `inspect_network: BTreeSet<Key>` is a separate default-deny read grant.
A preparation callback may call `inspect_network()` only with an exact host
grant; this preserves trusted compiled exports without exposing all topology
to ordinary declared-import consumers. Each `CompositionPolicy` has
`grants: BTreeSet<Footprint>` and `priority: i64`. Missing policy grants no writes.
Grants and priorities are host decisions, never fields in plugin patch payloads.
Host `selection_limits: phenix_graph::compose::SelectionLimits` defaults to
`max_work: 1_000_000`. The budget applies independently to each `Composer` update
and each self-import preview, not to the total kernel build. Incremental
registration is unchanged: every registered prefix must independently have a
unique valid selection. Budget exhaustion remains a typed `ComposeError` source
in `ResolutionError::Composition`. Limits do not preempt all callbacks: ordinary
preparation runs before main registration; self-import preview failure runs before
that contribution's preparation. External callback effects are not rolled back.
The build's dispatcher argument overrides `execution.dispatcher`; a snapshot code
lease is added without discarding an application-provided lease.

Side-effect-only processes declare no outputs. Intentionally unconnected emissions
require explicit `execution.discard_outputs` entries naming the physical process
path and output; the kernel does not silently grant discard permission.

## Contracts

- `ContractDefinition { id, contract: phenix_graph::contract::Contract }`.
- `.offer(id, unit, definition_key)` advertises an actual component path owned by
  this composer; the internal producer key is derived during registration.
- `.import(id, definition_key)` declares a local requirement for this composer.
- `ImportPolicy { selections, allow, deny }` is host-owned; denial wins and unknown policy keys reject.

Missing, ambiguous, incompatible, foreign-owner and sibling-producer offers fail
closed. All definitions and all offers, including unused entries, are checked.
Each producing contribution receives an exact child `GraphOrigin` within its
leased plugin revision. Final validation uses `compiled.inspect(path).origin`,
`compiled.check_implementation(path, &contract)` and the native contract catalog.
An origin is provenance, not write authority or a code lease.

`ResolvedImport::bind(&CompiledNetwork) -> Result<BoundComponent<'_>, NetworkError>`
uses the actual native path/checker and returns only contract-visible ports.
Local ports require Rust type identity. Wire ports require complete matching
schema identities, allowing independent Rust implementations and codecs. Matching
shape alone is not semantic membership, nor does schema equality prove codec honesty.

## Sessions And Producers

```rust,ignore
pub struct InputEvent { pub input: String, pub value: Value }
pub trait Producer: Send {
    fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError>;
    fn set_waker(&mut self, waker: WakeHandle) -> bool { false }
}
```

`Kernel::build(name, snapshots, Arc<dyn ProcessDispatcher>)` and
`build_with_options(name, snapshots, dispatcher, options)` do not activate producers.
`start(&mut self)` activates in dependency order; failures release all partial
producers and permit retry without reconstructing candidate artifacts.

- `inject(&self, input: &str, value: Value) -> Result<(), InjectionError>` admits exactly one value and returns its ownership on failure.
- `inject_key(&self, InputKey<T>, T)` pairs a public input name with its payload;
  local and encoded keys preserve their modes. Conversion precedes session access,
  and failure returns the constructed Value. Generation forwards the same method.
- `with_session<R>(&self, impl FnOnce(&mut NetworkSession) -> R) -> Result<R, String>` permits native requests, captures, completions and wakes only after start.
- `run_steps(&self, budget: usize) -> Result<RunReport, String>` executes bounded native steps through the actual dispatcher and worker execution guards.
- `drain(&self, max_polls: usize) -> Result<Vec<ProducerEvent>, DriverError>` only
  polls/admits events; it never runs process steps. A live blocking driver owns
  polling exclusively. `ProducerEvent.result` is `Result<(), ProducerError>`.
- `compiled()` exposes immutable native declarations; `export::<T>(&Key)` retrieves a candidate-owned artifact.

Each producer retains at most one pending event on `Full` without repolling.
`InputEvent::new(key, payload)` supplies the same typed conversion for producers;
it does not declare or grant a route, and retry reuses the retained envelope.
Round-robin position persists across calls. Terminal poll errors detach producers
and destroy them outside the producer mutex. Explicit session work does not hold
the producer mutex; a drain defers admission instead of waiting behind it.
The blocking `run_until` driver combines producer notification/shutdown with native
session wakes and bounded step execution. Portless processes need an explicit
`Edit::AddSource` or `NetworkBuilder::start`, never an implicit trigger.

The kernel combines exact snapshot leases with any application-provided lease,
then applies the same lease to native compiled declarations and the session.
Candidate-context compiled views are leased before callbacks can export or clone
them. Detached `compiled()` clones, exported compiled views, extracted factories,
spawned processes and independently started sessions retain those leases after
kernel/host teardown. Native queued packets, captured/cloned values, owned jobs and
completions retain their leases through payload/process destruction as well.

Escaping prepare, activation, producer-poll and host-binding callback errors own
their code/application lease, not merely their containing event or lifecycle error.
Moving a cause out of an error preserves the lease. The private wrapper delegates
formatting and exposes the original concrete cause through `Error::source`; walk
the source chain to downcast rather than assuming a fixed depth. The original
cause drops before its lease. Retaining an error intentionally delays plugin code
teardown even after its failed candidate and owned producers have been released.

## Host Publication

`KernelHost::prepare(stages, options, dispatcher, name, bind)` builds the exact
candidate inventory, invokes the nonadmitting binding hook, then activates producers
without polling. The hook receives only:

```rust,ignore
Binding::export::<T>(&Key) -> Option<&T>
Binding::inspect(&str) -> Result<ComponentSnapshot, NetworkError>
Binding::lookup_definition(DefinitionKey) -> Result<PhysicalDeclaration, NetworkError>
Binding::input_targets(&str) -> Option<&[(usize, String)]>
Binding::output_source(&str) -> Option<(usize, &str)>
```

There is no session or startable compiled-network getter on `Binding`.
Use opaque `DefinitionKey` identities, not factory/action pointer equality.
`publish(candidate)` rejects foreign/stale hosts and epochs, commits the plugin
batch, retires old owned producers and replaces the generation. Publication waits
only for producer drains, never old explicit session work. Old `Generation` handles
retain `inject`, `with_session`, `run_steps`, `compiled`, `with_kernel` and exports;
their retired producer drains remain empty. Candidate drop releases artifacts and
producers before their plugin leases. Native callback external effects are not
sandboxed or rolled back. Typed callback errors preserve their exact source.

Run `cargo test -p phenix-plugin -p phenix-kernel`. Storage integration targets also
require the independently migrated StorageGraph adapter.

## Host Wake Hints

`KernelHost::with_wake(plugins, wake.clone())` opts into producer/publication
notifications; `new` remains manual. Preparation builds, binds, and starts before
registering producer wakers, with shutdown checks on either side of registration.
It never polls or runs candidate process steps or installs a session notifier.
Unsupported/panicking registration and shutdown return `LifecycleError::Driver`.
Activation/subscription may perform I/O, and failed candidates may signal hints;
these external effects and wake versions are not rolled back. Shutdown checks
are not atomic publication revocation of an already prepared candidate.

`WakeHandle::version()` snapshots a wrapping change hint. Snapshot before work
inspection, then `wait_since(version)` waits for a different version or sticky
shutdown, looping through spurious wakes. `is_shutdown()` must also be checked
during busy iterations. Wakes notify all waiters before forwarding to the native
driver notifier, without holding the epoch mutex during forwarding. Versions
are neither event counts nor generation IDs. Publication signals after the new
active pointer is visible and the old drain gate is released, before old producer
destructors, including on first publication.

This is **source/publication-driven only**, not a general scheduler. An empty
bounded drain is not idle: finish a sweep using an application-known producer
bound, execute bounded steps, drain captures/backpressure, and retry after
progress or exhausted budgets. Graph process wakes, worker completion, deadlines,
explicit injection, and session-lease release do not automatically advance the
host version. Keep native `run_until` or coordinate those sources independently.
Never hold a host mutex while processing or waiting. Mixing host-reactor and
static-driver modes with different wake handles, including handover back to the
host after static registration, is unsupported: producer wakers are not restored.

`DerivedHost::with_wake` forwards this opt-in without changing mandatory storage
guards. See [the bounded design and reactor contract](../../docs/fbp-published-host-wake-20261115.md).
