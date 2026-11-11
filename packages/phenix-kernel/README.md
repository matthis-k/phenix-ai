# Native Network Kernel

The kernel resolves plugin contributions, builds host-authorized native network
patches, and owns a persistent `Mutex<NetworkSession>`. It adds no scheduler or
legacy execution adapter. Native Rust plugins are not a sandbox or a dynamic ABI.

## Contribution Schema

```rust,ignore
pub struct Contribution {
    pub id: String,
    pub requires: Vec<Key>,
    pub routes: Vec<String>, // Public inputs this producer may inject.
    pub patch: NetworkPatch,
    pub prepare: Option<Box<Prepare>>,
}
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
and `composition: BTreeMap<Key, CompositionPolicy>`. Each `CompositionPolicy` has
`grants: BTreeSet<Footprint>` and `priority: i64`. Missing policy grants no writes.
Grants and priorities are host decisions, never fields in plugin patch payloads.
The build's dispatcher argument overrides `execution.dispatcher`; a snapshot code
lease is added without discarding an application-provided lease.

Side-effect-only processes declare no outputs. Intentionally unconnected emissions
require explicit `execution.discard_outputs` entries naming the physical process
path and output; the kernel does not silently grant discard permission.

## Contracts

- `ContractDefinition { id, contract: phenix_graph::contract::Contract }`.
- `NetworkOffer { id, producer: Key, unit: String, contract: Key }`; `unit` is the actual component path and `component_path()` returns it.
- `ContractRequirement { id, consumer: Key, contract: Key }` declares an import local to one contribution.
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
- `with_session<R>(&self, impl FnOnce(&mut NetworkSession) -> R) -> Result<R, String>` permits native requests, captures, completions and wakes only after start.
- `run_steps(&self, budget: usize) -> Result<RunReport, String>` executes bounded native steps through the actual dispatcher and worker execution guards.
- `drain(&self, max_polls: usize) -> Vec<ProducerEvent>` only polls/admit events; it never runs process code. `ProducerEvent.result` is `Result<(), ProducerError>`.
- `compiled()` exposes immutable native declarations; `export::<T>(&Key)` retrieves a candidate-owned artifact.

Each producer retains at most one pending event on `Full` without repolling.
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
