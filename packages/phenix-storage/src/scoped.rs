//! Stable scoped resource identities over existing authorized storage sessions.
//!
//! This module does not choose database engines, issue grants or create filesystem
//! paths. A trusted host binds logical resources to provisioned physical datasets.
//! The low-level Registry/Session API remains available unchanged.
use crate::{
    Address, Codec, Operation, Revision, Revisioned, Schema, Scope, Session, Sql, Subscription,
};
use std::{collections::BTreeMap, marker::PhantomData, path::PathBuf, sync::Arc};

#[derive(Debug)]
pub enum Error {
    InvalidIdentity,
    InvalidResource,
    MissingProject,
    MissingSession,
    UnknownResource,
    DuplicateResource,
    PhysicalAlias,
    MissingHome,
    MissingRuntimeDirectory,
    InvalidBaseDirectory,
    WrongResourceKind,
    InvalidRuntimeDirectory,
    Storage(crate::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
impl From<crate::Error> for Error {
    fn from(error: crate::Error) -> Self {
        Self::Storage(error)
    }
}
pub type Result<T> = std::result::Result<T, Error>;

fn component(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
}
fn resource_name(value: &str) -> bool {
    let Some((namespace, name)) = value.split_once('/') else {
        return false;
    };
    namespace.split('.').all(component) && name.split('/').all(component)
}

/// Stable host-assigned identity, never a source directory or remote URL.
/// Distinct clones can have independent ProjectIds; renaming a workspace need
/// not change its ProjectId.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScopeId {
    User(String),
    Project(String),
    Session(String),
}
impl ScopeId {
    fn valid(&self) -> bool {
        match self {
            Self::User(id) | Self::Project(id) | Self::Session(id) => component(id),
        }
    }
}

/// Storage class controls retention and placement policy, not DB capabilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StorageClass {
    Config,
    Data,
    State,
    Cache,
    Runtime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScopeKind {
    User,
    Project,
    Session,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceKind {
    TypedKv(Schema),
    Sql,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceSpec {
    pub scope: ScopeKind,
    pub class: StorageClass,
    pub name: String,
    pub kind: ResourceKind,
}
impl ResourceSpec {
    pub fn resolve(&self, identity: &Identity) -> Result<ResourceId> {
        ResourceId::new(identity.resolve(self.scope)?, self.class, &self.name)
    }
}

#[derive(Clone, Debug)]
pub struct ResourceRequest {
    pub spec: ResourceSpec,
    pub operations: std::collections::BTreeSet<Operation>,
    pub atomic_cas: bool,
}
impl ResourceRequest {
    /// Scoped reads return revision tokens even when no CAS write was requested.
    pub fn requires_atomic_cas(&self) -> bool {
        self.atomic_cas
            || (matches!(self.spec.kind, ResourceKind::TypedKv(_))
                && self.operations.contains(&Operation::Read))
    }
    pub fn new(spec: ResourceSpec, operations: impl IntoIterator<Item = Operation>) -> Self {
        Self {
            spec,
            operations: operations.into_iter().collect(),
            atomic_cas: false,
        }
    }
    pub fn with_atomic_cas(mut self) -> Self {
        self.atomic_cas = true;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BindingTarget {
    Dataset(Address),
    SqlMount(String),
}

/// The type and physical database are intentionally absent from resource identity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResourceId {
    pub scope: ScopeId,
    pub class: StorageClass,
    pub name: String,
}
impl ResourceId {
    pub fn new(scope: ScopeId, class: StorageClass, name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        if !scope.valid() {
            return Err(Error::InvalidIdentity);
        }
        if !resource_name(&name) {
            return Err(Error::InvalidResource);
        }
        Ok(Self { scope, class, name })
    }
}

/// Host context is explicit: no process-wide current project, implicit fallback
/// or automatic sharing between project, user and session resources.
#[derive(Clone, Debug)]
pub struct Identity {
    user: String,
    project: Option<String>,
    session: Option<String>,
}
impl Identity {
    pub fn resolve(&self, scope: ScopeKind) -> Result<ScopeId> {
        Ok(match scope {
            ScopeKind::User => ScopeId::User(self.user.clone()),
            ScopeKind::Project => {
                ScopeId::Project(self.project.clone().ok_or(Error::MissingProject)?)
            }
            ScopeKind::Session => {
                ScopeId::Session(self.session.clone().ok_or(Error::MissingSession)?)
            }
        })
    }
    pub fn user(id: impl Into<String>) -> Result<Self> {
        let id = id.into();
        if !component(&id) {
            return Err(Error::InvalidIdentity);
        }
        Ok(Self {
            user: id,
            project: None,
            session: None,
        })
    }
    pub fn project(mut self, id: impl Into<String>) -> Result<Self> {
        let id = id.into();
        if !component(&id) {
            return Err(Error::InvalidIdentity);
        }
        self.project = Some(id);
        Ok(self)
    }
    pub fn session(mut self, id: impl Into<String>) -> Result<Self> {
        let id = id.into();
        if !component(&id) {
            return Err(Error::InvalidIdentity);
        }
        self.session = Some(id);
        Ok(self)
    }
}

/// Mutable trusted-host declarations. Freeze before providing scoped views to
/// plugins. Two logical identities may not alias the same physical dataset:
/// sharing must use the *same* ResourceId.
#[derive(Default)]
pub struct ResourceCatalog {
    logical: BTreeMap<ResourceId, BindingTarget>,
    physical: BTreeMap<BindingTarget, ResourceId>,
}
impl ResourceCatalog {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn bind(&mut self, resource: ResourceId, address: Address) -> Result<()> {
        self.bind_target(resource, BindingTarget::Dataset(address))
    }
    pub fn bind_target(&mut self, resource: ResourceId, target: BindingTarget) -> Result<()> {
        self.bind_target_with_sql_sharing(resource, target, false)
    }
    /// Host-only opt-in to whole-mount SQL aliases. Dataset aliases remain forbidden.
    pub fn bind_target_with_sql_sharing(
        &mut self,
        resource: ResourceId,
        target: BindingTarget,
        share_sql: bool,
    ) -> Result<()> {
        if !resource.scope.valid() {
            return Err(Error::InvalidIdentity);
        }
        if !resource_name(&resource.name) {
            return Err(Error::InvalidResource);
        }
        if self.logical.contains_key(&resource) {
            return Err(Error::DuplicateResource);
        }
        if self.physical.contains_key(&target)
            && !(share_sql && matches!(target, BindingTarget::SqlMount(_)))
        {
            return Err(Error::PhysicalAlias);
        }
        if let BindingTarget::SqlMount(mount) = &target
            && !component(mount)
        {
            return Err(Error::InvalidResource);
        }
        self.physical.insert(target.clone(), resource.clone());
        self.logical.insert(resource, target);
        Ok(())
    }
    pub fn freeze(self) -> Bindings {
        Bindings {
            entries: Arc::new(self.logical),
        }
    }
}

/// Immutable lookup shared by independent plugin instances, each carrying
/// its own restricted Session. A binding never grants storage authority.
#[derive(Clone)]
pub struct Bindings {
    entries: Arc<BTreeMap<ResourceId, BindingTarget>>,
}
impl Bindings {
    pub fn access(&self, session: Session, identity: Identity) -> Storage {
        Storage {
            bindings: self.clone(),
            session,
            identity,
        }
    }
}

pub struct Storage {
    bindings: Bindings,
    session: Session,
    identity: Identity,
}
impl Storage {
    pub fn bind<B: BindCells>(&self, cells: B) -> Result<B::Bound> {
        cells.bind(self)
    }
    pub fn user(&self) -> View<'_> {
        View {
            store: self,
            scope: ScopeId::User(self.identity.user.clone()),
        }
    }
    pub fn project(&self) -> Result<View<'_>> {
        let project = self.identity.project.clone().ok_or(Error::MissingProject)?;
        Ok(View {
            store: self,
            scope: ScopeId::Project(project),
        })
    }
    pub fn session(&self) -> Result<View<'_>> {
        let session = self.identity.session.clone().ok_or(Error::MissingSession)?;
        Ok(View {
            store: self,
            scope: ScopeId::Session(session),
        })
    }
    fn resolve(&self, resource: &ResourceId) -> Result<BindingTarget> {
        self.bindings
            .entries
            .get(resource)
            .cloned()
            .ok_or(Error::UnknownResource)
    }
}

/// Short-lived view specifying logical owner; it never changes grants.
pub struct View<'a> {
    store: &'a Storage,
    scope: ScopeId,
}
impl View<'_> {
    fn kv<T: Stored>(&self, class: StorageClass, name: &str) -> Result<Kv<T>> {
        let key = ResourceId::new(self.scope.clone(), class, name)?;
        let BindingTarget::Dataset(address) = self.store.resolve(&key)? else {
            return Err(Error::WrongResourceKind);
        };
        Ok(Kv {
            address,
            session: self.store.session.clone(),
            format: T::Format::default(),
            _value: PhantomData,
        })
    }
    pub fn config<T: Stored>(&self, name: &str) -> Result<Kv<T>> {
        self.kv(StorageClass::Config, name)
    }
    pub fn data<T: Stored>(&self, name: &str) -> Result<Kv<T>> {
        self.kv(StorageClass::Data, name)
    }
    pub fn state<T: Stored>(&self, name: &str) -> Result<Kv<T>> {
        self.kv(StorageClass::State, name)
    }
    pub fn cache<T: Stored>(&self, name: &str) -> Result<Kv<T>> {
        self.kv(StorageClass::Cache, name)
    }
    pub fn runtime<T: Stored>(&self, name: &str) -> Result<Kv<T>> {
        self.kv(StorageClass::Runtime, name)
    }
    /// SQL remains mount-scoped in the underlying Session authority.
    /// A logical resource does not turn SQL into dataset-scoped access.
    pub fn sql(&self, class: StorageClass, name: &str) -> Result<SqlResource> {
        let key = ResourceId::new(self.scope.clone(), class, name)?;
        let BindingTarget::SqlMount(mount) = self.store.resolve(&key)? else {
            return Err(Error::WrongResourceKind);
        };
        Ok(SqlResource {
            mount,
            session: self.store.session.clone(),
        })
    }
}

/// Binds a high-level Rust type to a codec without changing the existing
/// low-level Codec trait. Independently authored codecs may still interoperate
/// if the registered schema and actual wire encoding agree.
pub trait Stored: Sized {
    type Format: Codec<Value = Self> + Default;
}

/// Static singleton descriptor. Each resource uses the reserved KV key `value`.
/// No primitive codec is imposed: T must explicitly implement Stored.
pub struct CellSpec<T> {
    scope: ScopeKind,
    class: StorageClass,
    name: &'static str,
    _value: PhantomData<fn() -> T>,
}
impl<T> Copy for CellSpec<T> {}
impl<T> Clone for CellSpec<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> CellSpec<T> {
    pub const fn new(scope: ScopeKind, class: StorageClass, name: &'static str) -> Self {
        Self {
            scope,
            class,
            name,
            _value: PhantomData,
        }
    }
    pub const fn project_state(name: &'static str) -> Self {
        Self::new(ScopeKind::Project, StorageClass::State, name)
    }
    pub const fn read(self) -> CellRead<T> {
        CellRead(self)
    }
}
impl<T: Stored> CellSpec<T> {
    pub fn resource_spec(&self) -> ResourceSpec {
        ResourceSpec {
            scope: self.scope,
            class: self.class,
            name: self.name.into(),
            kind: ResourceKind::TypedKv(T::Format::default().schema()),
        }
    }
}
pub struct CellRead<T>(CellSpec<T>);

/// Kernel-independent binding adapter, implemented for tuples of arity 1-4.
pub trait BindCells {
    type Bound;
    fn bind(self, storage: &Storage) -> Result<Self::Bound>;
}
impl<T: Stored> BindCells for CellRead<T> {
    type Bound = ReadCell<T>;
    fn bind(self, storage: &Storage) -> Result<Self::Bound> {
        let spec = self.0;
        let resource = spec.resource_spec().resolve(&storage.identity)?;
        let BindingTarget::Dataset(address) = storage.resolve(&resource)? else {
            return Err(Error::WrongResourceKind);
        };
        let format = Arc::new(T::Format::default());
        let source = storage
            .session
            .read_source(address, "value".into(), format.schema())?;
        Ok(ReadCell { source, format })
    }
}
macro_rules! bind_cells {
    ($($name:ident),+) => {
        impl<$($name: BindCells),+> BindCells for ($($name,)+) {
            type Bound = ($($name::Bound,)+);
            #[allow(non_snake_case)]
            fn bind(self, storage: &Storage) -> Result<Self::Bound> {
                let ($($name,)+) = self;
                Ok(($($name.bind(storage)?,)+))
            }
        }
    };
}
bind_cells!(A);
bind_cells!(A, B);
bind_cells!(A, B, C);
bind_cells!(A, B, C, D);

pub struct ReadCell<T: Stored> {
    source: crate::snapshot::ReadSource,
    format: Arc<T::Format>,
}
impl<T: Stored> Clone for ReadCell<T> {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
            format: self.format.clone(),
        }
    }
}
impl<T: Stored> ReadCell<T> {
    pub fn source(&self) -> crate::snapshot::ReadSource {
        self.source.clone()
    }
    /// Decodes only a matching pinned registry/address/key/schema, outside locks.
    pub fn decode(&self, entry: &crate::snapshot::SnapshotEntry) -> crate::Result<Revisioned<T>> {
        self.optional()
            .decode(entry)?
            .ok_or_else(|| crate::Error::MissingInput {
                address: self.source.address().clone(),
                key: self.source.key().into(),
            })
    }
    pub fn get(&self) -> crate::Result<Revisioned<T>> {
        let snapshot = crate::snapshot::SnapshotReader::new(
            vec![self.source()],
            crate::snapshot::ReadConsistency::AtomicWithinMount,
        )?
        .read()?;
        self.decode(&snapshot.entries()[0])
    }
    pub fn optional(&self) -> OptionalReadCell<T> {
        OptionalReadCell(self.clone())
    }
    pub fn changed(&self, capacity: usize) -> crate::Result<Subscription> {
        self.source.changed(capacity)
    }
}
/// Explicit absence policy; storage and codec errors remain errors.
pub struct OptionalReadCell<T: Stored>(ReadCell<T>);
impl<T: Stored> Clone for OptionalReadCell<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T: Stored> OptionalReadCell<T> {
    pub fn source(&self) -> crate::snapshot::ReadSource {
        self.0.source()
    }
    pub fn decode(
        &self,
        entry: &crate::snapshot::SnapshotEntry,
    ) -> crate::Result<Option<Revisioned<T>>> {
        if !self.0.source.matches(entry.source()) {
            return Err(crate::Error::SnapshotEntryMismatch);
        }
        entry
            .value()
            .map(|v| {
                Ok(Revisioned {
                    value: self.0.format.decode(&v.value)?,
                    revision: v.revision.clone(),
                })
            })
            .transpose()
    }
    pub fn get(&self) -> crate::Result<Option<Revisioned<T>>> {
        let snapshot = crate::snapshot::SnapshotReader::new(
            vec![self.source()],
            crate::snapshot::ReadConsistency::AtomicWithinMount,
        )?
        .read()?;
        self.decode(&snapshot.entries()[0])
    }
}

/// Typed KV view; no extra hidden cache and no implicit provisioning.
pub struct Kv<T: Stored> {
    address: Address,
    session: Session,
    format: T::Format,
    _value: PhantomData<fn() -> T>,
}
impl<T: Stored> Kv<T> {
    pub fn get(&self, key: &str) -> crate::Result<Option<Revisioned<T>>> {
        self.session
            .get_revisioned(&self.address, key, &self.format)
    }
    pub fn compare_exchange(
        &self,
        key: &str,
        expected: Option<&Revision>,
        value: &T,
    ) -> crate::Result<Revision> {
        self.session
            .compare_exchange(&self.address, key, expected, value, &self.format)
    }
    pub fn put(&self, key: &str, value: &T) -> crate::Result<()> {
        self.session.put(&self.address, key, value, &self.format)
    }
    pub fn delete(&self, key: &str) -> crate::Result<bool> {
        self.session.delete(&self.address, key)
    }
    pub fn changed(&self, capacity: usize) -> crate::Result<Subscription> {
        self.session
            .subscribe(Scope::Dataset(self.address.clone()), capacity)
    }
}

/// Native SQL facade over the existing authorized transaction API.
pub struct SqlResource {
    mount: String,
    session: Session,
}
impl SqlResource {
    pub fn changed(&self, capacity: usize) -> crate::Result<Subscription> {
        self.session
            .subscribe(Scope::Mount(self.mount.clone()), capacity)
    }
    pub fn transaction<T>(&self, f: impl FnOnce(&Sql<'_>) -> crate::Result<T>) -> crate::Result<T> {
        self.session.transaction_mount(&self.mount, f)
    }
}

/// Host-only XDG placement policy; returns directories, never silently opens
/// a database or changes its location. Invalid relative XDG overrides are
/// ignored in favor of the HOME defaults.
#[derive(Clone)]
pub struct XdgPaths {
    pub config: PathBuf,
    pub data: PathBuf,
    pub state: PathBuf,
    pub cache: PathBuf,
    pub runtime: Option<PathBuf>,
}
impl XdgPaths {
    pub fn new(
        config: PathBuf,
        data: PathBuf,
        state: PathBuf,
        cache: PathBuf,
        runtime: Option<PathBuf>,
    ) -> Result<Self> {
        if [&config, &data, &state, &cache]
            .iter()
            .any(|p| !p.is_absolute())
            || runtime.as_ref().is_some_and(|p| !p.is_absolute())
        {
            return Err(Error::InvalidBaseDirectory);
        }
        Ok(Self {
            config,
            data,
            state,
            cache,
            runtime,
        })
    }
    pub fn from_env() -> Result<Self> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .ok_or(Error::MissingHome)?;
        let base = |name: &str, fallback: &str| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| home.join(fallback))
        };
        Self::new(
            base("XDG_CONFIG_HOME", ".config"),
            base("XDG_DATA_HOME", ".local/share"),
            base("XDG_STATE_HOME", ".local/state"),
            base("XDG_CACHE_HOME", ".cache"),
            std::env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute()),
        )
    }
    pub fn directory(&self, resource: &ResourceId) -> Result<PathBuf> {
        ResourceId::new(resource.scope.clone(), resource.class, &resource.name)?;
        let root = self.base_directory(resource.class)?.join("phenix");
        Ok(match &resource.scope {
            ScopeId::User(id) => root.join("users").join(id),
            ScopeId::Project(id) => root.join("projects").join(id),
            ScopeId::Session(id) => root.join("sessions").join(id),
        })
    }
    pub fn base_directory(&self, class: StorageClass) -> Result<&std::path::Path> {
        Ok(match class {
            StorageClass::Config => &self.config,
            StorageClass::Data => &self.data,
            StorageClass::State => &self.state,
            StorageClass::Cache => &self.cache,
            StorageClass::Runtime => self
                .runtime
                .as_ref()
                .ok_or(Error::MissingRuntimeDirectory)?,
        })
    }
    /// Validate against a trusted host-provided effective uid, never an environment guess.
    #[cfg(unix)]
    pub fn validate_runtime(&self, effective_uid: u32) -> Result<()> {
        use std::os::unix::fs::MetadataExt;
        let path = self
            .runtime
            .as_ref()
            .ok_or(Error::MissingRuntimeDirectory)?;
        let metadata =
            std::fs::symlink_metadata(path).map_err(|_| Error::InvalidRuntimeDirectory)?;
        if !metadata.is_dir()
            || metadata.uid() != effective_uid
            || metadata.mode() & 0o7777 != 0o700
        {
            return Err(Error::InvalidRuntimeDirectory);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Grant, Operation, Registry, Schema};

    struct Text(String);
    #[derive(Default)]
    struct TextCodec;
    impl Codec for TextCodec {
        type Value = Text;
        fn schema(&self) -> Schema {
            Schema {
                name: "phenix.test.text".into(),
                revision: 1,
                structure: "utf8-string".into(),
                codec: "utf8".into(),
            }
        }
        fn encode(&self, value: &Text) -> crate::Result<Vec<u8>> {
            Ok(value.0.as_bytes().to_vec())
        }
        fn decode(&self, bytes: &[u8]) -> crate::Result<Text> {
            String::from_utf8(bytes.to_vec())
                .map(Text)
                .map_err(|e| crate::Error::Codec(e.to_string()))
        }
    }
    impl Stored for Text {
        type Format = TextCodec;
    }

    #[test]
    fn scoped_sql_changes_require_mount_subscription_and_report_mutations() {
        let registry = Registry::new();
        registry.mount_sqlite("sql", ":memory:").unwrap();
        let resource = ResourceId::new(
            ScopeId::User("alice".into()),
            StorageClass::Data,
            "phenix.test/sql",
        )
        .unwrap();
        let mut catalog = ResourceCatalog::new();
        catalog
            .bind_target(resource, BindingTarget::SqlMount("sql".into()))
            .unwrap();
        let bindings = catalog.freeze();
        let identity = Identity::user("alice").unwrap();
        let writer = bindings.access(
            registry.session([Grant::new(
                Scope::Mount("sql".into()),
                [Operation::SqlAdmin],
            )]),
            identity.clone(),
        );
        let watcher = bindings.access(
            registry.session([Grant::new(
                Scope::Mount("sql".into()),
                [Operation::Subscribe],
            )]),
            identity,
        );
        let sql = writer
            .user()
            .sql(StorageClass::Data, "phenix.test/sql")
            .unwrap();
        assert!(matches!(sql.changed(4), Err(crate::Error::Denied)));
        let observed = watcher
            .user()
            .sql(StorageClass::Data, "phenix.test/sql")
            .unwrap();
        assert!(matches!(
            observed.changed(0),
            Err(crate::Error::InvalidCapacity)
        ));
        let mut changes = observed.changed(4).unwrap();
        sql.transaction(|sql| {
            sql.execute("CREATE TABLE events(value TEXT)", &[])?;
            sql.execute("INSERT INTO events VALUES (?)", &[&"changed"])?;
            Ok(())
        })
        .unwrap();
        let notices = changes.drain().unwrap();
        assert!(
            matches!(&notices[..], [crate::Notice::Changed(notice)] if notice.scope == Scope::Mount("sql".into()) && notice.key.is_none())
        );
        assert!(observed.transaction(|_| Ok(())).is_err());
    }

    #[test]
    fn resource_specs_resolve_explicit_identity_without_scope_fallback() {
        let spec = ResourceSpec {
            scope: ScopeKind::Project,
            class: StorageClass::Data,
            name: "phenix.test/records".into(),
            kind: ResourceKind::TypedKv(TextCodec.schema()),
        };
        let user = Identity::user("alice").unwrap();
        assert!(matches!(spec.resolve(&user), Err(Error::MissingProject)));
        let project = user.clone().project("clone-one").unwrap();
        let other = user.project("clone-two").unwrap();
        assert_ne!(
            spec.resolve(&project).unwrap(),
            spec.resolve(&other).unwrap()
        );
        assert!(matches!(
            project.resolve(ScopeKind::Session),
            Err(Error::MissingSession)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn runtime_validation_checks_owner_mode_and_symlinks() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
        let root = std::env::temp_dir().join(format!("phenix-runtime-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let uid = std::fs::metadata(&root).unwrap().uid();
        let paths = |runtime| {
            XdgPaths::new(
                "/tmp/config".into(),
                "/tmp/data".into(),
                "/tmp/state".into(),
                "/tmp/cache".into(),
                Some(runtime),
            )
            .unwrap()
        };
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        paths(root.clone()).validate_runtime(uid).unwrap();
        assert!(
            paths(root.clone())
                .validate_runtime(uid.wrapping_add(1))
                .is_err()
        );
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o750)).unwrap();
        assert!(paths(root.clone()).validate_runtime(uid).is_err());
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o1700)).unwrap();
        assert!(paths(root.clone()).validate_runtime(uid).is_err());
        let alias = root.with_extension("alias");
        symlink(&root, &alias).unwrap();
        assert!(paths(alias.clone()).validate_runtime(uid).is_err());
        std::fs::remove_file(alias).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn two_plugins_share_one_resource_but_not_permissions() {
        let registry = Registry::new();
        registry.mount_memory("main").unwrap();
        let address = Address::parse("store://main/settings").unwrap();
        registry.register(&address, TextCodec.schema()).unwrap();
        let resource = ResourceId::new(
            ScopeId::User("alice".into()),
            StorageClass::Config,
            "phenix.agent/settings",
        )
        .unwrap();
        let mut catalog = ResourceCatalog::new();
        catalog.bind(resource, address.clone()).unwrap();
        let bindings = catalog.freeze();
        let identity = Identity::user("alice").unwrap();
        let writer = bindings.access(
            registry.session([Grant::new(
                Scope::Dataset(address.clone()),
                [Operation::Write],
            )]),
            identity.clone(),
        );
        let reader = bindings.access(
            registry.session([Grant::new(
                Scope::Dataset(address.clone()),
                [Operation::Read],
            )]),
            identity,
        );
        writer
            .user()
            .config::<Text>("phenix.agent/settings")
            .unwrap()
            .put("default", &Text("selected".into()))
            .unwrap();
        let value = reader
            .user()
            .config::<Text>("phenix.agent/settings")
            .unwrap()
            .get("default")
            .unwrap()
            .unwrap();
        assert_eq!(value.value.0, "selected");
        assert!(
            reader
                .user()
                .config::<Text>("phenix.agent/settings")
                .unwrap()
                .put("default", &Text("denied".into()))
                .is_err()
        );
    }

    #[test]
    fn same_type_and_name_are_distinct_across_scopes() {
        let registry = Registry::new();
        registry.mount_memory("main").unwrap();
        let user = Address::parse("store://main/user").unwrap();
        let project = Address::parse("store://main/project").unwrap();
        registry.register(&user, TextCodec.schema()).unwrap();
        registry.register(&project, TextCodec.schema()).unwrap();
        let mut catalog = ResourceCatalog::new();
        catalog
            .bind(
                ResourceId::new(
                    ScopeId::User("alice".into()),
                    StorageClass::Config,
                    "phenix.agent/settings",
                )
                .unwrap(),
                user.clone(),
            )
            .unwrap();
        catalog
            .bind(
                ResourceId::new(
                    ScopeId::Project("project-1".into()),
                    StorageClass::Config,
                    "phenix.agent/settings",
                )
                .unwrap(),
                project.clone(),
            )
            .unwrap();
        let access = catalog.freeze().access(
            registry.session([Grant::new(
                Scope::Mount("main".into()),
                [Operation::Read, Operation::Write],
            )]),
            Identity::user("alice")
                .unwrap()
                .project("project-1")
                .unwrap(),
        );
        access
            .user()
            .config::<Text>("phenix.agent/settings")
            .unwrap()
            .put("a", &Text("user".into()))
            .unwrap();
        access
            .project()
            .unwrap()
            .config::<Text>("phenix.agent/settings")
            .unwrap()
            .put("a", &Text("project".into()))
            .unwrap();
        assert_eq!(
            access
                .user()
                .config::<Text>("phenix.agent/settings")
                .unwrap()
                .get("a")
                .unwrap()
                .unwrap()
                .value
                .0,
            "user"
        );
        assert_eq!(
            access
                .project()
                .unwrap()
                .config::<Text>("phenix.agent/settings")
                .unwrap()
                .get("a")
                .unwrap()
                .unwrap()
                .value
                .0,
            "project"
        );
    }

    #[test]
    fn bindings_reject_aliases_missing_scopes_and_bad_names() {
        let a = Address::parse("store://main/item").unwrap();
        let key = ResourceId::new(
            ScopeId::User("u".into()),
            StorageClass::Data,
            "phenix.model/records",
        )
        .unwrap();
        let mut catalog = ResourceCatalog::new();
        catalog.bind(key.clone(), a.clone()).unwrap();
        assert!(matches!(
            catalog.bind(key, a.clone()),
            Err(Error::DuplicateResource)
        ));
        let other = ResourceId::new(
            ScopeId::User("u".into()),
            StorageClass::Config,
            "phenix.model/records",
        )
        .unwrap();
        assert!(matches!(catalog.bind(other, a), Err(Error::PhysicalAlias)));
        assert!(
            ResourceId::new(
                ScopeId::Project("../unsafe".into()),
                StorageClass::Cache,
                "x/cache"
            )
            .is_err()
        );
        assert!(
            ResourceId::new(ScopeId::User("u".into()), StorageClass::Data, "a/../bad").is_err()
        );
        let storage = catalog
            .freeze()
            .access(Registry::new().session([]), Identity::user("u").unwrap());
        assert!(matches!(storage.project(), Err(Error::MissingProject)));
        assert!(matches!(storage.session(), Err(Error::MissingSession)));
    }

    #[test]
    fn scoped_sqlite_kv_survives_reopen_without_changing_resource_identity() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "phenix-scoped-{}-{stamp}.sqlite",
            std::process::id(),
        ));
        let address = Address::parse("store://main/settings").unwrap();
        let resource = ResourceId::new(
            ScopeId::Project("stable-repo".into()),
            StorageClass::Config,
            "phenix.agent/settings",
        )
        .unwrap();
        let open = |registry: &Registry| {
            let mut catalog = ResourceCatalog::new();
            catalog.bind(resource.clone(), address.clone()).unwrap();
            catalog.freeze().access(
                registry.session([Grant::new(
                    Scope::Dataset(address.clone()),
                    [Operation::Read, Operation::Write],
                )]),
                Identity::user("alice")
                    .unwrap()
                    .project("stable-repo")
                    .unwrap(),
            )
        };

        {
            let registry = Registry::new();
            registry.mount_sqlite("main", &path).unwrap();
            registry.register(&address, TextCodec.schema()).unwrap();
            let access = open(&registry);
            access
                .project()
                .unwrap()
                .config::<Text>("phenix.agent/settings")
                .unwrap()
                .put("default", &Text("persisted".into()))
                .unwrap();
        }

        {
            let registry = Registry::new();
            registry.mount_sqlite("main", &path).unwrap();
            let mut incompatible = TextCodec.schema();
            incompatible.revision = 2;
            assert!(matches!(
                registry.register(&address, incompatible),
                Err(crate::Error::SchemaMismatch)
            ));
            registry.register(&address, TextCodec.schema()).unwrap();
            let access = open(&registry);
            let found = access
                .project()
                .unwrap()
                .config::<Text>("phenix.agent/settings")
                .unwrap()
                .get("default")
                .unwrap()
                .unwrap();
            assert_eq!(found.value.0, "persisted");
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn xdg_placement_uses_stable_identity_not_workspace_path() {
        let xdg = XdgPaths::new(
            "/tmp/config".into(),
            "/tmp/data".into(),
            "/tmp/state".into(),
            "/tmp/cache".into(),
            None,
        )
        .unwrap();
        let project = ResourceId::new(
            ScopeId::Project("stable-id".into()),
            StorageClass::Data,
            "phenix.code/index",
        )
        .unwrap();
        let session = ResourceId::new(
            ScopeId::Session("turn-7".into()),
            StorageClass::State,
            "phenix.agent/history",
        )
        .unwrap();
        assert_eq!(
            xdg.directory(&project).unwrap(),
            PathBuf::from("/tmp/data/phenix/projects/stable-id")
        );
        assert_eq!(
            xdg.directory(&session).unwrap(),
            PathBuf::from("/tmp/state/phenix/sessions/turn-7")
        );
        assert!(matches!(
            xdg.directory(
                &ResourceId::new(
                    ScopeId::User("alice".into()),
                    StorageClass::Runtime,
                    "phenix.agent/sockets",
                )
                .unwrap()
            ),
            Err(Error::MissingRuntimeDirectory)
        ));
    }
}
