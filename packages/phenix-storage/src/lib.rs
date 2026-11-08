//! Synchronous, explicitly authorized storage. See `docs/storage.md` in the repository.
//! Hosts can mount external backends through object-safe optional capabilities.
//! Observations are invalidations, not committed row changes or a durable change log.

pub mod scoped;
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
    SubscriptionClosed,
    SchemaMismatch,
    InvalidCapacity,
    TransactionEnded,
    Poisoned,
    Reentrant,
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
    pub sql: bool,
    pub invalidation: bool,
    pub observation: Observation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    TypedKv,
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
    fn get(&mut self, dataset: &str, key: &str) -> Result<Option<Vec<u8>>>;
    fn put(&mut self, dataset: &str, key: &str, value: Vec<u8>) -> Result<()>;
    fn delete(&mut self, dataset: &str, key: &str) -> Result<bool>;
}
/// Supplies a SQLite connection; the registry installs authorization and controls transactions.
pub trait SqlBackend: Send {
    fn connection(&mut self) -> &mut Connection;
}
#[derive(Default)]
pub struct MemoryBackend(BTreeMap<(String, String), Vec<u8>>);
impl Backend for MemoryBackend {
    fn typed_kv(&mut self) -> Option<&mut dyn TypedKv> {
        Some(self)
    }
    fn observation(&self) -> Observation {
        observation(ObservationCoverage::Key)
    }
}
impl TypedKv for MemoryBackend {
    fn get(&mut self, dataset: &str, key: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.0.get(&(dataset.into(), key.into())).cloned())
    }
    fn put(&mut self, dataset: &str, key: &str, value: Vec<u8>) -> Result<()> {
        self.0.insert((dataset.into(), key.into()), value);
        Ok(())
    }
    fn delete(&mut self, dataset: &str, key: &str) -> Result<bool> {
        Ok(self.0.remove(&(dataset.into(), key.into())).is_some())
    }
}
struct SqliteBackend(Connection);
impl Backend for SqliteBackend {
    fn typed_kv(&mut self) -> Option<&mut dyn TypedKv> {
        Some(self)
    }
    fn sql(&mut self) -> Option<&mut dyn SqlBackend> {
        Some(self)
    }
}
impl TypedKv for SqliteBackend {
    fn get(&mut self, dataset: &str, key: &str) -> Result<Option<Vec<u8>>> {
        use rusqlite::OptionalExtension;
        Ok(self.0.query_row(
            "SELECT value FROM __phenix_kv WHERE dataset = ?1 AND key = ?2",
            rusqlite::params![dataset, key],
            |row| row.get(0),
        ).optional()?)
    }
    fn put(&mut self, dataset: &str, key: &str, value: Vec<u8>) -> Result<()> {
        self.0.execute(
            "INSERT INTO __phenix_kv (dataset, key, value) VALUES (?1, ?2, ?3) \
             ON CONFLICT(dataset, key) DO UPDATE SET value = excluded.value",
            rusqlite::params![dataset, key, value],
        )?;
        Ok(())
    }
    fn delete(&mut self, dataset: &str, key: &str) -> Result<bool> {
        Ok(self.0.execute(
            "DELETE FROM __phenix_kv WHERE dataset = ?1 AND key = ?2",
            rusqlite::params![dataset, key],
        )? != 0)
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
    waker: Option<std::task::Waker>,
}
#[derive(Default)]
struct State {
    mounts: BTreeMap<String, Arc<Mutex<Mount>>>,
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
thread_local! {
    static ACTIVE: std::cell::RefCell<BTreeSet<usize>> = const { std::cell::RefCell::new(BTreeSet::new()) };
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
        ACTIVE.with(|active| {
            active.borrow_mut().remove(&self.0);
        });
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
        if !component(name) {
            return Err(Error::InvalidAddress);
        }
        let mut state = lock(&self.inner)?;
        if state.mounts.contains_key(name) {
            return Err(Error::DuplicateMount);
        }
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
    /// The host owns path selection; logical addresses never select files.
    pub fn mount_sqlite(&self, name: &str, path: impl AsRef<std::path::Path>) -> Result<()> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "foreign_keys", true)?;
        // Typed KV is an adapter table, not a new database engine. Each write
        // is an autocommitted SQLite statement before invalidation publication.
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS __phenix_kv (
                dataset TEXT NOT NULL,
                key TEXT NOT NULL,
                value BLOB NOT NULL,
                PRIMARY KEY (dataset, key)
            )",
        )?;
        self.mount(name, SqliteBackend(connection))
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
        let _active = Active::enter(&mount)?;
        let mut mount = mount.lock().map_err(|_| Error::Poisoned)?;
        Ok(Capabilities {
            typed_kv: mount.backend.typed_kv().is_some(),
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
        lock(&self.inner)?.emit(
            Scope::Dataset(address.clone()),
            Some(key.into()),
            mount.backend.observation(),
        );
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
        if removed {
            lock(&self.inner)?.emit(
                Scope::Dataset(address.clone()),
                Some(key.into()),
                mount.backend.observation(),
            );
        }
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
        let legacy = self.authorize(address, Operation::Sql).is_ok();
        let admin = legacy || self.authorize(address, Operation::SqlAdmin).is_ok();
        let write = admin || self.authorize(address, Operation::SqlWrite).is_ok();
        let read = write || self.authorize(address, Operation::SqlRead).is_ok();
        if !read {
            return Err(Error::Denied);
        }
        let mount = self.mount(address)?;
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
            AuthAction::Transaction { .. } | AuthAction::Savepoint { .. }
                if !permission.load(Ordering::Relaxed) =>
            {
                Authorization::Deny
            }
            // No host-selected path or connection policy may be overridden by SQL.
            AuthAction::Attach { .. } | AuthAction::Detach { .. } | AuthAction::Pragma { .. } => {
                Authorization::Deny
            }
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
        let tx = connection.transaction()?;
        internal.store(false, Ordering::Relaxed);
        // Restore internal control before Transaction::drop, including on unwind.
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
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&sql)));
        let dirty = sql.dirty.get();
        drop(internal);
        let result = match result {
            Ok(result) => result,
            Err(panic) => {
                drop(tx);
                drop(mount);
                std::panic::resume_unwind(panic);
            }
        };
        let value = result?;
        tx.commit()?;
        if dirty {
            lock(&self.inner)?.emit(Scope::Mount(address.mount.clone()), None, observation);
        }
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
    fn emit(&mut self, scope: Scope, key: Option<String>, observation: Observation) {
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
                // A faulty subscriber must not poison storage or skip later watches.
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    waker.wake_by_ref();
                }));
            }
            true
        });
    }
}
pub struct Subscription {
    inner: Weak<Mutex<State>>,
    watch: Arc<Mutex<Watch>>,
}
impl Subscription {
    /// Register notification atomically with publication, waking immediately if ready.
    /// Wakers run under storage locks and must not block or reenter storage.
    /// Waker panics are isolated so publication continues for other subscribers.
    pub fn set_waker(&mut self, waker: std::task::Waker) -> Result<()> {
        let Some(_inner) = self.inner.upgrade() else {
            return Err(Error::SubscriptionClosed);
        };
        let mut watch = self.watch.lock().map_err(|_| Error::Poisoned)?;
        watch.waker = Some(waker);
        if watch.overflow.is_some() || !watch.queue.is_empty() {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                watch.waker.as_ref().unwrap().wake_by_ref();
            }));
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
