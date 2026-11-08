# Storage

`packages/phenix-storage` is a synchronous host-owned storage library. It has no
graph, kernel, async runtime, SQLx, serde, JSON, or Rust `TypeId` dependency.
SQLite uses the maintained `rusqlite` library with bundled SQLite and authorizer
hooks. It is a root workspace member, with dependencies recorded in Cargo.lock.

## Ownership

- The trusted host owns a `Registry`: mount provisioning, SQLite filesystem paths,
  dataset registration, and grant issuance are trusted host operations.
- A `Session` holds immutable explicit grants and a shared registry reference.
  Give plugins sessions, never the registry. Grant declarations alone are not
  authority; only the trusted registry creates operational sessions.
- Each mount owns either an in-memory byte map or a SQLite connection. The
  private backend enum is the primitive boundary. No extensible object-safe
  backend trait is needed for these two concrete implementations; adding one
  now would expose authorization/invalidation invariants unnecessarily.
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
Metadata is registry-local, including for persistent SQLite files; the host must
register datasets again on restart. A dataset does not create a SQL table.

Grants pair an exact dataset or mount scope with explicit operations: `Read`,
`Write`, `Delete`, `Sql`, and `Subscribe`. There is no implicit hierarchy among
operations and no prefix matching. Authorization precedes resource lookup.
`capabilities(address, operation)` requires the specified grant and reports the
backend's features, not additional permissions. Memory supports typed KV; SQLite
supports native SQL; both support invalidation.

**SQL requires a mount-scoped `Sql` grant**, even when called using a particular
dataset address. Arbitrary SQL crosses logical dataset boundaries through joins,
DDL, triggers, and table names. A dataset-scoped SQL grant is deliberately useless.
SQL authority is not a read-only grant or a general hostile-SQL/process sandbox.
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
`TypedKv` or `Sql`; unsupported operations do not silently emulate another backend.
An empty successful drain means no queued notices; `SubscriptionClosed` means the
registry state is gone (or the watch is absent), not an empty queue. Sessions keep
state alive even after the original registry handle is dropped.

`Capabilities` reports `typed_kv`, `sql`, `invalidation`, and `observation`.
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

Native SQLite usage, after trusted `mount_sqlite("db", path)` and registration of
`store://db/items` with application metadata:

```rust
# use phenix_storage::*;
fn sql_work(registry: &Registry, address: &Address) -> Result<Vec<(i64, String)>> {
    let node = registry.session([Grant::new(
        Scope::Mount("db".into()), [Operation::Sql],
    )]);
    node.transaction(address, |sql| {
        sql.execute("CREATE TABLE IF NOT EXISTS items(id INTEGER PRIMARY KEY, title TEXT NOT NULL)", &[])?;
        sql.execute("INSERT INTO items(title) VALUES (?)", &[&"bound, not interpolated"])?;
        sql.query("SELECT id, title FROM items WHERE id > ?", &[&0i64], |row| {
            Ok((row.get::<i64>(0)?, row.get::<String>(1)?))
        })
    })
}
```

Returning `Ok` commits; returning `Err` rolls back. A callback panic is caught long
enough to drop/roll back the SQLite transaction and release the registry guard,
then resumed outside the lock. It does not poison registry state or disable other
mounts. This is unwind handling, not recovery from process abort. Commit failures return errors
without notifications. Foreign keys are enabled when mounting. The authorizer
prevents callback SQL from manually committing or rolling back. If SQLite itself
ends a transaction (for example, `INSERT OR ROLLBACK`), further facade calls fail
with `TransactionEnded` rather than silently running in autocommit mode. A query
requires a read-only prepared statement; use execute for writes, not DML RETURNING.
Keep each call to one SQL statement. Parameters bind values, never identifiers.

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

Mutation, successful commit, sequence assignment, and queue insertion occur under
one registry mutex. Consumers cannot observe an event before its write completes.
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

## Limits And Verification

One mutex serializes all mounts and SQL callbacks. Do not reenter the same registry
from a SQL closure, SQL parameter conversion, or row decoder: that deadlocks.
Codec `schema`, `encode`, and `decode` callbacks all run outside the lock. Schema
equality is checked under the lock before encoding/decoding, and put rechecks it
under the mutation lock after encoding. SQLite query results
are eagerly collected. There is no async API, streaming cursor, cross-mount
transaction, memory batch transaction, SQL typed-KV emulation, backend plugin ABI,
read-only SQL permission, grant revocation, schema migration, persistence of the
registry metadata, snapshot/subscription handshake, live-query tracking, automatic
graph integration, or general poison recovery. `Error::Poisoned` still represents
an actually poisoned mutex; ordinary SQL callback unwinding does not cause it.
Subscription drop releases its queue even if the registry mutex was poisoned.
Drain does not wait for an event; it may briefly contend with queue publication.
Registry registration and grant issuance
are not concurrency-restricted after startup, so the host should own their policy.

Tests cover strict parsing, duplicates, missing resources, authority isolation,
each schema field, memory KV, queue ordering/overflow/scope, concurrent writes,
native SQLite types and parameterization, SQL authority, transaction-control
denial, rollback, deferred commit failure, implicit rollback, and panic behavior.
Tests also cover observation metadata, closed subscriptions, PRAGMA and pragma
function denial, VACUUM/ATTACH file escape attempts, codec failures and structured
SQL errors. The kernel integration tests exercise both backends through real
plugin contributions, separate bounded notification waves, schema rejection and
overflow delivery. Run from the workspace with `cargo test --locked -p
phenix-storage`, `cargo test --locked -p phenix-kernel`, and `cargo run --locked -p
phenix-kernel --example storage`.

The backend set is closed: only memory and SQLite exist. A schema descriptor is
self-attestation by the host and codec author, not proof of structural identity:
there is no automatic structural derivation, Rust-layout inspection, SQL table
validation, or enforcement that two codecs claiming the same descriptor behave
identically. SQL registration metadata does not type-check SQL rows. No future
backend or abstraction below should be read as an implemented capability.

## Public Abstraction Review

Each row answers six questions: who owns it, what invariant it enforces, what
existing facility it uses, why it exists, which phase it simplifies, and whether
it can be replaced. Phase describes current use, not a promised roadmap.

| Public abstraction | Ownership | Invariant | Existing facility | Reason | Simplifies phase | Replaceability |
| --- | --- | --- | --- | --- | --- | --- |
| `Address` | Host/plugin value | Canonical logical mount/dataset, never a path | Strings and strict byte validation | Stable resource identity | Provisioning and lookup | Grammar is public; parser internals replaceable |
| `Schema` | Host registers; codec author declares | Four fields must compare equal | Owned strings and revision | Reject declared schema/codec mismatch, not prove structure | Registration and typed access | Descriptor convention is application-owned; no migration |
| `Codec` | Application author | Typed value/bytes boundary with declared schema | Associated type and byte vectors | Avoid mandatory serialization format | Memory encode/decode | Application can replace implementation if metadata and bytes remain truthful |
| `Operation` | Host grant policy | No implicit operation hierarchy | Closed enum | Explicit permission vocabulary | Authorization | New operations require API/policy changes |
| `Scope` | Host grants; plugin subscriptions | Exact dataset or whole mount, no prefixes | `Address`/mount string | Align authority and invalidation scope | Authorization and delivery | Closed enum; changing scope semantics affects policy |
| `Grant` | Host declaration, copied into session | Declaration alone conveys no authority | Set of operations | Explicit session provisioning | Host setup | Host policy replaceable; issuance remains trusted |
| `Registry` | Trusted host; shared handles | Unique mounts/datasets, trusted issuance, serialized mutation | `Arc<Mutex<_>>`, map, rusqlite connection | Resource and authority root | Provisioning and commit | Private closed backend implementation replaceable, not pluggable |
| `Session` | Host issues; plugin holds/clones | Immutable explicit grants; authorize before lookup | Shared registry and grant vector | Separate use from provisioning authority | Plugin execution | Internal authorization replaceable, public semantics must hold |
| `Capabilities` | Adapter returns to authorized caller | Features are not permissions | Booleans and `Observation` | Report supported access modes | Binding setup | Closed report; no dynamic backend protocol |
| `Capability` | Adapter error metadata | Unsupported identifies `TypedKv` or `Sql` | Closed enum | Machine-readable failure | Access failure handling | Additions require API review |
| `Observation` | Adapter constructs; consumer reads | Source, coverage and delivery travel together | Three closed enums | Make invalidation limits explicit | Binding and rescan policy | Representation replaceable only preserving stated guarantees |
| `ObservationSource` | Adapter metadata | Only `RegistryWrites`, not external changes | Closed enum | Prevent change-capture assumptions | Observation interpretation | New source needs real implementation |
| `ObservationCoverage` | Backend metadata | `Key` for memory, `Mount` for SQLite | Closed enum | Avoid fake row/table precision | Dependency invalidation | New precision needs evidence and implementation |
| `ObservationDelivery` | Queue contract | Only `BoundedNonDurableWithOverflow` | Closed enum | State loss and overflow behavior | Consumer recovery | Durable delivery is not a drop-in claim |
| `Sql` | Adapter lends to callback | Connection cannot escape; transaction state checked | rusqlite prepare/transaction/authorizer | Native SQL without a new SQL language | SQLite execution | Facade internals replaceable; SQLite semantics remain exposed |
| `Row` | Adapter lends to decoder | Borrowed row cannot escape; typed indexed reads | rusqlite `Row` and `FromSql` | Decode without exposing connection/raw row | Result decoding | Thin facade replaceable within lifetime contract |
| `ToSql` (re-export) | rusqlite; caller implements/borrows | Native parameter conversion | rusqlite trait | Reuse maintained binding API | Parameter binding | Deliberate driver coupling, not backend-neutral |
| `FromSql` (re-export) | rusqlite; decoder chooses type | Native result conversion | rusqlite trait | Reuse maintained decoding API | Row decoding | Deliberate driver coupling |
| `Value` (re-export) | Caller-owned SQL value | Native SQLite value variants | rusqlite enum | Dynamic values without JSON | Binding/decoding | Driver type, not a universal storage value |
| `Invalidation` | Registry emits; consumer owns | Ordered registry-local metadata, not row values | Sequence, scope, key, observation | Identify what to re-read conservatively | Cache/graph binding | Consumer mapping replaceable; not a durable event format |
| `Notice` | Queue emits; consumer owns | Change or explicit overflow watermark | Closed enum | Never silently hide queue overflow | Drain and recovery | New outcomes require consumer review |
| `Subscription` | Plugin/consumer; weak registry reference | Bounded queue; drop cancels; closed differs from empty | `Weak`, `VecDeque`, mutex | Pull delivery without callback/channel sends | Polling and teardown | Queue internals replaceable within loss/closure contract |
| `Error` | Library returns; caller handles | Distinguish denial, unsupported, closed and structured SQL failure | Closed enum and rusqlite error source | Actionable failures without string matching | Failure handling | Variants are public; driver coupling is explicit |
| `Result<T>` | Library/caller | Common error channel | Standard `Result<T, Error>` alias | Consistent signatures | All fallible phases | Alias only, no new runtime abstraction |

## Research Links

- [SpacetimeDB repository](https://github.com/clockworklabs/SpacetimeDB)
- [SpacetimeDB documentation](https://spacetimedb.com/docs/)
- [SurrealDB repository](https://github.com/surrealdb/surrealdb)
- [SurrealDB LIVE SELECT](https://surrealdb.com/docs/surrealql/statements/live)
- [SQLx repository and runtime support](https://github.com/launchbadge/sqlx)
- [SQLx documentation](https://docs.rs/sqlx/latest/sqlx/)
- [SeaQuery repository](https://github.com/SeaQL/sea-query)
- [SeaQuery SQL builders and values](https://docs.rs/sea-query/latest/sea_query/)
- [redb repository](https://github.com/cberner/redb)
- [redb documentation](https://docs.rs/redb/latest/redb/)
- [PostgreSQL LISTEN](https://www.postgresql.org/docs/current/sql-listen.html)
- [PostgreSQL NOTIFY](https://www.postgresql.org/docs/current/sql-notify.html)

- [rusqlite API and maintained repository](https://github.com/rusqlite/rusqlite)
- [rusqlite bundled feature and SQLite build](https://docs.rs/rusqlite/latest/rusqlite/#notes-on-building-rusqlite-and-libsqlite3-sys)
- [rusqlite transactions](https://docs.rs/rusqlite/latest/rusqlite/struct.Transaction.html)
- [rusqlite FromSql](https://docs.rs/rusqlite/latest/rusqlite/types/trait.FromSql.html)
- [SQLite parameter binding](https://www.sqlite.org/c3ref/bind_blob.html)
- [SQLite authorizer](https://www.sqlite.org/c3ref/set_authorizer.html)
- [SQLite transaction semantics and automatic rollback](https://www.sqlite.org/lang_transaction.html)
- [SQLite update-hook limitations](https://www.sqlite.org/c3ref/update_hook.html)
- [SQLite commit-hook restrictions](https://www.sqlite.org/c3ref/commit_hook.html)

SQLite update hooks alone do not establish commit success and do not cover all
changes. This implementation deliberately tracks writes at its facade and emits
conservative invalidation only after commit, instead of advertising hook-based
metadata as complete database change capture.

SpacetimeDB's subscription model combines an initial query result snapshot with
ongoing matching changes maintained in a client cache. An initial snapshot alone
is not a live subscription. This crate provides neither that snapshot/live handoff
nor maintained query results: callers read explicitly and drain lossy invalidation
notices. It does not embed SpacetimeDB, and its registry sequence is not a database
snapshot token. SurrealDB's LIVE SELECT is likewise a database-managed live-query
facility, not equivalent to these mount/key notices; no SurrealDB adapter exists.

SQLx is absent deliberately: this synchronous, SQLite-only implementation uses
rusqlite's native binding, typed decoding, transactions and authorizer hooks without
requiring an async runtime or introducing a second driver API. This is a scope
choice, not a claim that SQLx is unmaintained or unsuitable for other applications.
An application author can use SeaQuery to build SQLite SQL and supply the resulting
SQL text and rusqlite-compatible bound parameters to `Sql`. Builder values need
appropriate conversion; there is no built-in SeaQuery adapter. No additional SQL
parser is required: SQLite parses and the authorizer enforces the connection
boundary. A builder neither creates authority nor makes identifiers value parameters.

redb is an optional future embedded persistent-KV candidate, not an optional Cargo
feature or implemented backend today. PostgreSQL is a future network-backend
possibility only; LISTEN/NOTIFY research does not constitute an adapter, durable
outbox, replay guarantee or automatic graph binding. Neither is simulated through
SQLite or advertised by current capabilities. Any addition must implement honest
transaction, authority and observation semantics before extending the closed set.
