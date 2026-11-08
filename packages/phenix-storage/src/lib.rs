//! Synchronous, explicitly authorized storage. See `docs/storage.md` in the repository.
//! Backends are currently a closed set (memory and SQLite), not a plugin interface.
//! Observations are invalidations, not committed row changes or a durable change log.
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schema {
    pub name: String,
    pub revision: u32,
    pub structure: String,
    pub codec: String,
}
pub trait Codec {
    type Value;
    fn schema(&self) -> Schema;
    fn encode(&self, value: &Self::Value) -> Result<Vec<u8>>;
    fn decode(&self, bytes: &[u8]) -> Result<Self::Value>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Operation {
    Read,
    Write,
    Delete,
    Sql,
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
impl Backend {
    fn observation(&self) -> Observation {
        Observation {
            source: ObservationSource::RegistryWrites,
            coverage: match self {
                Self::Memory(_) => ObservationCoverage::Key,
                Self::Sqlite(_) => ObservationCoverage::Mount,
            },
            delivery: ObservationDelivery::BoundedNonDurableWithOverflow,
        }
    }
}

enum Backend {
    Memory(BTreeMap<(String, String), Vec<u8>>),
    Sqlite(Connection),
}
struct Mount {
    backend: Backend,
    datasets: BTreeMap<String, Schema>,
}
struct Watch {
    scope: Scope,
    capacity: usize,
    queue: VecDeque<Notice>,
    overflow: Option<u64>,
}
#[derive(Default)]
struct State {
    mounts: BTreeMap<String, Mount>,
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

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }
    fn mount(&self, name: &str, backend: Backend) -> Result<()> {
        if !component(name) {
            return Err(Error::InvalidAddress);
        }
        let mut state = lock(&self.inner)?;
        if state.mounts.contains_key(name) {
            return Err(Error::DuplicateMount);
        }
        state.mounts.insert(
            name.into(),
            Mount {
                backend,
                datasets: BTreeMap::new(),
            },
        );
        Ok(())
    }
    pub fn mount_memory(&self, name: &str) -> Result<()> {
        self.mount(name, Backend::Memory(BTreeMap::new()))
    }
    /// The host owns path selection; logical addresses never select files.
    pub fn mount_sqlite(&self, name: &str, path: impl AsRef<std::path::Path>) -> Result<()> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "foreign_keys", true)?;
        self.mount(name, Backend::Sqlite(connection))
    }
    pub fn register(&self, address: &Address, schema: Schema) -> Result<()> {
        let mut state = lock(&self.inner)?;
        let mount = state
            .mounts
            .get_mut(address.mount())
            .ok_or(Error::UnknownMount)?;
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
                && (operation != Operation::Sql || matches!(&g.scope, Scope::Mount(_)))
        }) {
            Ok(())
        } else {
            Err(Error::Denied)
        }
    }
    fn mount<'a>(state: &'a State, address: &Address) -> Result<&'a Mount> {
        let mount = state
            .mounts
            .get(address.mount())
            .ok_or(Error::UnknownMount)?;
        if !mount.datasets.contains_key(address.dataset()) {
            return Err(Error::UnknownDataset);
        }
        Ok(mount)
    }
    pub fn capabilities(&self, address: &Address, operation: Operation) -> Result<Capabilities> {
        self.authorize(address, operation)?;
        let state = lock(&self.inner)?;
        let mount = Self::mount(&state, address)?;
        Ok(Capabilities {
            typed_kv: matches!(mount.backend, Backend::Memory(_)),
            sql: matches!(mount.backend, Backend::Sqlite(_)),
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
            let state = lock(&self.inner)?;
            let mount = Self::mount(&state, address)?;
            if mount.datasets[address.dataset()] != schema {
                return Err(Error::SchemaMismatch);
            }
            let Backend::Memory(values) = &mount.backend else {
                return Err(Error::UnsupportedCapability {
                    capability: Capability::TypedKv,
                });
            };
            values.get(&(address.dataset.clone(), key.into())).cloned()
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
            let state = lock(&self.inner)?;
            let mount = Self::mount(&state, address)?;
            if mount.datasets[address.dataset()] != schema {
                return Err(Error::SchemaMismatch);
            }
            if !matches!(mount.backend, Backend::Memory(_)) {
                return Err(Error::UnsupportedCapability {
                    capability: Capability::TypedKv,
                });
            }
        }
        let bytes = codec.encode(value)?;
        let mut state = lock(&self.inner)?;
        let mount = state
            .mounts
            .get_mut(address.mount())
            .ok_or(Error::UnknownMount)?;
        if mount.datasets[address.dataset()] != schema {
            return Err(Error::SchemaMismatch);
        }
        let Backend::Memory(values) = &mut mount.backend else {
            return Err(Error::UnsupportedCapability {
                capability: Capability::TypedKv,
            });
        };
        values.insert((address.dataset.clone(), key.into()), bytes);
        state.emit(Scope::Dataset(address.clone()), Some(key.into()));
        Ok(())
    }
    pub fn delete(&self, address: &Address, key: &str) -> Result<bool> {
        self.authorize(address, Operation::Delete)?;
        let mut state = lock(&self.inner)?;
        Self::mount(&state, address)?;
        let mount = state.mounts.get_mut(address.mount()).unwrap();
        let Backend::Memory(values) = &mut mount.backend else {
            return Err(Error::UnsupportedCapability {
                capability: Capability::TypedKv,
            });
        };
        let removed = values
            .remove(&(address.dataset.clone(), key.into()))
            .is_some();
        if removed {
            state.emit(Scope::Dataset(address.clone()), Some(key.into()));
        }
        Ok(removed)
    }
    /// Closure success commits; error or unwind rolls back. Do not reenter this registry.
    pub fn transaction<T>(
        &self,
        address: &Address,
        f: impl FnOnce(&Sql<'_>) -> Result<T>,
    ) -> Result<T> {
        self.authorize(address, Operation::Sql)?;
        let mut state = lock(&self.inner)?;
        Self::mount(&state, address)?;
        let mount = state.mounts.get_mut(address.mount()).unwrap();
        let Backend::Sqlite(connection) = &mut mount.backend else {
            return Err(Error::UnsupportedCapability {
                capability: Capability::Sql,
            });
        };
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
            _ => Authorization::Allow,
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
                drop(state);
                std::panic::resume_unwind(panic);
            }
        };
        let value = result?;
        tx.commit()?;
        if dirty {
            state.emit(Scope::Mount(address.mount.clone()), None);
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
        let mut state = lock(&self.inner)?;
        match &scope {
            Scope::Dataset(a) => {
                Self::mount(&state, a)?;
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
        if self.connection.is_autocommit() {
            return Err(Error::TransactionEnded);
        }
        let mut statement = self.connection.prepare(sql)?;
        if !statement.readonly() {
            return Err(Error::Denied);
        }
        let mut rows = statement.query(params)?;
        let mut result = Vec::new();
        while let Some(row) = rows.next()? {
            result.push(decode(&Row(row))?);
        }
        Ok(result)
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
    fn emit(&mut self, scope: Scope, key: Option<String>) {
        let mount = match &scope {
            Scope::Dataset(a) => a.mount(),
            Scope::Mount(m) => m,
        };
        let observation = self.mounts[mount].backend.observation();
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
            true
        });
    }
}
pub struct Subscription {
    inner: Weak<Mutex<State>>,
    watch: Arc<Mutex<Watch>>,
}
impl Subscription {
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
