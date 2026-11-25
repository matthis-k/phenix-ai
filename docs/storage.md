# Storage

`packages/phenix-storage` is a synchronous host-owned storage library. It has no
graph, kernel, async runtime, SQLx, serde, JSON, or Rust `TypeId` dependency.
SQLite uses the maintained `rusqlite` library with bundled SQLite and authorizer
hooks. It is a root workspace member, with dependencies recorded in Cargo.lock.

See [Scoped storage](scoped-storage.md) for user/project/session resource
identity, XDG policy, persistent SQLite KV, and resource/backend plugin offers.

## Ownership

- The trusted host owns a `Registry`: mount provisioning, SQLite filesystem paths,
  dataset registration, and grant issuance are trusted host operations.
- A `Session` holds immutable explicit grants and a shared registry reference.
  Give plugins sessions, never the registry. Grant declarations alone are not
  authority; only the trusted registry creates operational sessions.
- Each mount owns a boxed object-safe `Backend` and its own mutex. A short
  registry lock resolves mount identity; no global lock is held throughout
  unrelated SQLite transaction callbacks. An optional `phenix-storage-plugin`
  bridge can install backend contributions while pinning their plugin lease.
- `Codec` is a typed application boundary with an associated value type. Its
  schema contains authoritative name, revision, structure, and codec identity.
  All four fields must match registered metadata before memory encoding or
  decoding. Structure is an application-defined canonical descriptor, not
  inferred Rust layout. The host and codec author are responsible for truthful
  descriptors and codec behavior. There is no automatic migration.
- SQL stays native: `Sql` borrows the transaction connection privately, accepts
  `&[&dyn ToSql]` parameters, and maps rows through a facade exposing `FromSql`.
  Neither the connection nor raw rusqlite rows escape the callback.
- A `Subscription` owns its queue and weakly references registry state. The registry
  keeps only weak watch registrations. Dropping a subscription releases its queue
  without locking the registry, including inside SQL callbacks. Dead registrations
  are pruned on the next subscribe or emit. It never keeps connections alive.

## Addresses And Authority

Only `store://mount/dataset` is accepted. Components use lowercase ASCII letters,
digits, hyphens, and underscores, with at least one character each. These strings
are already canonical: no case folding, percent decoding, URL parsing, path
normalization, or Unicode normalization occurs. Dots, traversal, percent escapes,
extra slashes, query/fragment suffixes, credentials, ports, backslashes, whitespace,
and non-ASCII text are rejected. Addresses never become filesystem paths or SQL
identifiers. Memory keys are opaque strings, not address components.

Mounts and datasets cannot be replaced, and duplicate registration is an error.
Memory metadata is registry-local; SQLite persists authoritative dataset
schemas in `__phenix_schema` and rejects incompatible re-registration after
restart. The host must still register datasets during startup. A logical
dataset declaration does not create an application SQL table.

Grants pair an exact dataset or mount scope with explicit operations: `Read`,
`Write`, `Delete`, `SqlRead`, `SqlWrite`, `SqlAdmin`, the legacy
`Sql` alias for administrator authority, and `Subscribe`. KV operations and
subscription are independent; SQL transactions interpret administrator as
read/write/admin and SQL write as read/write. Exact operation capability probes
still require the requested grant. There is no prefix matching. Authorization
precedes resource lookup. `capabilities(address, operation)` and
`capabilities_mount(name, operation)` report backend features, not additional
permissions. Memory supports typed KV; SQLite supports typed KV through its
reserved `__phenix_kv` table **and** native SQL; both support bounded invalidation.

**SQL requires mount-scoped authority**, through `SqlRead`, `SqlWrite`,
`SqlAdmin`, or legacy `Sql` (administrator). Arbitrary SQL crosses logical
dataset boundaries through joins, DDL, triggers and table names. Dataset-scoped
SQL grants do not authorize transactions. Read-only SQL authority cannot
execute mutations; native SQL remains no general hostile-SQL/process sandbox.
The case-insensitive `__phenix_` namespace is reserved: callback SQL cannot read,
write, create or drop its tables, indexes, triggers or views, even with administrator
authority. This includes typed KV, schema and revision-clock metadata. Reads of the
`sql` column in SQLite schema catalogs are denied, as is **all `ALTER TABLE`**, even
for application tables: SQLite's authorizer omits rename destinations. Nested/temp
triggers do not bypass these restrictions. Shared application SQL tables remain
mount-wide; the scoped host planner requires explicit SQL mount-sharing approval.
Callback transaction controls, savepoints, attachment and detachment are denied by
SQLite's authorizer. All PRAGMAs, including read-only PRAGMAs, and functions whose
names start with `pragma_` (case-insensitive) are denied too. Internal transaction
control remains available to the adapter for begin, commit and rollback. Foreign
keys are enabled by the host-side mount operation, not by callback SQL. SQLite
functions/extensions are not exposed for registration. Mount authority still
includes SQL-supported data and schema changes across that connection, but cannot
override these host connection policies. Only trusted SQL code should receive it.

Errors distinguish invalid addresses, duplicates, missing resources, denial,
unsupported capabilities, closed subscriptions, schema mismatch, codec errors,
invalid queue capacity, ended transactions, poisoned state, and underlying
structured rusqlite errors. `UnsupportedCapability { capability }` identifies
`TypedKv`, `RevisionedRead`, `AtomicReadMany`, `Sql` or `CompareExchange`;
unsupported operations do not silently emulate another backend.
An empty successful drain means no queued notices; `SubscriptionClosed` means the
registry state is gone (or the watch is absent), not an empty queue. Sessions keep
state alive even after the original registry handle is dropped.

`Capabilities` reports `typed_kv`, `revisioned_read`, `atomic_read_many`,
`compare_exchange`, `sql`, `invalidation`, and `observation`. Revisioned reads,
atomic snapshots and conditional writes are independent optional promises; plain
KV reads require none of those stronger capabilities.
The closed `Observation` metadata uses `RegistryWrites` as its source,
`Key` (memory) or `Mount` (SQLite) as coverage, and
`BoundedNonDurableWithOverflow` as delivery. Every `Invalidation` carries this same
metadata along with sequence, scope and optional key. These are explicit closed
outcomes and guarantees, not an extensible backend negotiation protocol.

## Host And Plugin Integration

The host provisions resources and passes restricted sessions to plugin-owned
synchronous node handlers. Storage is not a required kernel runtime dependency:
`packages/phenix-kernel/Cargo.toml` lists it only under dev-dependencies for the
example and tests. Application codecs can use any binary representation; this example
uses UTF-8 explicitly rather than an implicit JSON fallback:

```rust
use phenix_storage::*;

struct Text;
impl Codec for Text {
    type Value = String;
    fn schema(&self) -> Schema {
        Schema {
            name: "example.title".into(), revision: 1,
            structure: "utf8-string".into(), codec: "utf8".into(),
        }
    }
    fn encode(&self, value: &String) -> Result<Vec<u8>> {
        Ok(value.as_bytes().to_vec())
    }
    fn decode(&self, bytes: &[u8]) -> Result<String> {
        String::from_utf8(bytes.to_vec()).map_err(|e| Error::Codec(e.to_string()))
    }
}

fn provision() -> Result<()> {
    let registry = Registry::new();
    registry.mount_memory("local")?;
    let address = Address::parse("store://local/titles")?;
    registry.register(&address, Text.schema())?;
    let node = registry.session([Grant::new(
        Scope::Dataset(address.clone()),
        [Operation::Read, Operation::Write, Operation::Subscribe],
    )]);
    let mut changes = node.subscribe(Scope::Dataset(address.clone()), 32)?;
    node.put(&address, "document-1", &"Title".to_owned(), &Text)?;
    let title: Option<String> = node.get(&address, "document-1", &Text)?;
    for notice in changes.drain()? {
        // The plugin binding decides which graph input/cache to invalidate.
        // Overflow requires a complete rescan of the subscribed scope.
        println!("{notice:?}");
    }
    assert_eq!(title.as_deref(), Some("Title"));
    Ok(())
}
```

Native SQLite usage, after trusted `mount_sqlite("db", path)`; native SQL needs no
dataset registration:

```rust
# use phenix_storage::*;
fn sql_work(registry: &Registry) -> Result<Vec<(i64, String)>> {
    let node = registry.session([Grant::new(
        Scope::Mount("db".into()), [Operation::SqlAdmin],
    )]);
    node.transaction_mount("db", |sql| {
        sql.execute("CREATE TABLE IF NOT EXISTS items(id INTEGER PRIMARY KEY, title TEXT NOT NULL)", &[])?;
        sql.execute("INSERT INTO items(title) VALUES (?)", &[&"bound, not interpolated"])?;
        sql.query("SELECT id, title FROM items WHERE id > ?", &[&0i64], |row| {
            Ok((row.get::<i64>(0)?, row.get::<String>(1)?))
        })
    })
}
```

Returning `Ok` commits; returning `Err` rolls back. A callback panic is caught long
enough to drop/roll back the SQLite transaction and release the mount guard,
then resumed outside the lock. It does not poison registry state or disable other
mounts. This is unwind handling, not recovery from process abort. Commit failures return errors
without notifications. Foreign keys are enabled when mounting. The authorizer
prevents callback SQL from manually committing or rolling back. If SQLite itself
ends a transaction (for example, `INSERT OR ROLLBACK`), further facade calls fail
with `TransactionEnded` rather than silently running in autocommit mode. A query
requires a read-only prepared statement; use execute for writes, not DML RETURNING.
Keep each call to one SQL statement. Parameters bind values, never identifiers.

The registry removes its callback SQL authorizer before returning the connection
to a trusted external `SqlBackend`, including error, failed begin/commit, implicit
rollback and unwind paths. Built-in SQLite adapter operations install their own
authorizer denying derived trigger/view execution: persisted triggers cannot gain
internal KV, registration or migration authority.

## Revisions

Low-level `Session::get` still returns `Option<C::Value>`. `get_revisioned` returns
`Option<Revisioned<C::Value>>`; `compare_exchange(address, key,
expected: Option<&Revision>, value, codec)` returns a new opaque `Revision`.
`None` means insert only if absent; conflicts return
`Error::Conflict { current: Option<Revision> }`. Reads require `Read` and conditional
writes require `Write`; capabilities never add authority. Unsupported providers
return `Capability::CompareExchange`, not a get/put approximation.

Memory and SQLite support atomic CAS. Successful puts invalidate prior tokens,
including delete/recreate. SQLite persists revisions and its clock, serializes CAS
with an immediate transaction, and transactionally upgrades
legacy KV metadata. Already-versioned files with missing/regressed clocks fail
closed rather than risk ABA through token reuse. Tokens are comparable only for
the same mount/dataset/key, not global sequence numbers, snapshot tokens or grants.
Absence has no historical token; restoring an old database restores its tokens.
The public CAS guarantee is local to one backend/mount, not a transaction across
backend instances or mounts.
See [Scoped storage](scoped-storage.md#typed-access-and-cas) for plain and explicitly
revisioned scoped reads and independent declared capability requirements. Scoped
and storage-plugin errors retain wrapped storage/filesystem causes through
`Error::source`; unsupported behavior remains distinct from absence or denial.

## Graph Binding

The runnable [graph binding example](../packages/phenix-kernel/examples/storage.rs)
keeps business behavior in `CounterPlugin`: its graph module writes and then reads
through a restricted `Session`. `ObserverPlugin` activates a `StorageProducer`
which drains storage notices and maps them to the `storage_changed` trigger. The
producer preserves a drained batch's tail across bounded kernel polls. The kernel
only composes contributions and schedules generic triggers/producers; it does not
own counter logic, interpret storage metadata, or require storage in production.
The example logs notices, including overflow; a real plugin binding must implement
its own rescan/cache invalidation policy rather than treating that log as recovery.

## Invalidation Contract

The per-mount mutation lock covers successful mutation/commit through publication;
the registry mutex covers sequence assignment and queue insertion. Consumers cannot
observe an event before its write completes. Failed CAS publishes no notice.
Sequence numbers order this registry's events, not external database activity.
Memory put emits dataset/key metadata; delete emits only when a key existed.
SQLite emits one mount-wide event after a successful potentially writing
transaction, including DDL. Read-only transactions emit none. A failed statement
whose error the callback catches can conservatively cause invalidation if the
outer transaction later commits. No row-count/table inference claims precision.
SQL mount events also reach dataset subscribers on that mount, but carry no row
values, table names, or SQL text. Subscribers on other mounts are unaffected.

Each subscription has a positive queue capacity. When full, queued detail is
discarded and replaced by an explicit `Overflow { through }` watermark. Further
matching events advance that watermark until drained. The next drain returns only
the overflow notice, after which detailed delivery resumes. This is lossy bounded
invalidation, not an event log, replay mechanism, or live-query engine. Allocation
per queue is bounded by event count, not bytes; keys and codec payloads are not
size-limited. A slow consumer cannot block writers on channel sends.

Coverage is **best effort and adapter-local**: no observation of another registry,
another connection/process writing the same file, external file replacement, or
changes outside these methods. There is no durable notification/outbox guarantee;
a crash after database commit can lose notification. Plugin graph binding is an
explicit integration task: poll/drain at synchronous scheduling boundaries, map
notices to dependent graph inputs, and rescan on overflow. No callbacks execute
from the notification path.

## Limits

Each mount has its own mutex. Recursive same-mount entry returns
`Error::Reentrant` on the same thread. Nested cross-mount transactions can
still deadlock if application code locks mounts in inconsistent order; avoid
calling independent storage operations from SQL callbacks.
Codec `schema`, `encode`, and `decode` callbacks all run outside the lock. Schema
equality is checked under the lock before encoding/decoding, and put rechecks it
under the mutation lock after encoding. SQLite `query` eagerly collects; `query_each` and `returning` can visit
rows incrementally. There is no async API, cross-mount transaction, memory batch
transaction, native shared-library ABI, grant revocation, automated schema
migration, snapshot/subscription handshake, live-query tracking, or general
poison recovery. Scoped resources, XDG path policy and a graph observer binding
are provided as separate modules/crates. SQL remains SQLite-specific. `Error::Poisoned` still represents
an actually poisoned mutex; ordinary SQL callback unwinding does not cause it.
Subscription drop releases its queue even if the registry mutex was poisoned.
Drain does not wait for an event; it may briefly contend with queue publication.
Registry registration and grant issuance
are not concurrency-restricted after startup, so the host should own their policy.

The bundled backends are memory and SQLite, but external typed-KV backends
can implement the object-safe `Backend` interface and be contributed through
`phenix-storage-plugin`. Native `SqlBackend` still exposes rusqlite and is
not PostgreSQL-neutral. Canonical structural schema support exists in
`phenix-schema`; a hand-written codec may still falsely claim compatible bytes.
No arbitrary codec is proven correct by the descriptor alone. SQL registration
metadata does not type-check SQL rows.
