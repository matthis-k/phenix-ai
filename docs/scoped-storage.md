# Scoped Storage

Implemented in `phenix_storage::scoped` and the optional
`phenix-storage-plugin` host bridge. See [Storage](storage.md) for the low-level
contract and [Review](scoped-storage-review.md) for the exact `eeb39b64` baseline
comparison, architecture assessment, review fixes and verification status.
The [bridge README](../packages/phenix-storage-plugin/README.md) describes the
same public API in more detail.

## Identity And Sharing

`ResourceId { scope, class, name }` identifies a logical resource. `ScopeId` is
a host-issued user, project or session ID; `StorageClass` is config, data, state,
cache or runtime; names use a validated namespace/path such as
`phenix.agent/settings`. Backend, filesystem path, working directory, Git remote,
Rust type and requester identity are not part of resource identity.

`ResourceSpec { scope: ScopeKind, class, name, kind }` is an unresolved declaration.
`Identity` resolves its scope explicitly. Missing project/session identities fail;
there is no user fallback or scope inheritance. Project IDs must survive workspace
moves; clone discovery and config precedence remain host/domain policy.

`ResourceKind::TypedKv(Schema)` binds a dataset; `ResourceKind::Sql` binds a whole
SQL mount without a placeholder KV dataset. `ResourceCatalog` rejects duplicate
logical bindings and physical dataset aliases. Sharing a dataset means using the
same logical ID, not two aliases. `bind_target_with_sql_sharing` is a host-only
opt-in for SQL mount aliases. Frozen `Bindings` are immutable lookup data, not
authority; each `Storage` combines bindings, identity and an independently granted
`Session`.

## Declare, Prepare, Activate

Plugins contribute through the existing registrar, not a second loader:

```rust
use phenix_storage::{Codec, Operation};
use phenix_storage::scoped::{ScopeKind, StorageClass};
use phenix_storage_plugin::ResourceOffer;

// AgentConfigCodec is the application's explicit Codec.
registrar.add(ResourceOffer::kv_schema(
    "settings", ScopeKind::Project, StorageClass::Config,
    "phenix.agent/settings", AgentConfigCodec::default().schema(),
    [Operation::Read],
))?;
```

`ResourceOffer::kv::<T>` uses `T: Stored`; `kv_schema` requires only a shared wire
descriptor, not a shared Rust type. `ResourceOffer::sql` declares SQL operations.
`ResourceOffer::new(id, ResourceRequest::new(spec, operations)
.require(Capability::CompareExchange))` declares conditional-write support,
requiring `Write`. `Capability::RevisionedRead` requires `Read`, independently
of conditional writes; `Capability::AtomicReadMany` requires `Read` and is
implied by `CellSpec::request(operations)` for coherent derived snapshots.
KV accepts read/write/delete/subscribe; SQL accepts its native operations
and subscription. A plain scoped `Kv::get` needs only a KV Read grant.
Requested capabilities are checked against declared provider features at
prepare, and against actual opened backends before activation publishes views.

Host composition is explicit and default-deny:

```rust,no_run
use phenix_plugin::PluginSnapshot;
use phenix_storage::Operation;
use phenix_storage::scoped::{Identity, ResourceId, ScopeId, StorageClass, XdgPaths};
use phenix_storage_plugin::{ActivatedStorage, Error, PreparedStorage,
    ResourceRequirements, StoragePolicy};

fn activate(snapshots: Vec<PluginSnapshot>) -> Result<ActivatedStorage, Error> {
    let identity = Identity::user("alice")?.project("stable-project-id")?;
    let requirements = ResourceRequirements::collect(snapshots, &identity)?;
    let mut policy = StoragePolicy::xdg(&requirements, &XdgPaths::from_env()?)?;
    let settings = ResourceId::new(
        ScopeId::Project("stable-project-id".into()), StorageClass::Config,
        "phenix.agent/settings",
    )?;
    // Fixed host approvals, not copied or inferred from plugin requests.
    policy.resources.get_mut(&settings).ok_or(Error::MissingPlacement)?
        .allowed.insert(Operation::Read);
    policy.plugin_access.insert("settings-reader".into(),
        [(settings, [Operation::Read].into())].into());
    PreparedStorage::prepare(requirements, identity, policy)?.activate()
}
```

1. `ResourceRequirements::collect(snapshots, &identity)` copies declarations,
   merges compatible kinds/schemas and preserves per-plugin requests. Conflicts,
   duplicate plugin names and invalid operation/kind combinations fail. Copied
   declarations retain no requester code/instance leases.
2. The host chooses placement and approves operations independently in both
   `ResourceBinding::allowed` and `StoragePolicy::plugin_access[plugin][resource]`.
   Missing or insufficient approvals deny the entire plan. Never turn requests
   into authority by copying them into either allowlist. XDG selects placement
   only and leaves both allowlists empty.
3. `PreparedStorage::prepare` validates identity, placement, aliases, authority,
   declared capabilities and sharing without opening provider factories, creating
   directories, registering datasets or issuing sessions. It does inspect existing
   paths and, after authorization, opens existing built-in SQLite files read-only
   once per mount through `Registry::validate_sqlite_datasets`.
4. `activate` consumes the plan, constructs a private registry, opens mounts,
   probes providers, registers KV schemas and only then returns per-plugin
   `ActivatedStorage.views`. Issued grants contain requested operations, not the
   full host allowance; views contain only that plugin's bindings.

The public plan is named `PreparedStorage`, the semantic equivalent of a
`StoragePlan`, not a second public type. Inspect `requirements()`, `bindings()`,
`plugin_access()`, `shared_sql_mounts()`, `mounts()`, `registrations()` and
`xdg_paths()` without executing it. `PreparedBackend::capabilities()` reports
declared features; registrations are derived KV schemas, never invented SQL metadata.

Activation failure publishes no views and drops opened backends and leases.
It does **not** roll back filesystem or durable database effects: created
directories, files and registration metadata remain. Existing data is never deleted
as cleanup. The host publishes successful replacement views; old sessions remain
usable until dropped. There is no revocation or cross-database transaction.

## Typed Access And CAS

`Stored` associates a value with `type Format: Codec<Value = Self> + Default`.
It adds no serialization format or `TypeId` coupling. Independent codecs must
truthfully agree on all schema fields and actual bytes.

```rust
// storage: scoped::Storage; AgentConfig: Stored
let config = storage.project()?.config::<AgentConfig>("phenix.agent/settings")?;
let current = config.get_revisioned("default")?; // Option<Revisioned<AgentConfig>>
let expected = current.as_ref().map(|stored| &stored.revision);
let revision = config.compare_exchange("default", expected, &replacement)?;
```

`Kv::get` returns `Result<Option<T>>` without requiring revision support.
`Kv::get_revisioned` returns `Result<Option<Revisioned<T>>>`; use `.value` and
retain `.revision`. `compare_exchange(key, expected: Option<&Revision>, value)` returns
the new `Revision`; `None` inserts only if absent. Conflict returns
`Error::Conflict { current: Option<Revision> }`, not a stored value. CAS is atomic
in the backend, never get-then-put. `get` needs `Read`; CAS and `put` need `Write`;
`delete` needs `Delete`; `changed(capacity)` needs `Subscribe` independently.

Tokens are opaque backend bytes, comparable only for the same mount/dataset/key.
They are not counters for application arithmetic, grants, database snapshots or
global order. Successful puts invalidate old tokens, including delete/recreate;
absence has no token/history guarantee. Restoring an older database restores its
tokens. Memory revisions last for that backend lifetime. SQLite persists the clock
and performs CAS under an immediate SQLite transaction. The public guarantee is
within one backend/mount, not a transaction across backend instances or mounts. Legacy KV
tables gain a revision column transactionally; missing/regressed clocks in already
versioned databases fail closed rather than reconstructing from live rows and
reusing deleted-row tokens. This adapter migration is not application schema migration.

## SQL And Persistence

`view.project()?.sql(class, name)?.transaction(...)` uses
`Session::transaction_mount`, not a synthetic dataset. `SqlResource::changed`
subscribes to mount-wide invalidation with a separate `Subscribe` grant.

SQL is mount-wide, not narrowed by logical resource names. Sharing a SQL mount
across multiple resources **or plugins**, including colocation with KV, requires
an explicit `StoragePolicy::shared_sql_mounts` entry. Shared application SQL tables
are visible according to SQL read/write/admin authority. Separate databases remain
the stronger isolation boundary.

Even SQL administrators cannot access the case-insensitive `__phenix_` reserved
namespace, including KV, schema and revision-clock tables; schema SQL-text reads
and all callback `ALTER TABLE` are denied. See [SQL restrictions](storage.md#addresses-and-authority).
Adapter operations deny derived trigger/view execution, so persisted triggers cannot
borrow internal KV/migration authority. External file writers remain outside this policy.

SQLite persists authoritative descriptors in `__phenix_schema`. Planner preflight
checks all four schema fields, accepts missing files/rows and legacy missing
metadata without migration, and rejects a metadata view. It is advisory: it does
not validate stored payloads or revision-clock health. Activation registration
rechecks schemas, including changes after preparation. Bindings are host
configuration, not implicitly durable; application migrations remain explicit.

## XDG Placement

`XdgPaths::from_env` uses absolute XDG overrides or HOME defaults for config,
data, state and cache; relative overrides are ignored. Runtime is optional.
`directory(&resource)` is pure path policy under
`<class-base>/phenix/{users,projects,sessions}/<stable-id>`.
`StoragePolicy::xdg` adds class and injectively hex-encoded resource name to
select an isolated SQLite file/mount per resource. Paths and inode aliases across
separate mounts are rejected. Names never become unvalidated filesystem paths.

On Unix, activation creates app-private directories including `phenix` as `0700`
and databases as `0600`. Existing components must already have exact modes and
effective-uid ownership; unsafe data is rejected, not chmod-repaired. Database and
journal/WAL/SHM files must be regular, single-link files; symlinks are rejected.
File checks/precreation use `O_NOFOLLOW`; new SQLite sidecars inherit database
permissions (rollback-journal behavior is tested). Existing XDG base directories
may be `0755` and are not changed. The process umask is not changed; an excessively
restrictive umask may fail closed.

Private paths are checked during prepare, before activation opens any backend,
while provisioning and after SQLite opens. Provenance is retained for all classes.
Overrides must remain in the appropriate private subtree. Runtime additionally
requires an existing non-symlink directory owned by `geteuid()` with exact `0700`,
checked at policy construction and activation. Non-Unix private XDG SQLite/runtime
placement is rejected. These checks are not a hostile filesystem sandbox; hosts
must protect selected bases against concurrent path replacement.

Manual `StoragePolicy::default()` placement leaves path security and modes to the
host. The runnable scoped example deliberately uses manual policy with an XDG-derived
path and explicit SQL/KV colocation; it does not demonstrate private XDG provisioning.

## Providers And Graphs

`BackendOffer::new(id, factory).with_features(BackendFeatures { ... })` declares
a provider for planning. `mount_offer` remains the low-level host installation API
and does not require that declaration. Preparation pins the exact provider offer;
opened `LeasedBackend` drops backend state before releasing its lease. Provider
replacement/unload does not replace live backends. Requester declarations need no
equivalent lease after copying.

Provider feature declarations are trusted contracts. Activation probes typed KV,
SQL and `supports_compare_exchange` for requested capabilities, but cannot prove
atomicity, durable token uniqueness, truthful observation or persisted schema
compatibility before opening a provider. Durable providers must implement
`Backend::register_dataset`; the default no-op is only sufficient for memory.
`SqlBackend` lends rusqlite, so it is SQLite-specific, not PostgreSQL-neutral.

The [scoped example](../packages/phenix-kernel/examples/scoped_storage.rs) loads
writer, observer and SQL code-index plugins into **one PluginSet**, collects and
later composes the same snapshots, activates host-approved storage, then injects
each view exactly once through `Arc<OnceLock<Arc<Storage>>>`. Deferred graph
composition reads the injected views; there is no reload/second contribution pass.
The observer uses `StorageBinding` for bounded invalidation waves. The kernel owns
neither domain logic nor storage policy. Overflow still requires consumer rescan.

Blob/search adapters, secrets storage, durable observation and snapshot/subscription
handoff are not implemented. Storage classes do not imply these capabilities.
