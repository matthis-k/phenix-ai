# Versioned Snapshots And Cells

This API is synchronous and independent of Graph and Kernel. It does not issue
grants, provision resources, install observers, or add a batch-write API.

## Typed Cells

Implement `scoped::Stored` for a domain type, with a `Default` codec implementing
`Codec<Value = YourType>`. There is deliberately no canonical primitive codec.

```rust,ignore
use phenix_storage::scoped::CellSpec;
use phenix_storage::snapshot::{ReadConsistency, SnapshotReader};

const A: CellSpec<YourType> = CellSpec::project_state("demo.values/a");
const B: CellSpec<YourType> = CellSpec::project_state("demo.values/b");

// Host planning uses A.resource_spec() and B.resource_spec(). The host must
// provision/register datasets and bind distinct resource identities as usual.
let (a, b) = storage.bind((A.read(), B.read()))?;
let reader = SnapshotReader::new(
    vec![a.source(), b.source()],
    ReadConsistency::AtomicWithinMount,
)?;
let snapshot = reader.read()?;
let a_value = a.decode(&snapshot.entries()[0])?; // Revisioned<YourType>
let b_value = b.optional().decode(&snapshot.entries()[1])?; // Option<Revisioned<YourType>>
```

The exact public cell API in `phenix_storage::scoped` is:

- `CellSpec::<T>::new(scope: ScopeKind, class: StorageClass, name: &'static str) -> CellSpec<T>` is `const`.
- `CellSpec::<T>::project_state(name: &'static str) -> CellSpec<T>` is `const`.
- `CellSpec::<T>::read(self) -> CellRead<T>` is `const`.
- `CellSpec::<T>::resource_spec(&self) -> ResourceSpec` requires `T: Stored`.
- `Storage::bind<B: BindCells>(&self, cells: B) -> scoped::Result<B::Bound>` supports `CellRead<T>` and tuples of arity 1-4, returning correspondingly shaped `ReadCell<T>` handles.
- `ReadCell::<T>::source(&self) -> ReadSource` erases the type, not authority.
- `ReadCell::<T>::get(&self) -> storage::Result<Revisioned<T>>` uses an atomic singleton snapshot.
- `ReadCell::<T>::decode(&self, entry: &SnapshotEntry) -> storage::Result<Revisioned<T>>` rejects mismatching registry/address/key/schema and returns `MissingInput` on absence.
- `ReadCell::<T>::optional(&self) -> OptionalReadCell<T>` explicitly selects absence handling.
- `OptionalReadCell::<T>::source(&self) -> ReadSource`, `get(&self) -> storage::Result<Option<Revisioned<T>>>`, and `decode(&self, entry: &SnapshotEntry) -> storage::Result<Option<Revisioned<T>>>` preserve storage/codec errors.
- `ReadCell::<T>::changed(&self, capacity: usize) -> storage::Result<Subscription>` requires the original session's Subscribe grant.

Here `storage::Result` means `phenix_storage::Result`. `ReadCell<T>` and
`OptionalReadCell<T>` implement `Clone` without `T: Clone` or `T::Format: Clone`;
the codec is retained in an `Arc`. Send/Sync follow the codec's own bounds.
Binding checks Read authority and registered schema, resolves scope once, and
pins the session/address/schema/key. Every resource uses KV key `value`.
Distinct logical resources cannot alias datasets under the existing catalog
rules; the same resource intentionally refers to the same singleton.

## Erased Groups

The public API in `phenix_storage::snapshot` is:

```rust,ignore
pub struct ReadRequest<'a> {
    pub address: &'a Address,
    pub key: &'a str,
    pub schema: &'a Schema,
}
pub enum ReadConsistency { AtomicWithinMount, BestEffort }

Session::read_many(&self, requests: &[ReadRequest<'_>], consistency: ReadConsistency)
    -> phenix_storage::Result<Snapshot>;
SnapshotReader::new(sources: Vec<ReadSource>, consistency: ReadConsistency)
    -> phenix_storage::Result<SnapshotReader>;
SnapshotReader::read(&self) -> phenix_storage::Result<Snapshot>;
Snapshot::entries(&self) -> &[SnapshotEntry];
Snapshot::consistency(&self) -> ReadConsistency;
SnapshotEntry::address(&self) -> &Address;
SnapshotEntry::key(&self) -> &str;
SnapshotEntry::value(&self) -> Option<&Revisioned<Vec<u8>>>;
ReadSource::address(&self) -> &Address;
ReadSource::key(&self) -> &str;
ReadSource::schema(&self) -> &Schema;
ReadSource::changed(&self, capacity: usize) -> phenix_storage::Result<Subscription>;
```

`ReadSource` and `SnapshotEntry` are clonable with private fields. Sources can
come from independently bound cells, including different sessions on the same
registry/mount. Each source retains and checks its own session; grants are never
unioned. The source exposes no Session, Registry, or grant-issuing authority.
`changed` requires Subscribe; Read alone cannot install an observer.

All Read grants across the group are checked before any lookup. Atomic mixed
mounts or registries fail with `MixedSnapshotSources` before backend IO. Every
schema is checked before any backend read or capability probe. Construction then
probes each participating mount once for TypedKv and, in atomic mode, atomic
batch support, without reading values; `read()` repeats these checks. BestEffort
does not require CAS support; unsupported versioned fallback reads still fail at
read time. Sessions have immutable grants
and there is currently no revocation API.

Empty batches succeed with zero entries and no IO. Entries always follow input
order, including duplicates and absent keys. Decoding happens after all backend
locks/transactions have ended. Reads and failures publish no notices. Same-mount
transaction reentry returns `Reentrant` rather than blocking.

## Backend Contract

`TypedKv` adds these object-safe, default-unsupported methods:

```rust,ignore
pub struct ReadKey<'a> { pub dataset: &'a str, pub key: &'a str }
fn supports_atomic_read_many(&self) -> bool; // default false
fn read_many(&mut self, keys: &[ReadKey<'_>])
    -> phenix_storage::Result<Vec<Option<Revisioned<Vec<u8>>>>>;
```

An opt-in backend must return a true coherent versioned snapshot in exactly the
requested order and length. Memory reads run under one mount lock. SQLite reads
use one deferred read transaction and its own snapshot, including isolation from
other connections. Existing revision tokens and SQL authorizer policy are reused.
No Backend methods or plugin forwarding changes are needed.

`AtomicWithinMount` requires this capability and one registry/mount.
`BestEffort` explicitly permits mixed sources; each mount uses atomic reads when
available, otherwise sequential `get_revisioned`. The returned consistency stays
`BestEffort` even if a particular read happened to be coherent. There is no
cross-mount atomicity and no assumed external-backend atomic capability.

Absence is `None`, not a revision or a durable negative-cache token. Consumers
must revalidate missing inputs rather than memoize absence as unchanged history.
Present tokens remain scoped to registry/mount/dataset/key; they are not global
snapshot IDs. No memoization or invalidation coverage beyond existing Storage
guarantees is introduced here.
