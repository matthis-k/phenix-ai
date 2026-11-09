//! Synchronous, explicitly authorized storage. See `docs/storage.md` in the repository.
//! Hosts can mount external backends through object-safe optional capabilities.
//! Observations are invalidations, not committed row changes or a durable change log.

pub mod scoped;
pub mod snapshot;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, Weak};

pub use rusqlite::types::{FromSql, ToSql, Value};
use rusqlite::{
    Connection,
    hooks::{AuthAction, AuthContext, Authorization},
};

#[derive(Debug)]
pub enum Error {
    InvalidAddress,
    DuplicateMount,
    DuplicateDataset,
    UnknownMount,
    UnknownDataset,
    Denied,
    UnsupportedCapability { capability: Capability },
    Conflict { current: Option<Revision> },
    RevisionExhausted,
    SubscriptionClosed,
    SchemaMismatch,
    InvalidCapacity,
    TransactionEnded,
    Poisoned,
    Reentrant,
    MixedSnapshotSources,
    SnapshotEntryMismatch,
    MissingInput { address: Address, key: String },
    Codec(String),
    Sql(rusqlite::Error),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sql(e) => Some(e),
            _ => None,
        }
    }
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e)
    }
}
pub type Result<T> = std::result::Result<T, Error>;

/// Opaque backend token, comparable only for the same mount/dataset/key.
/// Not authority or a database snapshot. Restoring an older database file also
/// restores its tokens; absence has no token and carries no history guarantee.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revision(Vec<u8>);
impl Revision {
    /// Backend extension point. Tokens must never be reused after a mutation.
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revisioned<T> {
    pub value: T,
    pub revision: Revision,
}

fn component(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// Canonical logical address, never a filesystem path or SQL identifier.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Address {
    mount: String,
    dataset: String,
}
impl Address {
    pub fn parse(value: &str) -> Result<Self> {
        let (mount, dataset) = value
            .strip_prefix("store://")
            .and_then(|s| s.split_once('/'))
            .ok_or(Error::InvalidAddress)?;
        if !component(mount) || !component(dataset) {
            return Err(Error::InvalidAddress);
        }
        Ok(Self {
            mount: mount.into(),
            dataset: dataset.into(),
        })
    }
    pub fn mount(&self) -> &str {
        &self.mount
    }
    pub fn dataset(&self) -> &str {
        &self.dataset
    }
}
impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "store://{}/{}", self.mount, self.dataset)
    }
}

/// All four fields are authoritative and compared before decoding or encoding.
pub use phenix_schema::Schema;
pub trait Codec {
    type Value;
    fn schema(&self) -> Schema;
    fn encode(&self, value: &Self::Value) -> Result<Vec<u8>>;
    fn decode(&self, bytes: &[u8]) -> Result<Self::Value>;
}

/// Bind a codec to a shared, mechanically derived descriptor. Registry admission
/// compares this descriptor before invoking either codec operation.
pub struct SchemaCodec<C> {
    pub codec: C,
    pub schema: Schema,
}
impl<C: Codec> Codec for SchemaCodec<C> {
    type Value = C::Value;
    fn schema(&self) -> Schema {
        self.schema.clone()
    }
    fn encode(&self, value: &Self::Value) -> Result<Vec<u8>> {
        self.codec.encode(value)
    }
    fn decode(&self, bytes: &[u8]) -> Result<Self::Value> {
        self.codec.decode(bytes)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Operation {
    Read,
    Write,
    Delete,
    /// Legacy full SQL authority, equivalent to SqlAdmin. Mount scope only.
    Sql,
    /// SELECT and read-only SQL functions. Mount scope only.
    SqlRead,
    /// SqlRead plus INSERT, UPDATE and DELETE, including trigger effects.
    SqlWrite,
    /// SqlWrite plus DDL. Connection policy and transaction escape remain forbidden.
    SqlAdmin,
    Subscribe,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    Dataset(Address),
    Mount(String),
}
impl Scope {
    fn covers(&self, address: &Address) -> bool {
        match self {
            Self::Dataset(a) => a == address,
            Self::Mount(m) => m == address.mount(),
        }
    }
}
#[derive(Clone, Debug)]
pub struct Grant {
    pub scope: Scope,
    pub operations: BTreeSet<Operation>,
}
impl Grant {
    pub fn new(scope: Scope, operations: impl IntoIterator<Item = Operation>) -> Self {
        Self {
            scope,
            operations: operations.into_iter().collect(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub typed_kv: bool,
    /// Revisioned reads and atomic conditional writes, not additional authority.
    pub compare_exchange: bool,
    pub sql: bool,
    pub invalidation: bool,
    pub observation: Observation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    TypedKv,
    CompareExchange,
    AtomicReadMany,
    Sql,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationSource {
    RegistryWrites,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationCoverage {
    Key,
    Mount,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationDelivery {
    BoundedNonDurableWithOverflow,
}
/// External connections/processes are not observed. Consumers must rescan on overflow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observation {
    pub source: ObservationSource,
    pub coverage: ObservationCoverage,
    pub delivery: ObservationDelivery,
}
fn observation(coverage: ObservationCoverage) -> Observation {
    Observation {
        source: ObservationSource::RegistryWrites,
        coverage,
        delivery: ObservationDelivery::BoundedNonDurableWithOverflow,
    }
}

/// Trusted host extension point. Backends must not expose authority to sessions.
pub trait Backend: Send {
    /// Validate/provision persistent dataset metadata before registry admission.
    /// In-memory adapters may use the default; durable adapters should enforce
    /// schema identity across process restarts.
    fn register_dataset(&mut self, _dataset: &str, _schema: &Schema) -> Result<()> {
        Ok(())
    }
    fn typed_kv(&mut self) -> Option<&mut dyn TypedKv> {
        None
    }
    fn sql(&mut self) -> Option<&mut dyn SqlBackend> {
        None
    }
    fn observation(&self) -> Observation {
        observation(ObservationCoverage::Mount)
    }
}
pub trait TypedKv: Send {
    /// Opt in only for a coherent, versioned snapshot over the entire batch.
    fn supports_atomic_read_many(&self) -> bool {
        false
    }
    fn read_many(
        &mut self,
        _keys: &[snapshot::ReadKey<'_>],
    ) -> Result<Vec<Option<Revisioned<Vec<u8>>>>> {
        Err(Error::UnsupportedCapability {
            capability: Capability::AtomicReadMany,
        })
    }
    fn get(&mut self, dataset: &str, key: &str) -> Result<Option<Vec<u8>>>;
    fn put(&mut self, dataset: &str, key: &str, value: Vec<u8>) -> Result<()>;
    fn delete(&mut self, dataset: &str, key: &str) -> Result<bool>;
    /// Opt in only when both revision methods are implemented. All successful
    /// puts must invalidate older tokens, including delete/recreate cycles.
    /// Durable backends must retain that guarantee across reopen.
    fn supports_compare_exchange(&self) -> bool {
        false
    }
    fn get_revisioned(
        &mut self,
        _dataset: &str,
        _key: &str,
    ) -> Result<Option<Revisioned<Vec<u8>>>> {
        Err(Error::UnsupportedCapability {
            capability: Capability::CompareExchange,
        })
    }
    /// Must atomically compare and mutate within this backend's resource boundary.
    fn compare_exchange(
        &mut self,
        _dataset: &str,
        _key: &str,
        _expected: Option<&Revision>,
        _value: Vec<u8>,
    ) -> Result<Revision> {
        Err(Error::UnsupportedCapability {
            capability: Capability::CompareExchange,
        })
    }
}
/// Lends a connection with no installed authorizer or active transaction.
/// The registry installs its SQL policy only for the duration of each transaction,
/// then removes it before returning the connection to the trusted backend.
pub trait SqlBackend: Send {
    fn connection(&mut self) -> &mut Connection;
}
#[derive(Default)]
pub struct MemoryBackend {
    values: BTreeMap<(String, String), Revisioned<Vec<u8>>>,
    sequence: u64,
}
impl MemoryBackend {
    fn next_revision(&mut self) -> Result<Revision> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(Error::RevisionExhausted)?;
        Ok(Revision(self.sequence.to_be_bytes().to_vec()))
    }
}
impl Backend for MemoryBackend {
    fn typed_kv(&mut self) -> Option<&mut dyn TypedKv> {
        Some(self)
    }
    fn observation(&self) -> Observation {
        observation(ObservationCoverage::Key)
    }
}
impl TypedKv for MemoryBackend {
    fn supports_atomic_read_many(&self) -> bool {
        true
    }
    fn read_many(
        &mut self,
        keys: &[snapshot::ReadKey<'_>],
    ) -> Result<Vec<Option<Revisioned<Vec<u8>>>>> {
        keys.iter()
            .map(|k| self.get_revisioned(k.dataset, k.key))
            .collect()
    }
    fn get(&mut self, dataset: &str, key: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .values
            .get(&(dataset.into(), key.into()))
            .map(|v| v.value.clone()))
    }
    fn put(&mut self, dataset: &str, key: &str, value: Vec<u8>) -> Result<()> {
        let revision = self.next_revision()?;
        self.values
            .insert((dataset.into(), key.into()), Revisioned { value, revision });
        Ok(())
    }
    fn delete(&mut self, dataset: &str, key: &str) -> Result<bool> {
        Ok(self.values.remove(&(dataset.into(), key.into())).is_some())
    }
    fn supports_compare_exchange(&self) -> bool {
        true
    }
    fn get_revisioned(&mut self, dataset: &str, key: &str) -> Result<Option<Revisioned<Vec<u8>>>> {
        Ok(self.values.get(&(dataset.into(), key.into())).cloned())
    }
    fn compare_exchange(
        &mut self,
        dataset: &str,
        key: &str,
        expected: Option<&Revision>,
        value: Vec<u8>,
    ) -> Result<Revision> {
        let current = self
            .values
            .get(&(dataset.into(), key.into()))
            .map(|v| v.revision.clone());
        if current.as_ref() != expected {
            return Err(Error::Conflict { current });
        }
        let revision = self.next_revision()?;
        self.values.insert(
            (dataset.into(), key.into()),
            Revisioned {
                value,
                revision: revision.clone(),
            },
        );
        Ok(revision)
    }
}
struct SqliteBackend(Connection);
impl Backend for SqliteBackend {
    fn register_dataset(&mut self, dataset: &str, schema: &Schema) -> Result<()> {
        use rusqlite::OptionalExtension;
        self.reset_sql_authority()?;
        let tx = self
            .0
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let existing = tx
            .query_row(
                "SELECT name, revision, structure, codec FROM __phenix_schema WHERE dataset = ?1",
                rusqlite::params![dataset],
                |row| {
                    Ok(Schema {
                        name: row.get(0)?,
                        revision: row.get(1)?,
                        structure: row.get(2)?,
                        codec: row.get(3)?,
                    })
                },
            )
            .optional()?;
        if let Some(previous) = existing {
            if previous != *schema {
                return Err(Error::SchemaMismatch);
            }
        } else {
            tx.execute(
                "INSERT INTO __phenix_schema (dataset, name, revision, structure, codec) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    dataset,
                    &schema.name,
                    schema.revision,
                    &schema.structure,
                    &schema.codec
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    fn typed_kv(&mut self) -> Option<&mut dyn TypedKv> {
        Some(self)
    }
    fn sql(&mut self) -> Option<&mut dyn SqlBackend> {
        Some(self)
    }
}
impl SqliteBackend {
    fn write_revisioned(
        &mut self,
        dataset: &str,
        key: &str,
        expected: Option<Option<&Revision>>,
        value: Vec<u8>,
    ) -> Result<Revision> {
        self.reset_sql_authority()?;
        let tx = self
            .0
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if let Some(expected) = expected {
            use rusqlite::OptionalExtension;
            let current = tx
                .query_row(
                    "SELECT revision FROM __phenix_kv WHERE dataset = ?1 AND key = ?2",
                    rusqlite::params![dataset, key],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?
                .map(|v| Revision(v.to_be_bytes().to_vec()));
            if current.as_ref() != expected {
                return Err(Error::Conflict { current });
            }
        }
        let sequence: i64 = tx.query_row(
            "SELECT sequence FROM __phenix_revision_clock WHERE id = 1",
            [],
            |r| r.get(0),
        )?;
        let sequence = sequence.checked_add(1).ok_or(Error::RevisionExhausted)?;
        tx.execute(
            "UPDATE __phenix_revision_clock SET sequence = ?1 WHERE id = 1",
            [sequence],
        )?;
        tx.execute("INSERT INTO __phenix_kv (dataset, key, value, revision) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(dataset, key) DO UPDATE SET value = excluded.value, revision = excluded.revision", rusqlite::params![dataset, key, value, sequence])?;
        tx.commit()?;
        Ok(Revision(sequence.to_be_bytes().to_vec()))
    }
    // Only direct adapter SQL gets internal authority. In particular, triggers
    // persisted by older versions must not inherit it during KV writes.
    fn reset_sql_authority(&mut self) -> Result<()> {
        self.0.authorizer(Some(adapter_authorizer))?;
        Ok(())
    }
}
impl TypedKv for SqliteBackend {
    fn supports_atomic_read_many(&self) -> bool {
        true
    }
    fn read_many(
        &mut self,
        keys: &[snapshot::ReadKey<'_>],
    ) -> Result<Vec<Option<Revisioned<Vec<u8>>>>> {
        use rusqlite::OptionalExtension;
        self.reset_sql_authority()?;
        let tx = self
            .0
            .transaction_with_behavior(rusqlite::TransactionBehavior::Deferred)?;
        let entries = {
            let mut statement = tx.prepare(
                "SELECT value, revision FROM __phenix_kv WHERE dataset = ?1 AND key = ?2",
            )?;
            keys.iter()
                .map(|key| {
                    Ok(statement
                        .query_row(rusqlite::params![key.dataset, key.key], |row| {
                            Ok(Revisioned {
                                value: row.get(0)?,
                                revision: Revision(row.get::<_, i64>(1)?.to_be_bytes().to_vec()),
                            })
                        })
                        .optional()?)
                })
                .collect::<Result<Vec<_>>>()?
        };
        tx.commit()?;
        Ok(entries)
    }
    fn get(&mut self, dataset: &str, key: &str) -> Result<Option<Vec<u8>>> {
        self.reset_sql_authority()?;
        use rusqlite::OptionalExtension;
        Ok(self
            .0
            .query_row(
                "SELECT value FROM __phenix_kv WHERE dataset = ?1 AND key = ?2",
                rusqlite::params![dataset, key],
                |row| row.get(0),
            )
            .optional()?)
    }
    fn put(&mut self, dataset: &str, key: &str, value: Vec<u8>) -> Result<()> {
        self.write_revisioned(dataset, key, None, value)?;
        Ok(())
    }
    fn delete(&mut self, dataset: &str, key: &str) -> Result<bool> {
        self.reset_sql_authority()?;
        Ok(self.0.execute(
            "DELETE FROM __phenix_kv WHERE dataset = ?1 AND key = ?2",
            rusqlite::params![dataset, key],
        )? != 0)
    }
    fn supports_compare_exchange(&self) -> bool {
        true
    }
    fn get_revisioned(&mut self, dataset: &str, key: &str) -> Result<Option<Revisioned<Vec<u8>>>> {
        self.reset_sql_authority()?;
        use rusqlite::OptionalExtension;
        Ok(self
            .0
            .query_row(
                "SELECT value, revision FROM __phenix_kv WHERE dataset = ?1 AND key = ?2",
                rusqlite::params![dataset, key],
                |r| {
                    Ok(Revisioned {
                        value: r.get(0)?,
                        revision: Revision(r.get::<_, i64>(1)?.to_be_bytes().to_vec()),
                    })
                },
            )
            .optional()?)
    }
    fn compare_exchange(
        &mut self,
        dataset: &str,
        key: &str,
        expected: Option<&Revision>,
        value: Vec<u8>,
    ) -> Result<Revision> {
        self.write_revisioned(dataset, key, Some(expected), value)
    }
}
impl SqlBackend for SqliteBackend {
    fn connection(&mut self) -> &mut Connection {
        &mut self.0
    }
}
struct Mount {
    backend: Box<dyn Backend>,
    datasets: BTreeMap<String, Schema>,
}
struct Watch {
    scope: Scope,
    capacity: usize,
    queue: VecDeque<Notice>,
    overflow: Option<u64>,
    waker: Option<Arc<std::task::Waker>>,
}
#[derive(Default)]
struct State {
    mounts: BTreeMap<String, Arc<Mutex<Mount>>>,
    reservations: BTreeSet<String>,
    watches: Vec<Weak<Mutex<Watch>>>,
    sequence: u64,
}
#[derive(Clone, Default)]
pub struct Registry {
    inner: Arc<Mutex<State>>,
}
fn lock(inner: &Mutex<State>) -> Result<MutexGuard<'_, State>> {
    inner.lock().map_err(|_| Error::Poisoned)
}
struct MountReservation<'a> {
    registry: &'a Registry,
    name: &'a str,
}
impl Drop for MountReservation<'_> {
    fn drop(&mut self) {
        self.registry
            .inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .reservations
            .remove(self.name);
    }
}
thread_local! {
    static ACTIVE: std::cell::RefCell<BTreeSet<usize>> = const { std::cell::RefCell::new(BTreeSet::new()) };
    static PENDING_WAKES: std::cell::RefCell<Vec<WakerAction>> = const { std::cell::RefCell::new(Vec::new()) };
}
struct Active(usize);
impl Active {
    fn enter(mount: &Arc<Mutex<Mount>>) -> Result<Self> {
        let id = Arc::as_ptr(mount) as usize;
        ACTIVE.with(|active| {
            if active.borrow_mut().insert(id) {
                Ok(Self(id))
            } else {
                Err(Error::Reentrant)
            }
        })
    }
}
impl Drop for Active {
    fn drop(&mut self) {
        let idle = ACTIVE.with(|active| {
            let mut active = active.borrow_mut();
            active.remove(&self.0);
            active.is_empty()
        });
        if idle {
            let wakes = PENDING_WAKES.with(|pending| std::mem::take(&mut *pending.borrow_mut()));
            run_wakers(wakes);
        }
    }
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn mount(&self, name: &str, backend: impl Backend + 'static) -> Result<()> {
        self.mount_boxed(name, Box::new(backend))
    }
    /// Trusted host may install an erased backend supplied by a leased plugin
    /// contribution. The host must retain the provider's code/instance lease
    /// until after the backend is dropped (see phenix-storage-plugin).
    pub fn mount_boxed(&self, name: &str, backend: Box<dyn Backend>) -> Result<()> {
        self.mount_with(name, || Ok(backend))
    }
    /// Reserve a valid, unused name before invoking a trusted backend factory.
    /// The factory runs without storage locks. Error or unwind releases the name;
    /// concurrent attempts at the same name fail with DuplicateMount.
    pub fn mount_with<E: From<Error>>(
        &self,
        name: &str,
        open: impl FnOnce() -> std::result::Result<Box<dyn Backend>, E>,
    ) -> std::result::Result<(), E> {
        if !component(name) {
            return Err(Error::InvalidAddress.into());
        }
        {
            let mut state = lock(&self.inner)?;
            if state.mounts.contains_key(name) || !state.reservations.insert(name.into()) {
                return Err(Error::DuplicateMount.into());
            }
        }
        let _reservation = MountReservation {
            registry: self,
            name,
        };
        let backend = open()?;
        let mut state = lock(&self.inner)?;
        state.mounts.insert(
            name.into(),
            Arc::new(Mutex::new(Mount {
                backend,
                datasets: BTreeMap::new(),
            })),
        );
        Ok(())
    }
    pub fn mount_memory(&self, name: &str) -> Result<()> {
        self.mount(name, MemoryBackend::default())
    }
    /// Advisory, read-only planner preflight. Missing files, missing dataset rows,
    /// and legacy databases without schema metadata are allowed, matching initial
    /// registration behavior. No tables, directories or migrations are provisioned.
    /// This checks schema declarations, not KV contents or revision-clock health.
    /// Activation must still register every dataset to recheck after planning races.
    pub fn validate_sqlite_datasets(
        path: impl AsRef<std::path::Path>,
        datasets: &[(String, Schema)],
    ) -> Result<()> {
        use rusqlite::OptionalExtension;
        let path = path.as_ref();
        if matches!(path.try_exists(), Ok(false)) {
            return Ok(());
        }
        // Let SQLite report inaccessible/invalid paths as structured SQL errors.
        // READ_ONLY deliberately omits CREATE and URI interpretation.
        let mut connection =
            Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        connection.authorizer(Some(adapter_authorizer))?;
        let tx = connection.transaction()?;
        let kind: Option<String> = tx
            .query_row(
                "SELECT type FROM main.sqlite_schema WHERE name = '__phenix_schema' COLLATE NOCASE",
                [],
                |row| row.get(0),
            )
            .optional()?;
        match kind.as_deref() {
            None => return Ok(()),
            Some("table") => {}
            _ => return Err(Error::SchemaMismatch),
        }
        for (dataset, schema) in datasets {
            let existing: Option<Schema> = tx.query_row(
                "SELECT name, revision, structure, codec FROM main.__phenix_schema WHERE dataset = ?1",
                [dataset],
                |row| Ok(Schema {
                    name: row.get(0)?,
                    revision: row.get(1)?,
                    structure: row.get(2)?,
                    codec: row.get(3)?,
                }),
            ).optional()?;
            if existing.is_some_and(|existing| existing != *schema) {
                return Err(Error::SchemaMismatch);
            }
        }
        tx.commit()?;
        Ok(())
    }
    /// The host owns path selection; logical addresses never select files.
    pub fn mount_sqlite(&self, name: &str, path: impl AsRef<std::path::Path>) -> Result<()> {
        self.mount_with(name, || {
            let mut connection = Connection::open(path)?;
            connection.busy_timeout(std::time::Duration::from_secs(5))?;
            connection.pragma_update(None, "foreign_keys", true)?;
            connection.authorizer(Some(adapter_authorizer))?;
            // Migrate adapter metadata under the same cross-connection write lock
            // used by CAS. Publication always follows a successful commit.
            let tx =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS __phenix_kv (
                dataset TEXT NOT NULL,
                key TEXT NOT NULL,
                value BLOB NOT NULL,
                PRIMARY KEY (dataset, key)
            );
            CREATE TABLE IF NOT EXISTS __phenix_schema (
                dataset TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                revision INTEGER NOT NULL,
                structure TEXT NOT NULL,
                codec TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS __phenix_revision_clock (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                sequence INTEGER NOT NULL
            )",
            )?;
            let has_revision = {
                let mut statement = tx.prepare("PRAGMA table_info(__phenix_kv)")?;
                let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
                columns
                    .collect::<std::result::Result<Vec<_>, _>>()?
                    .iter()
                    .any(|name| name == "revision")
            };
            if !has_revision {
                // Legacy rows predate revision tokens. Zero is never allocated again.
                tx.execute_batch(
                    "ALTER TABLE __phenix_kv ADD COLUMN revision INTEGER NOT NULL DEFAULT 0",
                )?;
                tx.execute("INSERT INTO __phenix_revision_clock VALUES (1, 0)", [])?;
            } else {
                use rusqlite::OptionalExtension;
                let clock: Option<i64> = tx
                    .query_row(
                        "SELECT sequence FROM __phenix_revision_clock WHERE id = 1",
                        [],
                        |r| r.get(0),
                    )
                    .optional()?;
                let maximum: i64 = tx.query_row(
                    "SELECT coalesce(max(revision), 0) FROM __phenix_kv",
                    [],
                    |r| r.get(0),
                )?;
                // Never reconstruct a clock from live rows: deleted rows may have
                // issued larger tokens. Missing metadata must fail closed.
                if clock.is_none_or(|clock| clock < maximum || clock < 0) {
                    return Err(Error::SchemaMismatch);
                }
            }
            tx.commit()?;
            Ok(Box::new(SqliteBackend(connection)))
        })
    }
    pub fn register(&self, address: &Address, schema: Schema) -> Result<()> {
        let mount = lock(&self.inner)?
            .mounts
            .get(address.mount())
            .cloned()
            .ok_or(Error::UnknownMount)?;
        let _active = Active::enter(&mount)?;
        let mut mount = mount.lock().map_err(|_| Error::Poisoned)?;
        if mount.datasets.contains_key(address.dataset()) {
            return Err(Error::DuplicateDataset);
        }
        // Persistent backends must verify the existing on-disk schema before
        // the in-memory dataset declaration becomes visible.
        mount.backend.register_dataset(address.dataset(), &schema)?;
        mount.datasets.insert(address.dataset.clone(), schema);
        Ok(())
    }
    /// Trusted host authority: do not expose the registry to untrusted plugins.
    pub fn session(&self, grants: impl IntoIterator<Item = Grant>) -> Session {
        Session {
            inner: self.inner.clone(),
            grants: grants.into_iter().collect(),
        }
    }
}

#[derive(Clone)]
pub struct Session {
    inner: Arc<Mutex<State>>,
    grants: Vec<Grant>,
}
impl Session {
    fn authorize(&self, address: &Address, operation: Operation) -> Result<()> {
        if self.grants.iter().any(|g| {
            g.operations.contains(&operation)
                && g.scope.covers(address)
                && (!matches!(
                    operation,
                    Operation::Sql | Operation::SqlRead | Operation::SqlWrite | Operation::SqlAdmin
                ) || matches!(&g.scope, Scope::Mount(_)))
        }) {
            Ok(())
        } else {
            Err(Error::Denied)
        }
    }
    fn mount(&self, address: &Address) -> Result<Arc<Mutex<Mount>>> {
        let mount = lock(&self.inner)?
            .mounts
            .get(address.mount())
            .cloned()
            .ok_or(Error::UnknownMount)?;
        let _active = Active::enter(&mount)?;
        let guard = mount.lock().map_err(|_| Error::Poisoned)?;
        if !guard.datasets.contains_key(address.dataset()) {
            return Err(Error::UnknownDataset);
        }
        drop(guard);
        Ok(mount)
    }
    pub fn capabilities(&self, address: &Address, operation: Operation) -> Result<Capabilities> {
        self.authorize(address, operation)?;
        let mount = self.mount(address)?;
        self.mount_capabilities(mount)
    }
    /// Mount-scoped capability lookup without a dataset declaration.
    /// Requires the exact requested grant; SQL's execution hierarchy is not used.
    pub fn capabilities_mount(&self, name: &str, operation: Operation) -> Result<Capabilities> {
        let mount = self.authorized_mount(name, operation)?;
        self.mount_capabilities(mount)
    }
    fn authorized_mount(&self, name: &str, operation: Operation) -> Result<Arc<Mutex<Mount>>> {
        if !self.has_mount_grant(name, operation) {
            return Err(Error::Denied);
        }
        if !component(name) {
            return Err(Error::InvalidAddress);
        }
        lock(&self.inner)?
            .mounts
            .get(name)
            .cloned()
            .ok_or(Error::UnknownMount)
    }
    fn has_mount_grant(&self, name: &str, operation: Operation) -> bool {
        self.grants.iter().any(|g| {
            matches!(&g.scope, Scope::Mount(m) if m == name) && g.operations.contains(&operation)
        })
    }
    fn mount_capabilities(&self, mount: Arc<Mutex<Mount>>) -> Result<Capabilities> {
        let _active = Active::enter(&mount)?;
        let mut mount = mount.lock().map_err(|_| Error::Poisoned)?;
        let (typed_kv, compare_exchange) = match mount.backend.typed_kv() {
            Some(kv) => (true, kv.supports_compare_exchange()),
            None => (false, false),
        };
        Ok(Capabilities {
            typed_kv,
            compare_exchange,
            sql: mount.backend.sql().is_some(),
            invalidation: true,
            observation: mount.backend.observation(),
        })
    }
    pub fn get<C: Codec>(
        &self,
        address: &Address,
        key: &str,
        codec: &C,
    ) -> Result<Option<C::Value>> {
        self.authorize(address, Operation::Read)?;
        let schema = codec.schema();
        let bytes = {
            let mount = self.mount(address)?;
            let _active = Active::enter(&mount)?;
            let mut mount = mount.lock().map_err(|_| Error::Poisoned)?;
            if mount.datasets[address.dataset()] != schema {
                return Err(Error::SchemaMismatch);
            }
            let Some(values) = mount.backend.typed_kv() else {
                return Err(Error::UnsupportedCapability {
                    capability: Capability::TypedKv,
                });
            };
            values.get(address.dataset(), key)?
        };
        bytes.map(|b| codec.decode(&b)).transpose()
    }
    pub fn get_revisioned<C: Codec>(
        &self,
        address: &Address,
        key: &str,
        codec: &C,
    ) -> Result<Option<Revisioned<C::Value>>> {
        self.authorize(address, Operation::Read)?;
        let schema = codec.schema();
        let bytes = {
            let mount = self.mount(address)?;
            let _active = Active::enter(&mount)?;
            let mut mount = mount.lock().map_err(|_| Error::Poisoned)?;
            if mount.datasets[address.dataset()] != schema {
                return Err(Error::SchemaMismatch);
            }
            let values = mount
                .backend
                .typed_kv()
                .ok_or(Error::UnsupportedCapability {
                    capability: Capability::TypedKv,
                })?;
            values.get_revisioned(address.dataset(), key)?
        };
        bytes
            .map(|v| {
                Ok(Revisioned {
                    value: codec.decode(&v.value)?,
                    revision: v.revision,
                })
            })
            .transpose()
    }
    /// Requires Write, not Read. `None` creates only if absent; a stale token
    /// returns Conflict with the revision current at the atomic comparison.
    /// Encoding runs before comparison, outside storage locks, and may fail first.
    pub fn compare_exchange<C: Codec>(
        &self,
        address: &Address,
        key: &str,
        expected: Option<&Revision>,
        value: &C::Value,
        codec: &C,
    ) -> Result<Revision> {
        self.authorize(address, Operation::Write)?;
        let schema = codec.schema();
        {
            let mount = self.mount(address)?;
            let _active = Active::enter(&mount)?;
            let mut mount = mount.lock().map_err(|_| Error::Poisoned)?;
            if mount.datasets[address.dataset()] != schema {
                return Err(Error::SchemaMismatch);
            }
            let values = mount
                .backend
                .typed_kv()
                .ok_or(Error::UnsupportedCapability {
                    capability: Capability::TypedKv,
                })?;
            if !values.supports_compare_exchange() {
                return Err(Error::UnsupportedCapability {
                    capability: Capability::CompareExchange,
                });
            }
        }
        let bytes = codec.encode(value)?;
        let mount = self.mount(address)?;
        let _active = Active::enter(&mount)?;
        let mut mount = mount.lock().map_err(|_| Error::Poisoned)?;
        if mount.datasets[address.dataset()] != schema {
            return Err(Error::SchemaMismatch);
        }
        let values = mount
            .backend
            .typed_kv()
            .ok_or(Error::UnsupportedCapability {
                capability: Capability::TypedKv,
            })?;
        let revision = values.compare_exchange(address.dataset(), key, expected, bytes)?;
        let wakes = lock(&self.inner)?.emit(
            Scope::Dataset(address.clone()),
            Some(key.into()),
            mount.backend.observation(),
        );
        drop(mount);
        drop(_active);
        wake_all(wakes);
        Ok(revision)
    }
    pub fn put<C: Codec>(
        &self,
        address: &Address,
        key: &str,
        value: &C::Value,
        codec: &C,
    ) -> Result<()> {
        self.authorize(address, Operation::Write)?;
        let schema = codec.schema();
        // Validate schema before invoking user code; recheck under the mutation lock.
        {
            let mount = self.mount(address)?;
            let _active = Active::enter(&mount)?;
            let mut mount = mount.lock().map_err(|_| Error::Poisoned)?;
            if mount.datasets[address.dataset()] != schema {
                return Err(Error::SchemaMismatch);
            }
            if mount.backend.typed_kv().is_none() {
                return Err(Error::UnsupportedCapability {
                    capability: Capability::TypedKv,
                });
            }
        }
        let bytes = codec.encode(value)?;
        let mount = self.mount(address)?;
        let _active = Active::enter(&mount)?;
        let mut mount = mount.lock().map_err(|_| Error::Poisoned)?;
        if mount.datasets[address.dataset()] != schema {
            return Err(Error::SchemaMismatch);
        }
        let Some(values) = mount.backend.typed_kv() else {
            return Err(Error::UnsupportedCapability {
                capability: Capability::TypedKv,
            });
        };
        values.put(address.dataset(), key, bytes)?;
        let wakes = lock(&self.inner)?.emit(
            Scope::Dataset(address.clone()),
            Some(key.into()),
            mount.backend.observation(),
        );
        drop(mount);
        drop(_active);
        wake_all(wakes);
        Ok(())
    }
    pub fn delete(&self, address: &Address, key: &str) -> Result<bool> {
        self.authorize(address, Operation::Delete)?;
        let mount = self.mount(address)?;
        let _active = Active::enter(&mount)?;
        let mut mount = mount.lock().map_err(|_| Error::Poisoned)?;
        let Some(values) = mount.backend.typed_kv() else {
            return Err(Error::UnsupportedCapability {
                capability: Capability::TypedKv,
            });
        };
        let removed = values.delete(address.dataset(), key)?;
        let wakes = if removed {
            lock(&self.inner)?.emit(
                Scope::Dataset(address.clone()),
                Some(key.into()),
                mount.backend.observation(),
            )
        } else {
            Vec::new()
        };
        drop(mount);
        drop(_active);
        wake_all(wakes);
        Ok(removed)
    }
    /// Closure success commits; error or unwind rolls back. Same-mount reentry fails.
    /// Avoid nested transactions on different mounts in inconsistent lock order,
    /// or waiting for another thread that needs this mount, to prevent deadlocks.
    pub fn transaction<T>(
        &self,
        address: &Address,
        f: impl FnOnce(&Sql<'_>) -> Result<T>,
    ) -> Result<T> {
        // Preserve authorization-before-lookup for the address-based API.
        let allowed = [
            Operation::Sql,
            Operation::SqlAdmin,
            Operation::SqlWrite,
            Operation::SqlRead,
        ]
        .into_iter()
        .any(|op| self.authorize(address, op).is_ok());
        if !allowed {
            return Err(Error::Denied);
        }
        self.mount(address)?;
        self.transaction_mount(address.mount(), f)
    }
    /// Mount-wide native SQL without a synthetic dataset. SQL grants must be
    /// mount-scoped; closure success commits, error or unwind rolls back.
    pub fn transaction_mount<T>(
        &self,
        name: &str,
        f: impl FnOnce(&Sql<'_>) -> Result<T>,
    ) -> Result<T> {
        let legacy = self.has_mount_grant(name, Operation::Sql);
        let admin = legacy || self.has_mount_grant(name, Operation::SqlAdmin);
        let write = admin || self.has_mount_grant(name, Operation::SqlWrite);
        let read = write || self.has_mount_grant(name, Operation::SqlRead);
        if !read {
            return Err(Error::Denied);
        }
        if !component(name) {
            return Err(Error::InvalidAddress);
        }
        let mount = lock(&self.inner)?
            .mounts
            .get(name)
            .cloned()
            .ok_or(Error::UnknownMount)?;
        let _active = Active::enter(&mount)?;
        let mut mount = mount.lock().map_err(|_| Error::Poisoned)?;
        let observation = mount.backend.observation();
        let Some(backend) = mount.backend.sql() else {
            return Err(Error::UnsupportedCapability {
                capability: Capability::Sql,
            });
        };
        let connection = backend.connection();
        use std::sync::atomic::{AtomicBool, Ordering};
        let internal = Arc::new(AtomicBool::new(true));
        let permission = internal.clone();
        connection.authorizer(Some(move |context: AuthContext<'_>| match context.action {
            action if reserved_sql(action) || context.accessor.is_some_and(reserved_name) => {
                Authorization::Deny
            }
            AuthAction::Transaction { .. } | AuthAction::Savepoint { .. }
                if !permission.load(Ordering::Relaxed) =>
            {
                Authorization::Deny
            }
            // No host-selected path or connection policy may be overridden by SQL.
            AuthAction::Attach { .. } | AuthAction::Detach { .. } | AuthAction::Pragma { .. } => {
                Authorization::Deny
            }
            AuthAction::Unknown { .. } => Authorization::Deny,
            AuthAction::Function { function_name }
                if function_name.to_ascii_lowercase().starts_with("pragma_") =>
            {
                Authorization::Deny
            }
            AuthAction::Select
            | AuthAction::Read { .. }
            | AuthAction::Recursive
            | AuthAction::Function { .. } => Authorization::Allow,
            AuthAction::Insert { .. } | AuthAction::Update { .. } | AuthAction::Delete { .. }
                if write =>
            {
                Authorization::Allow
            }
            AuthAction::Transaction { .. } | AuthAction::Savepoint { .. } => Authorization::Allow,
            _ if admin => Authorization::Allow,
            _ => Authorization::Deny,
        }))?;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<_> {
            let tx = connection.transaction()?;
            internal.store(false, Ordering::Relaxed);
            // Only transaction control is restored for commit/rollback. All SQL
            // data and connection restrictions remain installed until tx ends.
            struct Internal(Arc<AtomicBool>);
            impl Drop for Internal {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::Relaxed);
                }
            }
            let internal = Internal(internal);
            let sql = Sql {
                connection: &tx,
                dirty: std::cell::Cell::new(false),
            };
            let result = f(&sql);
            let dirty = sql.dirty.get();
            drop(internal);
            let value = result?;
            tx.commit()?;
            Ok((value, dirty))
        }));
        // Also runs after failed BEGIN/COMMIT, implicit rollback and unwinding.
        let restored = connection.authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
        let result = match result {
            Ok(result) => result,
            Err(panic) => {
                drop(mount);
                std::panic::resume_unwind(panic);
            }
        };
        restored?;
        let (value, dirty) = result?;
        let wakes = if dirty {
            lock(&self.inner)?.emit(Scope::Mount(name.into()), None, observation)
        } else {
            Vec::new()
        };
        drop(mount);
        drop(_active);
        wake_all(wakes);
        Ok(value)
    }
    pub fn subscribe(&self, scope: Scope, capacity: usize) -> Result<Subscription> {
        if capacity == 0 {
            return Err(Error::InvalidCapacity);
        }
        match &scope {
            Scope::Dataset(a) => self.authorize(a, Operation::Subscribe)?,
            Scope::Mount(m) => {
                if !self
                    .grants
                    .iter()
                    .any(|g| g.scope == scope && g.operations.contains(&Operation::Subscribe))
                    || !component(m)
                {
                    return Err(Error::Denied);
                }
            }
        }
        if let Scope::Dataset(a) = &scope {
            self.mount(a)?;
        }
        let mut state = lock(&self.inner)?;
        match &scope {
            Scope::Dataset(a) => {
                let _ = a;
            }
            Scope::Mount(m) => {
                if !state.mounts.contains_key(m) {
                    return Err(Error::UnknownMount);
                }
            }
        }
        state.watches.retain(|watch| watch.strong_count() != 0);
        let watch = Arc::new(Mutex::new(Watch {
            scope,
            capacity,
            queue: VecDeque::new(),
            overflow: None,
            waker: None,
        }));
        state.watches.push(Arc::downgrade(&watch));
        Ok(Subscription {
            inner: Arc::downgrade(&self.inner),
            watch,
        })
    }
}

fn adapter_authorizer(context: AuthContext<'_>) -> Authorization {
    if context.accessor.is_some() {
        Authorization::Deny
    } else {
        Authorization::Allow
    }
}

fn reserved_name(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with("__phenix_")
}

fn reserved_sql(action: AuthAction<'_>) -> bool {
    use AuthAction::*;
    match action {
        Read {
            table_name,
            column_name,
        } => {
            reserved_name(table_name)
            // The schema SQL text can disclose reserved definitions and is also
            // used internally by ALTER TABLE to rewrite arbitrary object names.
            || (matches!(table_name.to_ascii_lowercase().as_str(), "sqlite_master" | "sqlite_schema" | "sqlite_temp_master" | "sqlite_temp_schema") && column_name.eq_ignore_ascii_case("sql"))
        }
        CreateIndex {
            index_name,
            table_name,
        }
        | CreateTempIndex {
            index_name,
            table_name,
        }
        | DropIndex {
            index_name,
            table_name,
        }
        | DropTempIndex {
            index_name,
            table_name,
        } => reserved_name(index_name) || reserved_name(table_name),
        CreateTrigger {
            trigger_name,
            table_name,
        }
        | CreateTempTrigger {
            trigger_name,
            table_name,
        }
        | DropTrigger {
            trigger_name,
            table_name,
        }
        | DropTempTrigger {
            trigger_name,
            table_name,
        } => reserved_name(trigger_name) || reserved_name(table_name),
        CreateTable { table_name }
        | CreateTempTable { table_name }
        | DropTable { table_name }
        | DropTempTable { table_name }
        | Insert { table_name }
        | Update { table_name, .. }
        | Delete { table_name }
        | Analyze { table_name }
        | CreateVtable { table_name, .. }
        | DropVtable { table_name, .. } => reserved_name(table_name),
        CreateView { view_name }
        | CreateTempView { view_name }
        | DropView { view_name }
        | DropTempView { view_name } => reserved_name(view_name),
        Reindex { index_name } => reserved_name(index_name),
        // SQLite's authorizer omits the destination name. Deny renames rather
        // than allowing an ordinary object to enter the reserved namespace.
        AlterTable { .. } => true,
        _ => false,
    }
}

/// Native parameter binding and typed row decoding, without connection escape.
pub struct Sql<'a> {
    connection: &'a Connection,
    dirty: std::cell::Cell<bool>,
}
pub struct Row<'a, 'stmt>(&'a rusqlite::Row<'stmt>);
impl Row<'_, '_> {
    pub fn get<T: FromSql>(&self, index: usize) -> Result<T> {
        Ok(self.0.get(index)?)
    }
}
impl Sql<'_> {
    pub fn execute(&self, sql: &str, params: &[&dyn ToSql]) -> Result<usize> {
        if self.connection.is_autocommit() {
            return Err(Error::TransactionEnded);
        }
        let mut statement = self.connection.prepare(sql)?;
        let dirty = !statement.readonly();
        // A statement may have effects even if stepping returns an error (RETURNING,
        // conflict policies). Conservatively invalidate if its transaction commits.
        if dirty {
            self.dirty.set(true);
        }
        let count = statement.execute(params)?;
        Ok(count)
    }
    pub fn query<T>(
        &self,
        sql: &str,
        params: &[&dyn ToSql],
        mut decode: impl FnMut(&Row<'_, '_>) -> Result<T>,
    ) -> Result<Vec<T>> {
        let mut result = Vec::new();
        self.query_each(sql, params, |row| {
            result.push(decode(row)?);
            Ok(())
        })?;
        Ok(result)
    }
    /// Visits rows incrementally without collecting them. Callback errors stop stepping.
    pub fn query_each(
        &self,
        sql: &str,
        params: &[&dyn ToSql],
        visit: impl FnMut(&Row<'_, '_>) -> Result<()>,
    ) -> Result<()> {
        self.visit(sql, params, false, visit)
    }
    /// Executes a mutating RETURNING statement and visits its rows incrementally.
    /// SQLite may buffer RETURNING internally. Propagate callback errors to roll
    /// back; catching them in the transaction callback can commit partial effects.
    pub fn returning(
        &self,
        sql: &str,
        params: &[&dyn ToSql],
        visit: impl FnMut(&Row<'_, '_>) -> Result<()>,
    ) -> Result<()> {
        self.visit(sql, params, true, visit)
    }
    fn visit(
        &self,
        sql: &str,
        params: &[&dyn ToSql],
        returning: bool,
        mut visit: impl FnMut(&Row<'_, '_>) -> Result<()>,
    ) -> Result<()> {
        if self.connection.is_autocommit() {
            return Err(Error::TransactionEnded);
        }
        let mut statement = self.connection.prepare(sql)?;
        if !statement.readonly() {
            if !returning {
                return Err(Error::Denied);
            }
            self.dirty.set(true);
        }
        let mut rows = statement.query(params)?;
        while let Some(row) = rows.next()? {
            visit(&Row(row))?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invalidation {
    pub sequence: u64,
    pub scope: Scope,
    pub key: Option<String>,
    pub observation: Observation,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    Changed(Invalidation),
    Overflow { through: u64 },
}
impl State {
    fn emit(
        &mut self,
        scope: Scope,
        key: Option<String>,
        observation: Observation,
    ) -> Vec<Arc<std::task::Waker>> {
        let mut wakes = Vec::new();
        let (scope, key) = match (&scope, observation.coverage) {
            (Scope::Dataset(address), ObservationCoverage::Mount) => {
                (Scope::Mount(address.mount.clone()), None)
            }
            _ => (scope, key),
        };
        self.sequence = self
            .sequence
            .checked_add(1)
            .expect("invalidation sequence exhausted");
        let event = Invalidation {
            sequence: self.sequence,
            scope,
            key,
            observation,
        };
        self.watches.retain(|watch| {
            let Some(watch) = watch.upgrade() else {
                return false;
            };
            let mut watch = watch.lock().unwrap_or_else(|p| p.into_inner());
            let overlaps = match (&watch.scope, &event.scope) {
                (Scope::Dataset(a), Scope::Dataset(b)) => a == b,
                (Scope::Mount(a), Scope::Mount(b)) => a == b,
                (Scope::Mount(m), Scope::Dataset(a)) | (Scope::Dataset(a), Scope::Mount(m)) => {
                    m == a.mount()
                }
            };
            if !overlaps {
                return true;
            }
            if watch.overflow.is_some() || watch.queue.len() == watch.capacity {
                watch.queue.clear();
                watch.overflow = Some(event.sequence);
            } else {
                watch.queue.push_back(Notice::Changed(event.clone()));
            }
            if let Some(waker) = &watch.waker {
                wakes.push(waker.clone());
            }
            true
        });
        wakes
    }
}
impl Drop for State {
    fn drop(&mut self) {
        let mut wakes = Vec::new();
        for watch in &self.watches {
            if let Some(watch) = watch.upgrade() {
                let mut watch = watch.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(waker) = watch.waker.take() {
                    wakes.push(waker);
                }
            }
        }
        wake_all(wakes);
    }
}
enum WakerAction {
    Wake(Arc<std::task::Waker>),
    Drop(Arc<std::task::Waker>),
}
fn wake_all(wakes: Vec<Arc<std::task::Waker>>) {
    run_wakers(wakes.into_iter().map(WakerAction::Wake));
}
fn run_wakers(actions: impl IntoIterator<Item = WakerAction>) {
    // Nested operations may still be inside another mount's transaction or
    // backend callback. Defer ownership release as well as wake callbacks.
    if ACTIVE
        .try_with(|active| !active.borrow().is_empty())
        .unwrap_or(false)
    {
        PENDING_WAKES.with(|pending| pending.borrow_mut().extend(actions));
        return;
    }
    for action in actions {
        // A faulty subscriber must not interrupt publication or teardown.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || match action {
            WakerAction::Wake(waker) => waker.wake_by_ref(),
            WakerAction::Drop(waker) => drop(waker),
        }));
    }
}
pub struct Subscription {
    inner: Weak<Mutex<State>>,
    watch: Arc<Mutex<Watch>>,
}
impl Subscription {
    /// Register notification atomically with publication, scheduling a wake if ready.
    /// Wakers run after storage locks and mutation reentrancy guards are released.
    /// Replaced wakers are also released only after enclosing mount operations end.
    /// Waker panics are isolated so publication continues for other subscribers.
    /// Final registry/session teardown also wakes subscribers, which then report closed.
    pub fn set_waker(&mut self, waker: std::task::Waker) -> Result<()> {
        let Some(_inner) = self.inner.upgrade() else {
            return Err(Error::SubscriptionClosed);
        };
        // Arc clones never invoke a custom RawWaker clone callback under locks.
        let waker = Arc::new(waker);
        let mut watch = self.watch.lock().map_err(|_| Error::Poisoned)?;
        let ready = watch.overflow.is_some() || !watch.queue.is_empty();
        let old = watch.waker.replace(waker.clone());
        drop(watch);
        run_wakers(old.into_iter().map(WakerAction::Drop));
        if ready {
            wake_all(vec![waker]);
        }
        Ok(())
    }

    /// Drains without waiting for events or locking the registry. May briefly wait
    /// for the queue mutex. Overflow means rescan the subscribed scope.
    pub fn drain(&mut self) -> Result<Vec<Notice>> {
        let Some(_inner) = self.inner.upgrade() else {
            return Err(Error::SubscriptionClosed);
        };
        let mut watch = self.watch.lock().map_err(|_| Error::Poisoned)?;
        if let Some(through) = watch.overflow.take() {
            return Ok(vec![Notice::Overflow { through }]);
        }
        Ok(watch.queue.drain(..).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct WakeFn(Box<dyn Fn() + Send + Sync>);
    impl std::task::Wake for WakeFn {
        fn wake(self: Arc<Self>) {
            (self.0)();
        }
    }

    #[test]
    fn replaced_waker_destructor_reenters_after_nested_error_unwind_and_teardown() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct DropWake(Box<dyn Fn() + Send + Sync>);
        impl std::task::Wake for DropWake {
            fn wake(self: Arc<Self>) {
                panic!("replaced waker must not be woken");
            }
        }
        impl Drop for DropWake {
            fn drop(&mut self) {
                (self.0)();
            }
        }

        for outcome in ["success", "error", "unwind"] {
            let registry = Registry::new();
            for name in ["outer", "inner"] {
                registry.mount_sqlite(name, ":memory:").unwrap();
            }
            let session = registry.session(
                ["outer", "inner"]
                    .map(|name| Grant::new(Scope::Mount(name.into()), [Operation::SqlRead])),
            );
            let observed = Registry::new();
            observed.mount_memory("main").unwrap();
            let subscriber = observed.session([Grant::new(
                Scope::Mount("main".into()),
                [Operation::Subscribe],
            )]);
            let mut sub = subscriber
                .subscribe(Scope::Mount("main".into()), 1)
                .unwrap();
            let state = Arc::downgrade(&observed.inner);
            let watch = Arc::downgrade(&sub.watch);
            let drops = Arc::new(AtomicUsize::new(0));
            let released = drops.clone();
            let reentrant = session.clone();
            sub.set_waker(
                Arc::new(DropWake(Box::new(move || {
                    // Record failures instead of panicking: callback panics are isolated.
                    let unlocked = watch.upgrade().unwrap().try_lock().is_ok();
                    let idle = ACTIVE.with(|active| active.borrow().is_empty());
                    let outer = reentrant.transaction_mount("outer", |_| Ok(()));
                    let inner = reentrant.transaction_mount("inner", |_| Ok(()));
                    if unlocked
                        && idle
                        && state.upgrade().is_none()
                        && outer.is_ok()
                        && inner.is_ok()
                    {
                        released.fetch_add(1, Ordering::SeqCst);
                    }
                })))
                .into(),
            )
            .unwrap();
            let wakes = Arc::new(AtomicUsize::new(0));
            let notified = wakes.clone();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                session.transaction_mount("outer", |_| -> Result<()> {
                    session.transaction_mount("inner", |_| {
                        sub.set_waker(
                            Arc::new(WakeFn(Box::new(move || {
                                notified.fetch_add(1, Ordering::SeqCst);
                            })))
                            .into(),
                        )?;
                        drop(subscriber);
                        drop(observed);
                        assert_eq!(drops.load(Ordering::SeqCst), 0);
                        assert_eq!(wakes.load(Ordering::SeqCst), 0);
                        Ok(())
                    })?;
                    assert_eq!(drops.load(Ordering::SeqCst), 0);
                    assert_eq!(wakes.load(Ordering::SeqCst), 0);
                    match outcome {
                        "error" => Err(Error::Codec("abort".into())),
                        "unwind" => panic!("abort"),
                        _ => Ok(()),
                    }
                })
            }));
            match outcome {
                "error" => assert!(matches!(result, Ok(Err(Error::Codec(_))))),
                "unwind" => assert!(result.is_err()),
                _ => result.unwrap().unwrap(),
            }
            assert_eq!(drops.load(Ordering::SeqCst), 1, "{outcome}");
            assert_eq!(wakes.load(Ordering::SeqCst), 1, "{outcome}");
            assert!(matches!(sub.drain(), Err(Error::SubscriptionClosed)));
        }
    }

    #[test]
    fn teardown_wakes_all_subscriptions_outside_watch_locks_despite_panics() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let registry = Registry::new();
        registry.mount_memory("main").unwrap();
        let session = registry.session([Grant::new(
            Scope::Mount("main".into()),
            [Operation::Subscribe],
        )]);
        let mut first = session.subscribe(Scope::Mount("main".into()), 1).unwrap();
        let mut second = session.subscribe(Scope::Mount("main".into()), 1).unwrap();
        first
            .set_waker(Arc::new(WakeFn(Box::new(|| panic!("bad subscriber")))).into())
            .unwrap();
        let watches = [Arc::downgrade(&first.watch), Arc::downgrade(&second.watch)];
        let mount = Arc::downgrade(&lock(&registry.inner).unwrap().mounts["main"]);
        let state = Arc::downgrade(&registry.inner);
        let woke = Arc::new(AtomicBool::new(false));
        let result = woke.clone();
        second
            .set_waker(
                Arc::new(WakeFn(Box::new(move || {
                    result.store(
                        state.upgrade().is_none()
                            && mount.upgrade().unwrap().try_lock().is_ok()
                            && watches
                                .iter()
                                .all(|w| w.upgrade().unwrap().try_lock().is_ok()),
                        Ordering::SeqCst,
                    );
                })))
                .into(),
            )
            .unwrap();
        drop(registry);
        assert!(!woke.load(Ordering::SeqCst));
        drop(session);
        assert!(woke.load(Ordering::SeqCst));
        assert!(matches!(first.drain(), Err(Error::SubscriptionClosed)));
        assert!(matches!(second.drain(), Err(Error::SubscriptionClosed)));
    }

    #[test]
    fn publication_and_ready_registration_wake_outside_all_storage_locks() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct Bytes;
        impl Codec for Bytes {
            type Value = Vec<u8>;
            fn schema(&self) -> Schema {
                Schema {
                    name: "bytes".into(),
                    revision: 1,
                    structure: "bytes".into(),
                    codec: "bytes".into(),
                }
            }
            fn encode(&self, v: &Vec<u8>) -> Result<Vec<u8>> {
                Ok(v.clone())
            }
            fn decode(&self, v: &[u8]) -> Result<Vec<u8>> {
                Ok(v.to_vec())
            }
        }
        let registry = Registry::new();
        registry.mount_sqlite("main", ":memory:").unwrap();
        registry.mount_sqlite("outer", ":memory:").unwrap();
        let address = Address::parse("store://main/values").unwrap();
        registry.register(&address, Bytes.schema()).unwrap();
        let session = registry.session([Grant::new(
            Scope::Mount("main".into()),
            [
                Operation::Write,
                Operation::Delete,
                Operation::Subscribe,
                Operation::SqlAdmin,
            ],
        )]);
        let mut sub = session.subscribe(Scope::Mount("main".into()), 8).unwrap();
        let state = Arc::downgrade(&registry.inner);
        let mount = Arc::downgrade(&lock(&registry.inner).unwrap().mounts["main"]);
        let watch = Arc::downgrade(&sub.watch);
        let outer = Arc::downgrade(&lock(&registry.inner).unwrap().mounts["outer"]);
        let wakes = Arc::new(AtomicUsize::new(0));
        let count = wakes.clone();
        let waker: std::task::Waker = Arc::new(WakeFn(Box::new(move || {
            let Some(state) = state.upgrade() else { return };
            let mount = mount.upgrade().unwrap();
            let watch = watch.upgrade().unwrap();
            let outer = outer.upgrade().unwrap();
            if state.try_lock().is_ok()
                && mount.try_lock().is_ok()
                && watch.try_lock().is_ok()
                && outer.try_lock().is_ok()
                && Active::enter(&mount).is_ok()
            {
                count.fetch_add(1, Ordering::SeqCst);
            }
        })))
        .into();
        sub.set_waker(waker.clone()).unwrap();
        session.put(&address, "a", &vec![1], &Bytes).unwrap();
        session
            .compare_exchange(&address, "b", None, &vec![2], &Bytes)
            .unwrap();
        session.delete(&address, "a").unwrap();
        session
            .transaction_mount("main", |sql| {
                sql.execute("CREATE TABLE example (value)", &[])?;
                Ok(())
            })
            .unwrap();
        sub.set_waker(waker.clone()).unwrap();
        assert_eq!(wakes.load(Ordering::SeqCst), 5);
        assert_eq!(sub.drain().unwrap().len(), 4);
        let outer_session = registry.session([Grant::new(
            Scope::Mount("outer".into()),
            [Operation::SqlAdmin],
        )]);
        outer_session
            .transaction_mount("outer", |_| {
                session.put(&address, "nested", &vec![3], &Bytes)?;
                sub.set_waker(waker)?;
                assert_eq!(wakes.load(Ordering::SeqCst), 5);
                Ok(())
            })
            .unwrap();
        assert_eq!(wakes.load(Ordering::SeqCst), 7);
        let result = outer_session.transaction_mount("outer", |_| -> Result<()> {
            session.put(&address, "error", &vec![4], &Bytes)?;
            Err(Error::Codec("abort outer transaction".into()))
        });
        assert!(matches!(result, Err(Error::Codec(_))));
        assert_eq!(wakes.load(Ordering::SeqCst), 8);
        assert!(
            std::panic::catch_unwind(|| {
                let _ = outer_session.transaction_mount("outer", |_| -> Result<()> {
                    session.put(&address, "panic", &vec![5], &Bytes)?;
                    panic!("abort outer transaction")
                });
            })
            .is_err()
        );
        assert_eq!(wakes.load(Ordering::SeqCst), 9);
    }

    #[test]
    fn cancellation_removes_queue_even_after_poison() {
        let registry = Registry::new();
        registry.mount_memory("main").unwrap();
        let session = registry.session([Grant::new(
            Scope::Mount("main".into()),
            [Operation::Subscribe],
        )]);
        let sub = session.subscribe(Scope::Mount("main".into()), 1).unwrap();
        let watch = lock(&registry.inner).unwrap().watches[0].clone();
        assert_eq!(watch.strong_count(), 1);
        drop(sub);
        assert!(watch.upgrade().is_none());
        let sub = session.subscribe(Scope::Mount("main".into()), 1).unwrap();
        assert_eq!(lock(&registry.inner).unwrap().watches.len(), 1);
        let watch = lock(&registry.inner).unwrap().watches[0].clone();
        assert_eq!(watch.strong_count(), 1);
        let _ = std::panic::catch_unwind(|| {
            let _guard = registry.inner.lock().unwrap();
            panic!("poison");
        });
        drop(sub);
        assert!(watch.upgrade().is_none());
    }
}
