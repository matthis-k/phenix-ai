//! Optional integration between host-owned storage mounts and the existing
//! in-process plugin Registrar. No second loader, database engine or kernel API.
//!
//! ## Host provisioning
//!
//! Collect resource requirements from plugin snapshots. Choose physical placement
//! and permissions from trusted host policy. Prepare the plan before opening
//! backends, then activate it to obtain restricted per-plugin views.
//!
//! ```rust,no_run
//! use phenix_plugin::PluginSnapshot;
//! use phenix_storage::Operation;
//! use phenix_storage::scoped::{Identity, ResourceId, ScopeId, StorageClass, XdgPaths};
//! use phenix_storage_plugin::{ActivatedStorage, Error, PreparedStorage,
//!     ResourceRequirements, StoragePolicy};
//!
//! fn activate(snapshots: Vec<PluginSnapshot>) -> Result<ActivatedStorage, Error> {
//!     let identity = Identity::user("alice")?.project("stable-project-id")?;
//!     let requirements = ResourceRequirements::collect(snapshots, &identity)?;
//!     let paths = XdgPaths::from_env()?;
//!     let mut policy = StoragePolicy::xdg(&requirements, &paths)?;
//!     // XDG selects placement only. These are trusted host decisions, not copies
//!     // of plugin requests. Every other request remains denied.
//!     let settings = ResourceId::new(
//!         ScopeId::Project("stable-project-id".into()),
//!         StorageClass::Config,
//!         "phenix.agent/settings",
//!     )?;
//!     policy.resources.get_mut(&settings).ok_or(Error::MissingPlacement)?
//!         .allowed.insert(Operation::Read);
//!     policy.plugin_access.insert("settings-reader".into(),
//!         [(settings, [Operation::Read].into())].into());
//!     // SQL shared by multiple resources OR plugins additionally requires an
//!     // explicit entry in policy.shared_sql_mounts.
//!     PreparedStorage::prepare(requirements, identity, policy)?.activate()
//! }
//! ```
//!
//! The host publishes the resulting views. Old views remain usable until their
//! owners release them. Preparation and activation do not create a second plugin
//! loader or graph runtime.
use phenix_plugin::{
    CallbackError, Contribution, ContributionHandle, PluginSnapshot, lease_callback_error,
};
use phenix_storage::{
    Address, Backend, Capability, Codec, Error as StorageError, Grant, Observation, Operation,
    Registry, Schema, Scope, SqlBackend, TypedKv,
    scoped::{
        self, BindingTarget, Identity, ResourceCatalog, ResourceId, ResourceKind, ResourceRequest,
        ResourceSpec, ScopeKind, Storage, StorageClass, Stored, XdgPaths,
    },
};
use std::collections::BTreeMap;

type OpenBackend = dyn Fn() -> Result<Box<dyn Backend>, Error> + Send + Sync;

/// A configured backend factory and its declared capabilities. Both local
/// placements and plugin offers carry this same provider value.
pub struct BackendProvider {
    open: Box<OpenBackend>,
    features: Option<BackendFeatures>,
    sqlite_path: Option<std::path::PathBuf>,
}

impl BackendProvider {
    pub fn new(
        open: impl Fn() -> Result<Box<dyn Backend>, Error> + Send + Sync + 'static,
    ) -> Self {
        Self {
            open: Box::new(open),
            features: None,
            sqlite_path: None,
        }
    }

    pub fn with_features(mut self, features: BackendFeatures) -> Self {
        self.features = Some(features);
        self
    }

    pub fn memory() -> Self {
        Self::new(|| Ok(Box::new(phenix_storage::MemoryBackend::default())))
            .with_features(BackendFeatures::MEMORY)
    }

    pub fn sqlite(path: std::path::PathBuf) -> Self {
        let open_path = path.clone();
        let mut provider = Self::new(move || {
            Ok(Box::new(phenix_storage::SqliteBackend::open(&open_path)?))
        })
        .with_features(BackendFeatures::SQLITE);
        provider.sqlite_path = Some(path);
        provider
    }
}

/// A plugin offers an ordinary configured provider. Only the trusted host
/// selects it and grants storage authority. The exact revision is retained
/// through opening, failures and backend destruction.
pub struct BackendOffer {
    id: String,
    provider: BackendProvider,
}
impl BackendOffer {
    pub fn new(
        id: impl Into<String>,
        open: impl Fn() -> Result<Box<dyn Backend>, CallbackError> + Send + Sync + 'static,
    ) -> Self {
        Self::from_provider(id, BackendProvider::new(move || open().map_err(Error::Open)))
    }

    pub fn from_provider(id: impl Into<String>, provider: BackendProvider) -> Self {
        Self {
            id: id.into(),
            provider,
        }
    }

    pub fn with_features(mut self, features: BackendFeatures) -> Self {
        self.provider = self.provider.with_features(features);
        self
    }
}
impl Contribution for BackendOffer {
    const KIND: &'static str = "storage.backend";
    fn id(&self) -> &str {
        &self.id
    }
}

#[derive(Debug)]
pub enum Error {
    MissingOffer,
    MissingResourceOffer,
    SchemaConflict,
    InvalidRequest,
    MissingPlacement,
    DuplicatePlugin,
    PhysicalAlias,
    UnsafeXdgPath,
    Denied,
    UnsupportedCapability,
    Scoped(scoped::Error),
    Io(std::io::Error),
    Open(CallbackError),
    Storage(StorageError),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Scoped(cause) => Some(cause),
            Self::Io(cause) => Some(cause),
            Self::Open(cause) => Some(cause.as_ref()),
            Self::Storage(cause) => Some(cause),
            _ => None,
        }
    }
}
impl From<StorageError> for Error {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}
impl From<scoped::Error> for Error {
    fn from(error: scoped::Error) -> Self {
        Self::Scoped(error)
    }
}
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

struct LeasedBackend {
    // Field order is deliberate: backend state and its trait methods must be
    // dropped before its source contribution and implementation are released.
    backend: Box<dyn Backend>,
    _lease: ContributionHandle<BackendOffer>,
}
impl Backend for LeasedBackend {
    fn register_dataset(&mut self, dataset: &str, schema: &Schema) -> Result<(), StorageError> {
        self.backend.register_dataset(dataset, schema)
    }
    fn typed_kv(&mut self) -> Option<&mut dyn TypedKv> {
        self.backend.typed_kv()
    }
    fn sql(&mut self) -> Option<&mut dyn SqlBackend> {
        self.backend.sql()
    }
    fn observation(&self) -> Observation {
        self.backend.observation()
    }
}

/// Install one selected backend offer in a trusted host's Registry. The
/// provider's exact revision is pinned by the installed backend. No graph
/// activation, storage grant, or code unload occurs as part of this operation.
pub fn mount_offer(
    registry: &Registry,
    snapshot: &PluginSnapshot,
    offer_id: &str,
    mount_name: &str,
) -> Result<(), Error> {
    let lease = snapshot
        .get::<BackendOffer>(offer_id)
        .ok_or(Error::MissingOffer)?;
    registry.mount_with(mount_name, || {
        PreparedBackend {
            provider: ProviderRef::Leased(lease),
        }
        .open()
    })?;
    Ok(())
}

/** A declarative request for one logical resource. Independent plugins can
 * offer the same ResourceId provided their canonical Schema agrees.
 * Declaration grants no permission and never opens a database.
 */
pub struct ResourceOffer {
    id: String,
    pub request: ResourceRequest,
}
impl ResourceOffer {
    pub fn new(id: impl Into<String>, request: ResourceRequest) -> Self {
        Self {
            id: id.into(),
            request,
        }
    }
    pub fn kv<T: Stored>(
        id: impl Into<String>,
        scope: ScopeKind,
        class: StorageClass,
        name: impl Into<String>,
        operations: impl IntoIterator<Item = Operation>,
    ) -> Self {
        Self::kv_schema(
            id,
            scope,
            class,
            name,
            T::Format::default().schema(),
            operations,
        )
    }
    /// Interoperable declarations need shared wire descriptors, not shared Rust types.
    pub fn kv_schema(
        id: impl Into<String>,
        scope: ScopeKind,
        class: StorageClass,
        name: impl Into<String>,
        schema: Schema,
        operations: impl IntoIterator<Item = Operation>,
    ) -> Self {
        Self::new(
            id,
            ResourceRequest::new(
                ResourceSpec {
                    scope,
                    class,
                    name: name.into(),
                    kind: ResourceKind::TypedKv(schema),
                },
                operations,
            ),
        )
    }
    pub fn sql(
        id: impl Into<String>,
        scope: ScopeKind,
        class: StorageClass,
        name: impl Into<String>,
        operations: impl IntoIterator<Item = Operation>,
    ) -> Self {
        Self::new(
            id,
            ResourceRequest::new(
                ResourceSpec {
                    scope,
                    class,
                    name: name.into(),
                    kind: ResourceKind::Sql,
                },
                operations,
            ),
        )
    }
}
impl Contribution for ResourceOffer {
    const KIND: &'static str = "storage.resource";
    fn id(&self) -> &str {
        &self.id
    }
}

/// Owned declaration data after schema validation, with no requester code leases.
/// The trusted host supplies physical bindings, provisions datasets, and issues
/// separate Session grants.
///
/// Collection merges compatible logical resources and retains each plugin's
/// requests independently. Schema and kind conflicts, duplicate plugin names,
/// and invalid operation or capability combinations fail.
///
/// Revisioned reads and atomic batch reads require KV and Read. Compare-and-exchange
/// requires KV and Write. SQL operations require SQL. Subscribe is independent.
/// A read cell requests atomic batch support only when Read is requested.
/// Another plugin's operations cannot make an invalid declaration valid.
pub struct ResourceRequirements {
    entries: BTreeMap<ResourceId, ResourceKind>,
    requests: BTreeMap<String, Vec<(ResourceId, ResourceRequest)>>,
}
impl ResourceRequirements {
    pub fn collect(
        snapshots: impl IntoIterator<Item = PluginSnapshot>,
        identity: &Identity,
    ) -> Result<Self, Error> {
        let mut entries = BTreeMap::new();
        let mut requests = BTreeMap::new();
        for snapshot in snapshots {
            if requests.contains_key(snapshot.name()) {
                return Err(Error::DuplicatePlugin);
            }
            let mut plugin_requests = Vec::new();
            for (_, kind, id) in snapshot.keys() {
                if kind != ResourceOffer::KIND {
                    continue;
                }
                let offer = snapshot
                    .get::<ResourceOffer>(id)
                    .ok_or(Error::MissingResourceOffer)?;
                let resource = offer.request.spec.resolve(identity)?;
                let sql = matches!(offer.request.spec.kind, ResourceKind::Sql);
                if offer.request.operations.is_empty()
                    || offer.request.operations.iter().any(|op| match op {
                        Operation::Sql
                        | Operation::SqlRead
                        | Operation::SqlWrite
                        | Operation::SqlAdmin => !sql,
                        Operation::Read | Operation::Write | Operation::Delete => sql,
                        Operation::Subscribe => false,
                    })
                    || offer
                        .request
                        .required
                        .iter()
                        .any(|capability| match capability {
                            Capability::TypedKv => sql,
                            Capability::Sql => !sql,
                            Capability::RevisionedRead | Capability::AtomicReadMany => {
                                sql || !offer.request.operations.contains(&Operation::Read)
                            }
                            Capability::CompareExchange => {
                                sql || !offer.request.operations.contains(&Operation::Write)
                            }
                        })
                {
                    return Err(Error::InvalidRequest);
                }
                if let Some(previous) = entries.get(&resource) {
                    if previous != &offer.request.spec.kind {
                        return Err(Error::SchemaConflict);
                    }
                } else {
                    entries.insert(resource.clone(), offer.request.spec.kind.clone());
                }
                plugin_requests.push((resource, offer.request.clone()));
            }
            requests.insert(snapshot.name().to_owned(), plugin_requests);
        }
        Ok(Self { entries, requests })
    }
    pub fn iter(&self) -> impl Iterator<Item = (&ResourceId, &ResourceKind)> {
        self.entries.iter()
    }
    pub fn requests(&self) -> &BTreeMap<String, Vec<(ResourceId, ResourceRequest)>> {
        &self.requests
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackendFeatures {
    pub typed_kv: bool,
    pub revisioned_read: bool,
    pub atomic_read_many: bool,
    pub compare_exchange: bool,
    pub sql: bool,
    pub invalidation: bool,
}
impl BackendFeatures {
    pub const MEMORY: Self = Self {
        typed_kv: true,
        revisioned_read: true,
        atomic_read_many: true,
        compare_exchange: true,
        sql: false,
        invalidation: true,
    };
    pub const SQLITE: Self = Self {
        sql: true,
        ..Self::MEMORY
    };
    fn supports(self, capability: Capability) -> bool {
        match capability {
            Capability::TypedKv => self.typed_kv,
            Capability::RevisionedRead => self.revisioned_read,
            Capability::AtomicReadMany => self.atomic_read_many,
            Capability::CompareExchange => self.compare_exchange,
            Capability::Sql => self.sql,
        }
    }
}

pub enum BackendPlacement {
    Local(BackendProvider),
    Offer {
        snapshot: PluginSnapshot,
        id: String,
    },
}

impl BackendPlacement {
    pub fn memory() -> Self {
        Self::Local(BackendProvider::memory())
    }

    pub fn sqlite(path: std::path::PathBuf) -> Self {
        Self::Local(BackendProvider::sqlite(path))
    }

    fn prepare(self) -> Result<PreparedBackend, Error> {
        let provider = match self {
            Self::Local(provider) => ProviderRef::Local(provider),
            Self::Offer { snapshot, id } => ProviderRef::Leased(
                snapshot.get::<BackendOffer>(&id).ok_or(Error::MissingOffer)?,
            ),
        };
        Ok(PreparedBackend { provider })
    }
}

pub struct ResourceBinding {
    pub target: BindingTarget,
    pub allowed: std::collections::BTreeSet<Operation>,
}
/// Trusted host decisions, not plugin contributions. SQL mount sharing requires
/// an explicit allowlist. Reserved KV tables remain protected even on shared mounts.
///
/// Both the resource allowance and the per-plugin allowance must authorize every
/// requested operation. XDG placement leaves both allowlists empty. Approve from
/// trusted configuration or a user decision, never by copying plugin requests.
/// Activation issues only the operations requested by that plugin.
///
/// Sharing a SQL mount across resources or plugins, including KV colocation, requires
/// an explicit sharing entry. SQL exposes application tables mount-wide, subject
/// to the storage authorizer. Separate databases provide stronger isolation.
///
/// # Private XDG SQLite placement
///
/// XDG placement assigns an isolated file per resource using stable scope IDs,
/// storage class, and injectively encoded resource names. Reject canonical path
/// and inode aliases between separately named mounts. Overrides must stay inside
/// the appropriate private class root.
///
/// On Unix, create app-owned directories including `phenix` with mode 0700 and
/// databases with mode 0600. Existing private components must have exact modes and
/// effective-uid ownership. Reject symlinks, nonregular files, and hard-linked
/// databases and sidecars. Use O_NOFOLLOW for checks and precreation. Validate
/// journal, WAL, and SHM files as private regular files; new sidecars inherit
/// the database mode. Do not change existing base-directory modes or process umask.
/// A restrictive umask can cause activation to fail.
///
/// Check private paths at preparation, before opening any backend at activation,
/// during provisioning, and after opening SQLite. Runtime placement requires an
/// existing nonsymlink directory owned by the effective uid with mode 0700.
/// Check it at policy construction and activation. Reject private XDG SQLite and
/// runtime placement on non-Unix platforms. Hosts must protect bases from hostile
/// concurrent replacement. These checks are not a filesystem sandbox.
///
/// Manual placement leaves filesystem security to the host. Memory needs no file.
/// An external provider owns its own path policy.
#[derive(Default)]
pub struct StoragePolicy {
    pub mounts: BTreeMap<String, BackendPlacement>,
    pub resources: BTreeMap<ResourceId, ResourceBinding>,
    pub shared_sql_mounts: std::collections::BTreeSet<String>,
    /// Host approvals for each plugin/resource pair. Missing entries deny access;
    /// these and ResourceBinding::allowed must both approve every operation.
    pub plugin_access:
        BTreeMap<String, BTreeMap<ResourceId, std::collections::BTreeSet<Operation>>>,
    /// Host-selected XDG filesystem requirements, independent of placement generation.
    /// Manual and preset policies with the same paths receive the same checks.
    /// None leaves filesystem security to the trusted host.
    pub xdg_paths: Option<XdgPaths>,
}

impl StoragePolicy {
    /// Isolated stable SQLite files. Encoding is injective, not a platform hash.
    /// Placement only: all operation allowlists remain empty (default deny).
    /// No directories or databases are created here.
    pub fn xdg(requirements: &ResourceRequirements, paths: &XdgPaths) -> Result<Self, Error> {
        let mut policy = Self {
            xdg_paths: Some(paths.clone()),
            ..Self::default()
        };
        for (resource, kind) in requirements.iter() {
            if resource.class == StorageClass::Runtime {
                #[cfg(unix)]
                {
                    paths.validate_runtime(unsafe { libc::geteuid() })?;
                }
                #[cfg(not(unix))]
                return Err(Error::Scoped(scoped::Error::InvalidRuntimeDirectory));
            }
            let scope = match &resource.scope {
                scoped::ScopeId::User(id) => format!("u-{}-{id}", id.len()),
                scoped::ScopeId::Project(id) => format!("p-{}-{id}", id.len()),
                scoped::ScopeId::Session(id) => format!("s-{}-{id}", id.len()),
            };
            let class = match resource.class {
                StorageClass::Config => "config",
                StorageClass::Data => "data",
                StorageClass::State => "state",
                StorageClass::Cache => "cache",
                StorageClass::Runtime => "runtime",
            };
            let encoded: String = resource.name.bytes().map(|b| format!("{b:02x}")).collect();
            let mount = format!("{scope}-{class}-{encoded}");
            let path = paths
                .directory(resource)?
                .join(class)
                .join(format!("{encoded}.sqlite"));
            let target = match kind {
                ResourceKind::TypedKv(_) => {
                    BindingTarget::Dataset(Address::parse(&format!("store://{mount}/values"))?)
                }
                ResourceKind::Sql => BindingTarget::SqlMount(mount.clone()),
            };
            policy.mounts.insert(mount, BackendPlacement::sqlite(path));
            policy.resources.insert(
                resource.clone(),
                ResourceBinding {
                    target,
                    allowed: Default::default(),
                },
            );
        }
        Ok(policy)
    }
}

fn xdg_root(
    paths: &XdgPaths,
    resources: &BTreeMap<ResourceId, ResourceBinding>,
    mount: &str,
    path: &std::path::Path,
) -> Result<std::path::PathBuf, Error> {
    let mut root = None;
    for (id, binding) in resources {
        let bound_mount = match &binding.target {
            BindingTarget::Dataset(address) => address.mount(),
            BindingTarget::SqlMount(name) => name,
        };
        if bound_mount != mount {
            continue;
        }
        let candidate = paths.base_directory(id.class)?.join("phenix");
        if !path.starts_with(&candidate) || root.as_ref().is_some_and(|root| root != &candidate) {
            return Err(Error::UnsafeXdgPath);
        }
        root = Some(candidate);
    }
    root.ok_or(Error::MissingPlacement)
}

/// Check existing app-owned components without mutation during prepare. On
/// activation, create private components individually, never chmod existing data.
#[cfg(unix)]
fn private_xdg_file(
    root: &std::path::Path,
    path: &std::path::Path,
    create: bool,
) -> Result<(), Error> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
    let relative = path.strip_prefix(root).map_err(|_| Error::UnsafeXdgPath)?;
    if relative.components().next().is_none()
        || relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(Error::UnsafeXdgPath);
    }
    let uid = unsafe { libc::geteuid() };
    if create {
        // XDG base directories are host-selected, not part of our private subtree.
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(root.parent().ok_or(Error::UnsafeXdgPath)?)?;
    }
    let parent = path.parent().ok_or(Error::UnsafeXdgPath)?;
    let directories: Vec<_> = parent
        .ancestors()
        .take_while(|directory| directory.starts_with(root))
        .collect();
    for directory in directories.into_iter().rev() {
        if create {
            match std::fs::DirBuilder::new().mode(0o700).create(directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        let metadata = match std::fs::symlink_metadata(directory) {
            Ok(metadata) => metadata,
            Err(error) if !create && error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o7777 != 0o700 {
            return Err(Error::UnsafeXdgPath);
        }
    }
    if create {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
        {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    // SQLite inherits the database's 0600 mode for new journal/WAL/SHM files.
    // Existing sidecars must satisfy the same policy before opening SQLite.
    for suffix in ["", "-journal", "-wal", "-shm"] {
        let mut file = path.as_os_str().to_owned();
        file.push(suffix);
        let metadata = match std::fs::symlink_metadata(&file) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        if !metadata.is_file()
            || metadata.uid() != uid
            || metadata.mode() & 0o7777 != 0o600
            || metadata.nlink() != 1
        {
            return Err(Error::UnsafeXdgPath);
        }
        let opened = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&file)?;
        let actual = opened.metadata()?;
        if !actual.is_file()
            || actual.dev() != metadata.dev()
            || actual.ino() != metadata.ino()
            || actual.uid() != uid
            || actual.mode() & 0o7777 != 0o600
            || actual.nlink() != 1
        {
            return Err(Error::UnsafeXdgPath);
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn private_xdg_file(
    _root: &std::path::Path,
    _path: &std::path::Path,
    _create: bool,
) -> Result<(), Error> {
    Err(Error::UnsafeXdgPath)
}

/// A planned mount action, not an opened backend. Offer handles pin provider code.
pub struct PreparedBackend {
    provider: ProviderRef,
}

// This distinction is ownership, not backend behavior.
enum ProviderRef {
    Local(BackendProvider),
    Leased(ContributionHandle<BackendOffer>),
}

impl PreparedBackend {
    fn provider(&self) -> &BackendProvider {
        match &self.provider {
            ProviderRef::Local(provider) => provider,
            ProviderRef::Leased(lease) => &lease.provider,
        }
    }

    /// Provider-declared capabilities; activation verifies the opened backend.
    pub fn capabilities(&self) -> Option<BackendFeatures> {
        self.provider().features
    }

    /// Host-managed SQLite path, if declared by the provider.
    pub fn sqlite_path(&self) -> Option<&std::path::Path> {
        self.provider().sqlite_path.as_deref()
    }

    pub fn offer_id(&self) -> Option<&str> {
        match &self.provider {
            ProviderRef::Local(_) => None,
            ProviderRef::Leased(lease) => Some(lease.id()),
        }
    }

    fn open(self) -> Result<Box<dyn Backend>, Error> {
        let result = (self.provider().open)();
        match self.provider {
            ProviderRef::Local(_) => result,
            ProviderRef::Leased(lease) => {
                let backend = result.map_err(|error| match error {
                    Error::Open(cause) => Error::Open(lease_callback_error(
                        cause,
                        std::sync::Arc::new(lease.clone()),
                    )),
                    other => Error::Open(lease_callback_error(
                        Box::new(other),
                        std::sync::Arc::new(lease.clone()),
                    )),
                })?;
                Ok(Box::new(LeasedBackend {
                    backend,
                    _lease: lease,
                }))
            }
        }
    }
}
///
/// Preparation validates declarations, placement, permissions, sharing, and
/// advertised features without opening provider factories, creating directories,
/// registering datasets, or issuing sessions. After authorization, check existing
/// built-in SQLite files read-only for persisted schema conflicts. Missing files
/// and schemas are allowed. This is advisory: activation rechecks registration,
/// and preparation cannot validate stored payloads or revision-clock health.
///
/// Inspect the plan without executing it. Activation opens mounts, probes demanded
/// actual interfaces and support predicates before registering datasets or issuing
/// sessions, then creates views with exact per-plugin bindings and grants.
/// Provider probes perform no sentinel payload reads or writes.
///
/// Failure publishes no views and releases opened backends and leases. Created
/// directories, files, and durable metadata remain. Never delete existing data
/// as cleanup. This is not a filesystem or multi-database transaction.
pub struct PreparedStorage {
    requirements: ResourceRequirements,
    identity: Identity,
    resources: BTreeMap<ResourceId, ResourceBinding>,
    mounts: BTreeMap<String, PreparedBackend>,
    xdg_paths: Option<XdgPaths>,
    plugin_access: BTreeMap<String, BTreeMap<ResourceId, std::collections::BTreeSet<Operation>>>,
    shared_sql_mounts: std::collections::BTreeSet<String>,
}

// Advisory at prepare, repeated before activation effects. This does not make
// host-selected paths immune to concurrent filesystem replacement.
fn validate_sqlite_paths<'a>(
    paths: impl IntoIterator<Item = &'a std::path::Path>,
) -> Result<(), Error> {
    let mut files = std::collections::BTreeSet::new();
    #[cfg(unix)]
    let mut file_ids = std::collections::BTreeSet::new();
    for path in paths {
        if !path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(Error::PhysicalAlias);
        }
        let mut prefix = path;
        let mut suffix = Vec::new();
        while !prefix.try_exists()? {
            suffix.push(prefix.file_name().ok_or(Error::InvalidRequest)?.to_owned());
            prefix = prefix.parent().ok_or(Error::InvalidRequest)?;
        }
        let mut canonical = std::fs::canonicalize(prefix)?;
        for part in suffix.into_iter().rev() {
            canonical.push(part);
        }
        if !files.insert(canonical) {
            return Err(Error::PhysicalAlias);
        }
        #[cfg(unix)]
        if path.try_exists()? {
            use std::os::unix::fs::MetadataExt;
            let metadata = std::fs::metadata(path)?;
            if !file_ids.insert((metadata.dev(), metadata.ino())) {
                return Err(Error::PhysicalAlias);
            }
        }
    }
    Ok(())
}

impl PreparedStorage {
    /// Canonical IDs, kinds and approved per-plugin requests. Every request in a
    /// successful plan passed both host allowlists; only requested grants issue.
    pub fn requirements(&self) -> &ResourceRequirements {
        &self.requirements
    }
    pub fn bindings(&self) -> &BTreeMap<ResourceId, ResourceBinding> {
        &self.resources
    }
    /// Mount actions and their declared capabilities. No backend is opened yet.
    pub fn mounts(&self) -> &BTreeMap<String, PreparedBackend> {
        &self.mounts
    }
    pub fn plugin_access(
        &self,
    ) -> &BTreeMap<String, BTreeMap<ResourceId, std::collections::BTreeSet<Operation>>> {
        &self.plugin_access
    }
    pub fn shared_sql_mounts(&self) -> &std::collections::BTreeSet<String> {
        &self.shared_sql_mounts
    }
    /// XDG provenance retained for private path checks before and during activation.
    pub fn xdg_paths(&self) -> Option<&XdgPaths> {
        self.xdg_paths.as_ref()
    }
    /// Planned dataset registrations; SQL mounts never invent dataset metadata.
    pub fn registrations(&self) -> impl Iterator<Item = (&Address, &Schema)> {
        self.requirements
            .iter()
            .filter_map(|(id, kind)| match (kind, &self.resources[id].target) {
                (ResourceKind::TypedKv(schema), BindingTarget::Dataset(address)) => {
                    Some((address, schema))
                }
                _ => None,
            })
    }
    pub fn prepare(
        requirements: ResourceRequirements,
        identity: Identity,
        policy: StoragePolicy,
    ) -> Result<Self, Error> {
        if requirements.entries.len() != policy.resources.len() {
            return Err(Error::MissingPlacement);
        }
        let mut mounts = BTreeMap::new();
        let mut features = BTreeMap::new();
        for (name, placement) in policy.mounts {
            // Reuse canonical mount validation without allocating a registry.
            Address::parse(&format!("store://{name}/validation"))?;
            let backend = placement.prepare()?;
            let caps = backend.capabilities().ok_or(Error::UnsupportedCapability)?;
            mounts.insert(name.clone(), backend);
            features.insert(name, caps);
        }
        validate_sqlite_paths(mounts.values().filter_map(PreparedBackend::sqlite_path))?;
        let mut targets = BTreeMap::new();
        let mut users: BTreeMap<String, Vec<bool>> = BTreeMap::new();
        for (resource, kind) in requirements.iter() {
            let binding = policy
                .resources
                .get(resource)
                .ok_or(Error::MissingPlacement)?;
            let (mount, sql) = match (&binding.target, kind) {
                (BindingTarget::Dataset(address), ResourceKind::TypedKv(_)) => {
                    (address.mount(), false)
                }
                (BindingTarget::SqlMount(mount), ResourceKind::Sql) => (mount.as_str(), true),
                _ => return Err(Error::InvalidRequest),
            };
            if targets.insert(binding.target.clone(), resource).is_some()
                && !(sql && policy.shared_sql_mounts.contains(mount))
            {
                return Err(Error::PhysicalAlias);
            }
            users.entry(mount.to_owned()).or_default().push(sql);
            let caps = features.get(mount).ok_or(Error::MissingPlacement)?;
            if (sql && !caps.sql) || (!sql && !caps.typed_kv) {
                return Err(Error::UnsupportedCapability);
            }
            for (_, request) in requirements
                .requests
                .values()
                .flatten()
                .filter(|(id, _)| id == resource)
            {
                if !request.operations.is_subset(&binding.allowed) {
                    return Err(Error::Denied);
                }
                if request.required.iter().any(|cap| !caps.supports(*cap))
                    || (request.operations.contains(&Operation::Subscribe) && !caps.invalidation)
                {
                    return Err(Error::UnsupportedCapability);
                }
            }
        }
        if users.len() != mounts.len() {
            return Err(Error::MissingPlacement);
        }
        for (mount, resources) in users {
            let consumers = requirements
                .requests
                .values()
                .filter(|requests| {
                    requests
                        .iter()
                        .any(|(id, _)| match &policy.resources[id].target {
                            BindingTarget::Dataset(address) => address.mount() == mount,
                            BindingTarget::SqlMount(name) => name == &mount,
                        })
                })
                .count();
            if (resources.len() > 1 || consumers > 1)
                && resources.contains(&true)
                && !policy.shared_sql_mounts.contains(&mount)
            {
                return Err(Error::PhysicalAlias);
            }
        }
        for (plugin, requests) in &requirements.requests {
            for (id, request) in requests {
                if !policy
                    .plugin_access
                    .get(plugin)
                    .and_then(|restrictions| restrictions.get(id))
                    .is_some_and(|allowed| request.operations.is_subset(allowed))
                {
                    return Err(Error::Denied);
                }
            }
        }
        // Reject accidentally preparing one identity and activating another.
        for (id, request) in requirements.requests.values().flatten() {
            if request.spec.resolve(&identity)? != *id {
                return Err(Error::InvalidRequest);
            }
        }
        for (mount, backend) in &mounts {
            if let Some(path) = backend.sqlite_path() {
                if let Some(paths) = &policy.xdg_paths {
                    let root = xdg_root(paths, &policy.resources, mount, path)?;
                    private_xdg_file(&root, path, false)?;
                }
                if path.try_exists()? {
                    let datasets: Vec<_> = requirements
                        .iter()
                        .filter_map(|(id, kind)| match (kind, &policy.resources[id].target) {
                            (ResourceKind::TypedKv(schema), BindingTarget::Dataset(address))
                                if address.mount() == mount =>
                            {
                                Some((address.dataset().to_owned(), schema.clone()))
                            }
                            _ => None,
                        })
                        .collect();
                    Registry::validate_sqlite_datasets(path, &datasets)?;
                }
            }
        }
        Ok(Self {
            requirements,
            identity,
            resources: policy.resources,
            mounts,
            xdg_paths: policy.xdg_paths,
            plugin_access: policy.plugin_access,
            shared_sql_mounts: policy.shared_sql_mounts,
        })
    }

    /// Failure publishes no sessions. Already-opened backends are dropped with
    /// the private registry; durable files/data are deliberately not deleted.
    pub fn activate(self) -> Result<ActivatedStorage, Error> {
        validate_sqlite_paths(self.mounts.values().filter_map(PreparedBackend::sqlite_path))?;
        #[cfg(not(unix))]
        if self.xdg_paths.is_some()
            && self
                .requirements
                .iter()
                .any(|(id, _)| id.class == StorageClass::Runtime)
        {
            return Err(Error::Scoped(scoped::Error::InvalidRuntimeDirectory));
        }
        #[cfg(unix)]
        if let Some(paths) = &self.xdg_paths
            && self
                .requirements
                .iter()
                .any(|(id, _)| id.class == StorageClass::Runtime)
        {
            paths.validate_runtime(unsafe { libc::geteuid() })?;
        }
        let mut private_roots = BTreeMap::new();
        if let Some(paths) = &self.xdg_paths {
            for (mount, backend) in &self.mounts {
                if let Some(path) = backend.sqlite_path() {
                    let root = xdg_root(paths, &self.resources, mount, path)?;
                    private_xdg_file(&root, path, false)?;
                    private_roots.insert(mount.clone(), root);
                }
            }
        }
        let registry = Registry::new();
        for (name, provider) in self.mounts {
            let path = provider.sqlite_path().map(std::path::Path::to_path_buf);
            registry.mount_with(&name, || -> Result<Box<dyn Backend>, Error> {
                if let Some(path) = &path {
                    if let Some(root) = private_roots.get(&name) {
                        private_xdg_file(root, path, true)?;
                    } else if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                }
                let mut backend = provider.open()?;
                if let Some(path) = &path
                    && let Some(root) = private_roots.get(&name)
                {
                    private_xdg_file(root, path, false)?;
                }
                // All providers follow the same actual-capability verification.
                let (typed_kv, revisioned_read, atomic_read_many, compare_exchange) =
                    match backend.typed_kv() {
                        Some(kv) => (
                            true,
                            kv.supports_revisioned_reads(),
                            kv.supports_atomic_read_many(),
                            kv.supports_compare_exchange(),
                        ),
                        None => (false, false, false, false),
                    };
                let sql = backend.sql().is_some();
                let actual = BackendFeatures {
                    typed_kv,
                    revisioned_read,
                    atomic_read_many,
                    compare_exchange,
                    sql,
                    invalidation: true, // Registry writes always publish notices.
                };
                for (id, request) in self.requirements.requests.values().flatten() {
                    let target_mount = match &self.resources[id].target {
                        BindingTarget::Dataset(address) => address.mount(),
                        BindingTarget::SqlMount(mount) => mount,
                    };
                    if target_mount != name {
                        continue;
                    }
                    let supported = match request.spec.kind {
                        ResourceKind::TypedKv(_) => typed_kv,
                        ResourceKind::Sql => sql,
                    };
                    if !supported || request.required.iter().any(|cap| !actual.supports(*cap)) {
                        return Err(Error::UnsupportedCapability);
                    }
                }
                Ok(backend)
            })?;
        }
        for (id, kind) in self.requirements.iter() {
            if let (ResourceKind::TypedKv(schema), BindingTarget::Dataset(address)) =
                (kind, &self.resources[id].target)
            {
                registry.register(address, schema.clone())?;
            }
        }
        let mut views = BTreeMap::new();
        for (plugin, requests) in &self.requirements.requests {
            let mut catalog = ResourceCatalog::new();
            let mut grants = Vec::new();
            let mut bound = std::collections::BTreeSet::new();
            for (id, request) in requests {
                let target = &self.resources[id].target;
                // Prepare already approved SQL aliases. Each plugin still gets
                // only its own logical bindings and requested operations.
                if bound.insert(id.clone()) {
                    catalog.bind_target_with_sql_sharing(id.clone(), target.clone(), true)?;
                }
                let scope = match target {
                    BindingTarget::Dataset(address) => Scope::Dataset(address.clone()),
                    BindingTarget::SqlMount(mount) => Scope::Mount(mount.clone()),
                };
                grants.push(Grant::new(scope, request.operations.iter().copied()));
            }
            let session = registry.session(grants);
            views.insert(
                plugin.clone(),
                catalog.freeze().access(session, self.identity.clone()),
            );
        }
        Ok(ActivatedStorage {
            views,
            _registry: registry,
        })
    }
}

pub struct ActivatedStorage {
    pub views: BTreeMap<String, Storage>,
    _registry: Registry,
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
    use phenix_storage::{Address, Codec, Grant, MemoryBackend, Operation, Schema, Scope};

    #[derive(Clone, Debug, Default)]
    struct CauseEvents(std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>);
    impl CauseEvents {
        fn live(&self) {
            let events = self.0.lock().unwrap();
            for owner in ["definition", "provider", "code"] {
                assert!(
                    !events.contains(&owner),
                    "premature {owner} drop: {events:?}"
                );
            }
        }
        fn push(&self, event: &'static str) {
            self.0.lock().unwrap().push(event);
        }
    }
    struct CauseOwner(CauseEvents, &'static str);
    impl Drop for CauseOwner {
        fn drop(&mut self) {
            self.0.push(self.1);
        }
    }
    struct CauseProvider(CauseOwner, bool);
    impl PluginInstance for CauseProvider {
        fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
            let definition = CauseOwner(self.0.0.clone(), "definition");
            let panic = self.1;
            registrar.add(
                BackendOffer::new("broken", move || {
                    definition.0.live();
                    Err(Box::new(StorageProviderErrorSource {
                        events: definition.0.clone(),
                        _field: SourceField(definition.0.clone()),
                        panic,
                    }))
                })
                .with_features(BackendFeatures::MEMORY),
            )
        }
    }
    struct SourceField(CauseEvents);
    impl Drop for SourceField {
        fn drop(&mut self) {
            self.0.live();
            self.0.push("source-field");
        }
    }
    struct StorageProviderErrorSource {
        events: CauseEvents,
        _field: SourceField,
        panic: bool,
    }
    impl std::fmt::Debug for StorageProviderErrorSource {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            self.events.live();
            f.write_str("StorageProviderErrorSource")
        }
    }
    impl std::fmt::Display for StorageProviderErrorSource {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            self.events.live();
            f.write_str("provider opener failed")
        }
    }
    impl std::error::Error for StorageProviderErrorSource {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.events.live();
            None
        }
    }
    impl Drop for StorageProviderErrorSource {
        fn drop(&mut self) {
            self.events.live();
            self.events.push("source");
            if self.panic {
                std::panic::panic_any(SourcePayload(self.events.clone()));
            }
        }
    }
    struct SourcePayload(CauseEvents);
    impl Drop for SourcePayload {
        fn drop(&mut self) {
            self.0.live();
            self.0.push("payload");
        }
    }

    #[test]
    fn opener_causes_pin_only_the_selected_definition_provider_and_code_through_cleanup() {
        use std::error::Error as _;
        for prepared in [false, true] {
            for panic in [false, true] {
                let events = CauseEvents::default();
                let unrelated = CauseEvents::default();
                let code = CauseOwner(events.clone(), "code");
                let mut catalog = PluginCatalog::new();
                let key = catalog
                    .register("provider", move |panic| {
                        code.0.live();
                        Ok(CauseProvider(CauseOwner(code.0.clone(), "provider"), panic))
                    })
                    .unwrap();
                let other = catalog
                    .register("unrelated", |events: CauseEvents| {
                        Ok(CauseProvider(CauseOwner(events, "provider"), false))
                    })
                    .unwrap();
                let mut plugins = catalog.instance();
                let snapshot = plugins.load(&key, panic).unwrap();
                drop(plugins.load(&other, unrelated.clone()).unwrap());
                let registry = Registry::new();
                let error = if prepared {
                    let requirements = requirements(vec![("client", request([Operation::Read]))]);
                    let policy = policy(
                        &requirements,
                        BackendPlacement::Offer {
                            snapshot: snapshot.clone(),
                            id: "broken".into(),
                        },
                    );
                    PreparedStorage::prepare(requirements, Identity::user("alice").unwrap(), policy)
                        .unwrap()
                        .activate()
                        .err()
                        .unwrap()
                } else {
                    let error = mount_offer(&registry, &snapshot, "broken", "main").unwrap_err();
                    registry.mount_memory("main").unwrap();
                    error
                };
                plugins.unload(&key).unwrap();
                drop(snapshot);
                drop(plugins);
                drop(catalog);
                drop(key);
                drop(other);
                assert!(unrelated.0.lock().unwrap().contains(&"provider"));
                events.live();
                assert!(matches!(error, Error::Open(_)));
                let root = error
                    .source()
                    .unwrap()
                    .source()
                    .unwrap()
                    .downcast_ref::<StorageProviderErrorSource>()
                    .unwrap();
                assert_eq!(root.to_string(), "provider opener failed");
                assert!(root.source().is_none());
                assert!(format!("{error:?}").contains("StorageProviderErrorSource"));
                assert!(
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(error))).is_ok()
                );
                let mut expected = vec!["source", "source-field"];
                if panic {
                    expected.push("payload");
                }
                expected.extend(["definition", "provider", "code"]);
                assert_eq!(*events.0.lock().unwrap(), expected);
            }
        }
    }

    struct Requests(ResourceRequest);
    impl PluginInstance for Requests {
        fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
            registrar.add(ResourceOffer::new("settings", self.0.clone()))
        }
    }
    fn request(operations: impl IntoIterator<Item = Operation>) -> ResourceRequest {
        ResourceRequest::new(
            ResourceSpec {
                scope: ScopeKind::User,
                class: StorageClass::Config,
                name: "phenix.test/settings".into(),
                kind: ResourceKind::TypedKv(TextCodec.schema()),
            },
            operations,
        )
    }
    fn snapshots(requests: Vec<(&str, ResourceRequest)>) -> Vec<PluginSnapshot> {
        let mut catalog = PluginCatalog::new();
        let handles: Vec<_> = requests
            .into_iter()
            .map(|(name, request)| {
                (
                    catalog
                        .register(name, |request: ResourceRequest| Ok(Requests(request)))
                        .unwrap(),
                    request,
                )
            })
            .collect();
        let mut plugins = catalog.instance();
        handles
            .into_iter()
            .map(|(handle, request)| plugins.load(&handle, request).unwrap())
            .collect()
    }
    fn requirements(requests: Vec<(&str, ResourceRequest)>) -> ResourceRequirements {
        ResourceRequirements::collect(snapshots(requests), &Identity::user("alice").unwrap())
            .unwrap()
    }
    fn policy(requirements: &ResourceRequirements, placement: BackendPlacement) -> StoragePolicy {
        let mut policy = StoragePolicy::default();
        policy.mounts.insert("main".into(), placement);
        for (resource, kind) in requirements.iter() {
            let target = match kind {
                ResourceKind::TypedKv(_) => {
                    BindingTarget::Dataset(Address::parse("store://main/settings").unwrap())
                }
                ResourceKind::Sql => BindingTarget::SqlMount("main".into()),
            };
            policy.resources.insert(
                resource.clone(),
                ResourceBinding {
                    target,
                    allowed: Default::default(),
                },
            );
        }
        approve_fixture_requests(requirements, &mut policy);
        policy
    }

    #[test]
    fn standard_and_local_provider_share_preparation_and_activation() {
        for placement in [
            BackendPlacement::memory(),
            BackendPlacement::Local(
                BackendProvider::new(|| Ok(Box::new(phenix_storage::MemoryBackend::default())))
                    .with_features(BackendFeatures::MEMORY),
            ),
        ] {
            let requirements = requirements(vec![("reader", request([Operation::Read]))]);
            let policy = policy(&requirements, placement);
            let prepared =
                PreparedStorage::prepare(requirements, Identity::user("alice").unwrap(), policy)
                    .unwrap();
            assert_eq!(
                prepared.mounts()["main"].capabilities(),
                Some(BackendFeatures::MEMORY)
            );
            let active = prepared.activate().unwrap();
            assert!(active.views.contains_key("reader"));
        }
    }

    // Test host decisions, deliberately independent of requested operations.
    fn approve_fixture_requests(requirements: &ResourceRequirements, policy: &mut StoragePolicy) {
        let allowed: std::collections::BTreeSet<_> = [
            Operation::Read,
            Operation::Write,
            Operation::Delete,
            Operation::Subscribe,
            Operation::SqlRead,
            Operation::SqlAdmin,
        ]
        .into();
        for binding in policy.resources.values_mut() {
            binding.allowed = allowed.clone();
        }
        for (plugin, requests) in requirements.requests() {
            policy.plugin_access.insert(
                plugin.clone(),
                requests
                    .iter()
                    .map(|(id, _)| (id.clone(), allowed.clone()))
                    .collect(),
            );
        }
    }

    fn temporary_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "phenix-storage-plugin-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[cfg(unix)]
    #[test]
    fn xdg_creates_private_subtree_database_and_sqlite_journal() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let root = temporary_path("private");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        let paths =
            XdgPaths::new(root.clone(), root.clone(), root.clone(), root.clone(), None).unwrap();
        let mut sql = request([Operation::SqlAdmin, Operation::Subscribe]);
        sql.spec.kind = ResourceKind::Sql;
        let requirements = requirements(vec![("sql", sql)]);
        let mut policy = StoragePolicy::xdg(&requirements, &paths).unwrap();
        approve_fixture_requests(&requirements, &mut policy);
        let path = match policy.mounts.values().next().unwrap() {
            BackendPlacement::Local(provider) => provider.sqlite_path.clone().unwrap(),
            _ => unreachable!(),
        };
        let prepared =
            PreparedStorage::prepare(requirements, Identity::user("alice").unwrap(), policy)
                .unwrap();
        assert!(prepared.xdg_paths().is_some());
        assert!(!root.join("phenix").exists());
        let active = prepared.activate().unwrap();
        assert_eq!(std::fs::metadata(&root).unwrap().mode() & 0o7777, 0o755);
        for directory in path
            .parent()
            .unwrap()
            .ancestors()
            .take_while(|directory| directory.starts_with(root.join("phenix")))
        {
            let metadata = std::fs::symlink_metadata(directory).unwrap();
            assert!(metadata.is_dir());
            assert_eq!(metadata.mode() & 0o7777, 0o700);
            assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
        }
        assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o7777, 0o600);
        let sql = active.views["sql"]
            .user()
            .sql(StorageClass::Config, "phenix.test/settings")
            .unwrap();
        let mut changed = sql.changed(2).unwrap();
        sql.transaction(|sql| {
            sql.execute("CREATE TABLE private_data(value TEXT)", &[])?;
            let mut journal = path.as_os_str().to_owned();
            journal.push("-journal");
            let metadata = std::fs::symlink_metadata(journal).unwrap();
            assert!(metadata.is_file());
            assert_eq!(metadata.mode() & 0o7777, 0o600);
            assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
            Ok(())
        })
        .unwrap();
        assert_eq!(changed.drain().unwrap().len(), 1);
        private_xdg_file(&root.join("phenix"), &path, false).unwrap();
        drop(active);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn xdg_rejects_unsafe_existing_components_and_sidecars_without_repairing() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        for case in [
            "root-mode",
            "nested-mode",
            "file-mode",
            "file-symlink",
            "directory-symlink",
            "journal-mode",
            "wal-mode",
            "shm-mode",
            "wal-symlink",
            "hardlink",
        ] {
            let root = temporary_path(case);
            let paths = XdgPaths::new(root.clone(), root.clone(), root.clone(), root.clone(), None)
                .unwrap();
            let requirements = requirements(vec![("reader", request([Operation::Read]))]);
            let mut policy = StoragePolicy::xdg(&requirements, &paths).unwrap();
            approve_fixture_requests(&requirements, &mut policy);
            let path = match policy.mounts.values().next().unwrap() {
                BackendPlacement::Local(provider) => provider.sqlite_path.clone().unwrap(),
                _ => unreachable!(),
            };
            private_xdg_file(&root.join("phenix"), &path, true).unwrap();
            match case {
                "root-mode" => std::fs::set_permissions(
                    root.join("phenix"),
                    std::fs::Permissions::from_mode(0o755),
                )
                .unwrap(),
                "nested-mode" => std::fs::set_permissions(
                    path.parent().unwrap(),
                    std::fs::Permissions::from_mode(0o755),
                )
                .unwrap(),
                "file-mode" => {
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap()
                }
                "file-symlink" => {
                    std::fs::rename(&path, root.join("outside")).unwrap();
                    symlink(root.join("outside"), &path).unwrap();
                }
                "directory-symlink" => {
                    std::fs::rename(path.parent().unwrap(), root.join("moved")).unwrap();
                    symlink(root.join("moved"), path.parent().unwrap()).unwrap();
                }
                "hardlink" => std::fs::hard_link(&path, root.join("alias")).unwrap(),
                _ => {
                    let suffix = match case {
                        "journal-mode" => "-journal",
                        "shm-mode" => "-shm",
                        _ => "-wal",
                    };
                    let mut sidecar = path.as_os_str().to_owned();
                    sidecar.push(suffix);
                    if case == "wal-symlink" {
                        symlink(&path, sidecar).unwrap();
                    } else {
                        std::fs::File::create(&sidecar).unwrap();
                        std::fs::set_permissions(sidecar, std::fs::Permissions::from_mode(0o644))
                            .unwrap();
                    }
                }
            }
            assert!(
                matches!(
                    PreparedStorage::prepare(
                        requirements,
                        Identity::user("alice").unwrap(),
                        policy
                    ),
                    Err(Error::UnsafeXdgPath)
                ),
                "case {case}"
            );
            // The checker has no permission-fixing side effects.
            assert!(private_xdg_file(&root.join("phenix"), &path, false).is_err());
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn xdg_revalidates_non_runtime_paths_and_rejects_out_of_subtree_overrides() {
        use std::os::unix::fs::PermissionsExt;
        let root = temporary_path("revalidate");
        let paths =
            XdgPaths::new(root.clone(), root.clone(), root.clone(), root.clone(), None).unwrap();
        let declarations = snapshots(vec![("reader", request([Operation::Read]))]);
        let identity = Identity::user("alice").unwrap();
        let requirements = ResourceRequirements::collect(declarations.clone(), &identity).unwrap();
        let mut policy = StoragePolicy::xdg(&requirements, &paths).unwrap();
        approve_fixture_requests(&requirements, &mut policy);
        let path = match policy.mounts.values().next().unwrap() {
            BackendPlacement::Local(provider) => provider.sqlite_path.clone().unwrap(),
            _ => unreachable!(),
        };
        let prepared = PreparedStorage::prepare(requirements, identity.clone(), policy).unwrap();
        std::fs::create_dir_all(root.join("phenix")).unwrap();
        std::fs::set_permissions(root.join("phenix"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        assert!(matches!(prepared.activate(), Err(Error::UnsafeXdgPath)));
        assert!(!path.exists());
        std::fs::set_permissions(root.join("phenix"), std::fs::Permissions::from_mode(0o700))
            .unwrap();
        private_xdg_file(&root.join("phenix"), &path, true).unwrap();
        let requirements = ResourceRequirements::collect(declarations.clone(), &identity).unwrap();
        let mut policy = StoragePolicy::xdg(&requirements, &paths).unwrap();
        approve_fixture_requests(&requirements, &mut policy);
        let prepared = PreparedStorage::prepare(requirements, identity.clone(), policy).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(prepared.activate(), Err(Error::UnsafeXdgPath)));

        let requirements = ResourceRequirements::collect(declarations.clone(), &identity).unwrap();
        let mut policy = StoragePolicy::xdg(&requirements, &paths).unwrap();
        approve_fixture_requests(&requirements, &mut policy);
        *policy.mounts.values_mut().next().unwrap() =
            BackendPlacement::sqlite(root.join("manual.sqlite"));
        assert!(matches!(
            PreparedStorage::prepare(requirements, identity.clone(), policy),
            Err(Error::UnsafeXdgPath)
        ));
        assert!(!root.join("manual.sqlite").exists());

        let requirements = ResourceRequirements::collect(declarations, &identity).unwrap();
        let manual = self::policy(
            &requirements,
            BackendPlacement::sqlite(root.join("manual.sqlite")),
        );
        let declarations = snapshots(vec![("reader", request([Operation::Read]))]);
        let protected_requirements =
            ResourceRequirements::collect(declarations, &identity).unwrap();
        let mut protected = self::policy(
            &protected_requirements,
            BackendPlacement::sqlite(root.join("manual.sqlite")),
        );
        protected.xdg_paths = Some(paths.clone());
        assert!(matches!(
            PreparedStorage::prepare(protected_requirements, identity.clone(), protected),
            Err(Error::UnsafeXdgPath)
        ));
        assert!(!root.join("manual.sqlite").exists());
        let prepared = PreparedStorage::prepare(requirements, identity, manual).unwrap();
        assert!(prepared.xdg_paths().is_none());
        drop(prepared.activate().unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn prepare_checks_persisted_schemas_per_mount_after_authorization() {
        let root = temporary_path("preflight");
        std::fs::create_dir(&root).unwrap();
        let path = root.join("database.sqlite");
        {
            let registry = Registry::new();
            registry.mount_sqlite("main", &path).unwrap();
            registry
                .register(
                    &Address::parse("store://main/settings").unwrap(),
                    TextCodec.schema(),
                )
                .unwrap();
            registry
                .register(
                    &Address::parse("store://main/other").unwrap(),
                    TextCodec.schema(),
                )
                .unwrap();
        }
        let before = std::fs::read(&path).unwrap();
        let mut other = request([Operation::Read]);
        other.spec.name = "phenix.test/other".into();
        if let ResourceKind::TypedKv(schema) = &mut other.spec.kind {
            schema.revision += 1;
        }
        let declarations = snapshots(vec![
            ("reader", request([Operation::Read])),
            ("other", other),
        ]);
        let identity = Identity::user("alice").unwrap();
        for authorized in [false, true] {
            let requirements =
                ResourceRequirements::collect(declarations.clone(), &identity).unwrap();
            let mut policy = self::policy(&requirements, BackendPlacement::sqlite(path.clone()));
            for (id, binding) in &mut policy.resources {
                if id.name == "phenix.test/other" {
                    binding.target =
                        BindingTarget::Dataset(Address::parse("store://main/other").unwrap());
                }
            }
            if !authorized {
                policy.plugin_access.clear();
            }
            let result = PreparedStorage::prepare(requirements, identity.clone(), policy);
            if authorized {
                assert!(matches!(
                    result,
                    Err(Error::Storage(StorageError::SchemaMismatch))
                ));
            } else {
                assert!(matches!(result, Err(Error::Denied)));
            }
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn prepare_rejects_permissions_kinds_and_capabilities_without_opening() {
        let identity = Identity::user("alice").unwrap();
        let requirements = requirements(vec![("writer", request([Operation::Write]))]);
        let mut policy = policy(&requirements, BackendPlacement::memory());
        policy
            .resources
            .values_mut()
            .next()
            .unwrap()
            .allowed
            .clear();
        assert!(matches!(
            PreparedStorage::prepare(requirements, identity.clone(), policy),
            Err(Error::Denied)
        ));

        let mut sql = request([Operation::SqlRead]);
        sql.spec.kind = ResourceKind::Sql;
        let requirements = self::requirements(vec![("reader", sql)]);
        let policy = self::policy(&requirements, BackendPlacement::memory());
        assert!(matches!(
            PreparedStorage::prepare(requirements, identity.clone(), policy),
            Err(Error::UnsupportedCapability)
        ));

        let requirements = self::requirements(vec![("writer", request([Operation::Write]))]);
        let mut policy = self::policy(&requirements, BackendPlacement::memory());
        policy
            .plugin_access
            .insert("writer".into(), BTreeMap::new());
        assert!(matches!(
            PreparedStorage::prepare(requirements, identity, policy),
            Err(Error::Denied)
        ));
        assert!(matches!(
            ResourceRequirements::collect(
                snapshots(vec![("bad", request([Operation::SqlAdmin]))]),
                &Identity::user("alice").unwrap()
            ),
            Err(Error::InvalidRequest)
        ));
    }

    #[test]
    fn xdg_and_requests_are_placement_only_both_host_allowlists_are_required() {
        let root = std::env::temp_dir().join(format!("phenix-policy-deny-{}", std::process::id()));
        let paths = XdgPaths::new(
            root.join("config"),
            root.join("data"),
            root.join("state"),
            root.join("cache"),
            None,
        )
        .unwrap();
        let declarations = snapshots(vec![("reader", request([Operation::Read]))]);
        let identity = Identity::user("alice").unwrap();
        for (resource_ops, plugin_ops) in [
            (vec![], None),
            (vec![Operation::Read], None),
            (vec![], Some(vec![Operation::Read])),
            (vec![Operation::Read], Some(vec![])),
            (vec![Operation::Read], Some(vec![Operation::Write])),
        ] {
            let requirements =
                ResourceRequirements::collect(declarations.clone(), &identity).unwrap();
            let mut policy = StoragePolicy::xdg(&requirements, &paths).unwrap();
            assert!(
                policy
                    .resources
                    .values()
                    .all(|binding| binding.allowed.is_empty())
            );
            assert!(policy.plugin_access.is_empty());
            let (id, binding) = policy.resources.iter_mut().next().unwrap();
            binding.allowed = resource_ops.into_iter().collect();
            if let Some(operations) = plugin_ops {
                policy.plugin_access.insert(
                    "reader".into(),
                    [(id.clone(), operations.into_iter().collect())].into(),
                );
            }
            assert!(matches!(
                PreparedStorage::prepare(requirements, identity.clone(), policy),
                Err(Error::Denied)
            ));
            assert!(!root.exists());
        }
        // An explicit allowlist for another plugin cannot authorize this one.
        let requirements = ResourceRequirements::collect(declarations.clone(), &identity).unwrap();
        let mut policy = self::policy(&requirements, BackendPlacement::memory());
        let permissions = policy.plugin_access.remove("reader").unwrap();
        policy
            .plugin_access
            .insert("someone-else".into(), permissions);
        assert!(matches!(
            PreparedStorage::prepare(requirements, identity.clone(), policy),
            Err(Error::Denied)
        ));

        let requirements = ResourceRequirements::collect(declarations, &identity).unwrap();
        let mut policy = StoragePolicy::xdg(&requirements, &paths).unwrap();
        let (id, binding) = policy.resources.iter_mut().next().unwrap();
        binding.allowed = [Operation::Read].into();
        policy.plugin_access.insert(
            "reader".into(),
            [(id.clone(), [Operation::Read].into())].into(),
        );
        let prepared = PreparedStorage::prepare(requirements, identity, policy).unwrap();
        assert!(!root.exists());
        assert!(
            prepared.mounts().values().next().unwrap().sqlite_path().is_some_and(|path| path.starts_with(&root))
        );
    }

    #[test]
    fn prepared_plan_exposes_existing_data_without_opening_or_granting() {
        let requirements = requirements(vec![(
            "writer",
            request([Operation::Write]).require(Capability::CompareExchange),
        )]);
        let id = requirements.iter().next().unwrap().0.clone();
        let mut policy = policy(&requirements, BackendPlacement::memory());
        policy.resources.get_mut(&id).unwrap().allowed = [Operation::Write].into();
        policy.plugin_access.insert(
            "writer".into(),
            [(id.clone(), [Operation::Write].into())].into(),
        );
        let prepared =
            PreparedStorage::prepare(requirements, Identity::user("alice").unwrap(), policy)
                .unwrap();
        assert_eq!(
            prepared.requirements().iter().collect::<Vec<_>>(),
            vec![(&id, &ResourceKind::TypedKv(TextCodec.schema()))]
        );
        assert_eq!(
            prepared.requirements().requests()["writer"][0].1.required,
            [Capability::CompareExchange].into()
        );
        let address = Address::parse("store://main/settings").unwrap();
        assert_eq!(
            prepared.bindings()[&id].target,
            BindingTarget::Dataset(address.clone())
        );
        assert_eq!(prepared.bindings()[&id].allowed, [Operation::Write].into());
        assert_eq!(
            prepared.plugin_access()["writer"][&id],
            [Operation::Write].into()
        );
        assert_eq!(
            prepared.mounts()["main"].capabilities(),
            Some(BackendFeatures::MEMORY)
        );
        assert!(prepared.mounts()["main"].sqlite_path().is_none());
        assert!(prepared.mounts()["main"].offer_id().is_none());
        assert_eq!(
            prepared.registrations().collect::<Vec<_>>(),
            vec![(&address, &TextCodec.schema())]
        );
        assert!(prepared.shared_sql_mounts().is_empty());
        assert!(prepared.xdg_paths().is_none());
    }

    #[test]
    fn copied_request_declarations_do_not_pin_requester_code() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering::SeqCst},
        };
        struct Requester(Arc<AtomicUsize>);
        impl Drop for Requester {
            fn drop(&mut self) {
                self.0.fetch_add(1, SeqCst);
            }
        }
        impl PluginInstance for Requester {
            fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
                registrar.add(ResourceOffer::new("request", request([Operation::Read])))
            }
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let mut catalog = PluginCatalog::new();
        let handle = catalog
            .register("requester", |drops: Arc<AtomicUsize>| Ok(Requester(drops)))
            .unwrap();
        let mut plugins = catalog.instance();
        let snapshot = plugins.load(&handle, drops.clone()).unwrap();
        plugins.unload(&handle).unwrap();
        assert_eq!(drops.load(SeqCst), 0);
        let identity = Identity::user("alice").unwrap();
        let requirements = ResourceRequirements::collect([snapshot], &identity).unwrap();
        assert_eq!(drops.load(SeqCst), 1);
        let policy = policy(&requirements, BackendPlacement::memory());
        let prepared = PreparedStorage::prepare(requirements, identity, policy).unwrap();
        let active = prepared.activate().unwrap();
        assert!(active.views.contains_key("requester"));
        assert_eq!(drops.load(SeqCst), 1);
    }

    // A local type is sufficient: no resource declaration shares this type.
    struct Text(String);
    #[derive(Default)]
    struct Format;
    impl Codec for Format {
        type Value = Text;
        fn schema(&self) -> Schema {
            TextCodec.schema()
        }
        fn encode(&self, value: &Text) -> phenix_storage::Result<Vec<u8>> {
            TextCodec.encode(&value.0)
        }
        fn decode(&self, bytes: &[u8]) -> phenix_storage::Result<Text> {
            TextCodec.decode(bytes).map(Text)
        }
    }
    impl Stored for Text {
        type Format = Format;
    }

    #[test]
    fn activation_has_independent_views_and_cas_delegates() {
        let requirements = requirements(vec![
            (
                "writer",
                request([Operation::Write]).require(Capability::CompareExchange),
            ),
            (
                "reader",
                request([Operation::Read]).require(Capability::RevisionedRead),
            ),
        ]);
        assert_eq!(requirements.iter().count(), 1);
        assert_eq!(
            requirements.requests()["reader"][0].1.operations,
            [Operation::Read].into()
        );
        let policy = policy(&requirements, BackendPlacement::memory());
        let active =
            PreparedStorage::prepare(requirements, Identity::user("alice").unwrap(), policy)
                .unwrap()
                .activate()
                .unwrap();
        let writer = active.views["writer"]
            .user()
            .config::<Text>("phenix.test/settings")
            .unwrap();
        let reader = active.views["reader"]
            .user()
            .config::<Text>("phenix.test/settings")
            .unwrap();
        let revision = writer
            .compare_exchange("key", None, &Text("one".into()))
            .unwrap();
        let found = reader.get_revisioned("key").unwrap().unwrap();
        assert_eq!(found.value.0, "one");
        assert_eq!(found.revision, revision);
        assert!(reader.put("key", &Text("denied".into())).is_err());
        assert!(writer.get("key").is_err());
        writer
            .compare_exchange("key", Some(&revision), &Text("two".into()))
            .unwrap();
        assert!(
            writer
                .compare_exchange("key", Some(&revision), &Text("stale".into()))
                .is_err()
        );
    }

    #[test]
    fn sql_is_mount_only_and_sharing_requires_explicit_policy() {
        let mut first = request([Operation::SqlAdmin]);
        first.spec.kind = ResourceKind::Sql;
        let same_resource = requirements(vec![("a", first.clone()), ("b", first.clone())]);
        let isolated = policy(
            &same_resource,
            BackendPlacement::sqlite(std::env::temp_dir().join("phenix-prepare-only.sqlite")),
        );
        assert!(matches!(
            PreparedStorage::prepare(same_resource, Identity::user("alice").unwrap(), isolated),
            Err(Error::PhysicalAlias)
        ));
        let mut second = first.clone();
        second.spec.name = "phenix.test/other".into();
        let snapshots = snapshots(vec![("plugin", first.clone()), ("other", second.clone())]);
        let identity = Identity::user("alice").unwrap();
        let requirements = ResourceRequirements::collect(snapshots.clone(), &identity).unwrap();
        let policy = policy(&requirements, BackendPlacement::memory());
        assert!(matches!(
            PreparedStorage::prepare(requirements, identity.clone(), policy),
            Err(Error::UnsupportedCapability)
        ));
        let paths = XdgPaths::new(
            "/tmp/config".into(),
            "/tmp/data".into(),
            "/tmp/state".into(),
            "/tmp/cache".into(),
            None,
        )
        .unwrap();
        let requirements = ResourceRequirements::collect(snapshots.clone(), &identity).unwrap();
        let mut policy = StoragePolicy::xdg(&requirements, &paths).unwrap();
        approve_fixture_requests(&requirements, &mut policy);
        let target = policy.resources.values().next().unwrap().target.clone();
        let mount = match &target {
            BindingTarget::SqlMount(mount) => mount.clone(),
            _ => unreachable!(),
        };
        for binding in policy.resources.values_mut() {
            binding.target = target.clone();
        }
        policy.mounts.retain(|name, _| name == &mount);
        assert!(matches!(
            PreparedStorage::prepare(requirements, identity.clone(), policy),
            Err(Error::PhysicalAlias)
        ));

        let requirements = ResourceRequirements::collect(snapshots, &identity).unwrap();
        let mut policy = self::policy(
            &requirements,
            BackendPlacement::sqlite(
                std::env::temp_dir().join(format!("phenix-sql-{}.sqlite", std::process::id())),
            ),
        );
        policy.shared_sql_mounts.insert("main".into());
        let prepared = PreparedStorage::prepare(requirements, identity, policy).unwrap();
        assert!(prepared.shared_sql_mounts().contains("main"));
        assert_eq!(prepared.registrations().count(), 0);
        let active = prepared.activate().unwrap();
        active.views["plugin"]
            .user()
            .sql(StorageClass::Config, "phenix.test/settings")
            .unwrap()
            .transaction(|sql| {
                sql.execute("CREATE TABLE IF NOT EXISTS example(value TEXT)", &[])?;
                Ok(())
            })
            .unwrap();
        drop(active);
        std::fs::remove_file(
            std::env::temp_dir().join(format!("phenix-sql-{}.sqlite", std::process::id())),
        )
        .unwrap();
    }

    #[test]
    fn xdg_prepare_is_pure_and_placement_is_stable_and_isolated() {
        let root = std::env::temp_dir().join(format!("phenix-placement-{}", std::process::id()));
        let paths = XdgPaths::new(
            root.join("config"),
            root.join("data"),
            root.join("state"),
            root.join("cache"),
            None,
        )
        .unwrap();
        let declarations = snapshots(vec![("reader", request([Operation::Read]))]);
        let identity = Identity::user("alice").unwrap();
        let requirements = ResourceRequirements::collect(declarations.clone(), &identity).unwrap();
        let first = StoragePolicy::xdg(&requirements, &paths).unwrap();
        let requirements = ResourceRequirements::collect(declarations, &identity).unwrap();
        let mut second = StoragePolicy::xdg(&requirements, &paths).unwrap();
        assert_eq!(
            first.mounts.keys().collect::<Vec<_>>(),
            second.mounts.keys().collect::<Vec<_>>()
        );
        approve_fixture_requests(&requirements, &mut second);
        let prepared = PreparedStorage::prepare(requirements, identity, second).unwrap();
        assert!(!root.exists());
        drop(prepared);
        assert!(!root.exists());
    }

    #[cfg(unix)]
    #[test]
    fn xdg_runtime_is_revalidated_before_activation_side_effects() {
        use std::os::unix::fs::PermissionsExt;
        let root =
            std::env::temp_dir().join(format!("phenix-runtime-activation-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let paths = XdgPaths::new(
            root.join("config"),
            root.join("data"),
            root.join("state"),
            root.join("cache"),
            Some(root.clone()),
        )
        .unwrap();
        let mut runtime = request([Operation::Read]);
        runtime.spec.class = StorageClass::Runtime;
        let requirements = requirements(vec![("runtime", runtime)]);
        let mut policy = StoragePolicy::xdg(&requirements, &paths).unwrap();
        approve_fixture_requests(&requirements, &mut policy);
        let prepared =
            PreparedStorage::prepare(requirements, Identity::user("alice").unwrap(), policy)
                .unwrap();
        assert!(prepared.xdg_paths().is_some());
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o750)).unwrap();
        assert!(matches!(
            prepared.activate(),
            Err(Error::Scoped(scoped::Error::InvalidRuntimeDirectory))
        ));
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        std::fs::remove_dir(root).unwrap();
    }

    const PLAIN: BackendFeatures = BackendFeatures {
        typed_kv: true,
        revisioned_read: false,
        atomic_read_many: false,
        compare_exchange: false,
        sql: false,
        invalidation: true,
    };

    #[derive(Clone)]
    struct ProviderConfig {
        opens: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        backend_drops: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        provider_drops: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        fail: bool,
        features: BackendFeatures,
        actual: BackendFeatures,
        populated: bool,
        registrations: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        payloads: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }
    struct Provider(ProviderConfig);
    impl Drop for Provider {
        fn drop(&mut self) {
            self.0
                .provider_drops
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
    struct TrackedBackend {
        memory: MemoryBackend,
        config: ProviderConfig,
    }
    impl Backend for TrackedBackend {
        fn register_dataset(
            &mut self,
            dataset: &str,
            schema: &Schema,
        ) -> phenix_storage::Result<()> {
            self.config
                .registrations
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.memory.register_dataset(dataset, schema)
        }
        fn typed_kv(&mut self) -> Option<&mut dyn TypedKv> {
            if self.config.actual.typed_kv {
                Some(self)
            } else {
                None
            }
        }
    }
    impl TypedKv for TrackedBackend {
        fn supports_revisioned_reads(&self) -> bool {
            self.config.actual.revisioned_read
        }
        fn supports_atomic_read_many(&self) -> bool {
            self.config.actual.atomic_read_many
        }
        fn supports_compare_exchange(&self) -> bool {
            self.config.actual.compare_exchange
        }
        fn get_revisioned(
            &mut self,
            dataset: &str,
            key: &str,
        ) -> phenix_storage::Result<Option<phenix_storage::Revisioned<Vec<u8>>>> {
            assert!(self.config.actual.revisioned_read);
            self.config
                .payloads
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.memory.get_revisioned(dataset, key)
        }
        fn read_many(
            &mut self,
            keys: &[phenix_storage::snapshot::ReadKey<'_>],
        ) -> phenix_storage::Result<Vec<Option<phenix_storage::Revisioned<Vec<u8>>>>> {
            assert!(self.config.actual.atomic_read_many);
            self.config
                .payloads
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.memory.read_many(keys)
        }
        fn compare_exchange(
            &mut self,
            dataset: &str,
            key: &str,
            expected: Option<&phenix_storage::Revision>,
            value: Vec<u8>,
        ) -> phenix_storage::Result<phenix_storage::Revision> {
            assert!(self.config.actual.compare_exchange);
            self.config
                .payloads
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.memory.compare_exchange(dataset, key, expected, value)
        }
        fn get(&mut self, dataset: &str, key: &str) -> phenix_storage::Result<Option<Vec<u8>>> {
            self.config
                .payloads
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.memory.get(dataset, key)
        }
        fn put(&mut self, dataset: &str, key: &str, value: Vec<u8>) -> phenix_storage::Result<()> {
            self.config
                .payloads
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.memory.put(dataset, key, value)
        }
        fn delete(&mut self, dataset: &str, key: &str) -> phenix_storage::Result<bool> {
            self.config
                .payloads
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.memory.delete(dataset, key)
        }
    }
    impl Drop for TrackedBackend {
        fn drop(&mut self) {
            assert_eq!(
                self.config
                    .provider_drops
                    .load(std::sync::atomic::Ordering::SeqCst),
                0,
                "backend must drop before its provider"
            );
            self.config
                .backend_drops
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
    impl PluginInstance for Provider {
        fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
            let config = self.0.clone();
            let features = config.features;
            registrar.add(
                BackendOffer::new("tracked", move || {
                    config
                        .opens
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if config.fail {
                        return Err("intentional".into());
                    }
                    let mut memory = MemoryBackend::default();
                    if config.populated {
                        memory
                            .put("settings", "value", b"populated".to_vec())
                            .unwrap();
                    }
                    Ok(Box::new(TrackedBackend {
                        memory,
                        config: config.clone(),
                    }))
                })
                .with_features(features),
            )
        }
    }
    fn provider_config(fail: bool) -> ProviderConfig {
        ProviderConfig {
            opens: Default::default(),
            backend_drops: Default::default(),
            provider_drops: Default::default(),
            fail,
            features: BackendFeatures::MEMORY,
            actual: BackendFeatures::MEMORY,
            populated: false,
            registrations: Default::default(),
            payloads: Default::default(),
        }
    }

    fn prepared_provider(config: ProviderConfig, request: ResourceRequest) -> PreparedStorage {
        let mut catalog = PluginCatalog::new();
        let handle = catalog
            .register("provider", |config: ProviderConfig| Ok(Provider(config)))
            .unwrap();
        let mut plugins = catalog.instance();
        let snapshot = plugins.load(&handle, config.clone()).unwrap();
        let requirements = requirements(vec![("client", request)]);
        let policy = policy(
            &requirements,
            BackendPlacement::Offer {
                snapshot,
                id: "tracked".into(),
            },
        );
        let prepared =
            PreparedStorage::prepare(requirements, Identity::user("alice").unwrap(), policy)
                .unwrap();
        assert_eq!(config.opens.load(std::sync::atomic::Ordering::SeqCst), 0);
        prepared
    }

    #[test]
    fn populated_plain_scoped_reads_need_neither_revision_batch_nor_cas() {
        use std::sync::atomic::Ordering::SeqCst;
        // Undemanded advertised extras need not be implemented either.
        for features in [PLAIN, BackendFeatures::SQLITE] {
            let mut config = provider_config(false);
            config.actual = PLAIN;
            config.features = features;
            config.populated = true;
            let active = prepared_provider(config.clone(), request([Operation::Read]))
                .activate()
                .unwrap();
            assert_eq!(config.registrations.load(SeqCst), 1);
            assert_eq!(config.payloads.load(SeqCst), 0);
            let kv = active.views["client"]
                .user()
                .config::<Text>("phenix.test/settings")
                .unwrap();
            assert_eq!(kv.get("value").unwrap().unwrap().0, "populated");
            assert!(kv.get("missing").unwrap().is_none());
            assert!(matches!(
                kv.get_revisioned("value"),
                Err(StorageError::UnsupportedCapability {
                    capability: Capability::RevisionedRead,
                })
            ));
            let spec = scoped::CellSpec::<Text>::new(
                ScopeKind::User,
                StorageClass::Config,
                "phenix.test/settings",
            );
            assert!(matches!(
                active.views["client"].bind(spec.read()),
                Err(scoped::Error::Storage(
                    StorageError::UnsupportedCapability {
                        capability: Capability::AtomicReadMany
                    }
                ))
            ));
            assert_eq!(config.payloads.load(SeqCst), 2);
        }
    }

    #[test]
    fn populated_revision_only_reads_do_not_enable_atomic_cells_or_cas() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut config = provider_config(false);
        config.actual = BackendFeatures {
            revisioned_read: true,
            ..PLAIN
        };
        config.features = config.actual;
        config.populated = true;
        let active = prepared_provider(
            config.clone(),
            request([Operation::Read]).require(Capability::RevisionedRead),
        )
        .activate()
        .unwrap();
        assert_eq!(config.payloads.load(SeqCst), 0);
        let kv = active.views["client"]
            .user()
            .config::<Text>("phenix.test/settings")
            .unwrap();
        assert_eq!(
            kv.get_revisioned("value").unwrap().unwrap().value.0,
            "populated"
        );
        assert!(kv.get_revisioned("missing").unwrap().is_none());
        let spec = scoped::CellSpec::<Text>::new(
            ScopeKind::User,
            StorageClass::Config,
            "phenix.test/settings",
        );
        assert!(matches!(
            active.views["client"].bind(spec.read()),
            Err(scoped::Error::Storage(
                StorageError::UnsupportedCapability {
                    capability: Capability::AtomicReadMany
                }
            ))
        ));
        assert!(matches!(
            kv.compare_exchange("key", None, &Text("denied".into())),
            Err(StorageError::Denied)
        ));
        assert_eq!(config.payloads.load(SeqCst), 2);
    }

    #[test]
    fn batch_only_cells_and_best_effort_handle_populated_duplicates_and_absence() {
        use phenix_storage::snapshot::{ReadConsistency, SnapshotReader};
        use std::sync::atomic::Ordering::SeqCst;
        for populated in [false, true] {
            let mut config = provider_config(false);
            config.actual = BackendFeatures {
                atomic_read_many: true,
                ..PLAIN
            };
            config.features = config.actual;
            config.populated = populated;
            let spec = scoped::CellSpec::<Text>::new(
                ScopeKind::User,
                StorageClass::Config,
                "phenix.test/settings",
            );
            let request = spec.request([Operation::Read]);
            assert_eq!(request.required, [Capability::AtomicReadMany].into());
            let active = prepared_provider(config.clone(), request)
                .activate()
                .unwrap();
            let cell = active.views["client"].bind(spec.read()).unwrap();
            assert_eq!(config.payloads.load(SeqCst), 0);
            assert_eq!(
                cell.optional().get().unwrap().map(|v| v.value.0),
                populated.then(|| "populated".into())
            );
            for consistency in [
                ReadConsistency::AtomicWithinMount,
                ReadConsistency::BestEffort,
            ] {
                let before = config.payloads.load(SeqCst);
                let snapshot = SnapshotReader::new(vec![cell.source(), cell.source()], consistency)
                    .unwrap()
                    .read()
                    .unwrap();
                assert_eq!(snapshot.entries().len(), 2);
                for entry in snapshot.entries() {
                    assert_eq!(
                        cell.optional().decode(entry).unwrap().map(|v| v.value.0),
                        populated.then(|| "populated".into())
                    );
                }
                assert_eq!(config.payloads.load(SeqCst), before + 1);
            }
            let kv = active.views["client"]
                .user()
                .config::<Text>("phenix.test/settings")
                .unwrap();
            assert!(matches!(
                kv.get_revisioned("value"),
                Err(StorageError::UnsupportedCapability {
                    capability: Capability::RevisionedRead,
                })
            ));
        }
    }

    #[test]
    fn cas_only_write_authority_creates_and_updates_without_read_authority() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut config = provider_config(false);
        config.actual = BackendFeatures {
            compare_exchange: true,
            ..PLAIN
        };
        config.features = config.actual;
        let spec = scoped::CellSpec::<Text>::new(
            ScopeKind::User,
            StorageClass::Config,
            "phenix.test/settings",
        );
        let request = spec
            .request([Operation::Write])
            .require(Capability::CompareExchange);
        assert_eq!(request.required, [Capability::CompareExchange].into());
        let active = prepared_provider(config.clone(), request)
            .activate()
            .unwrap();
        assert_eq!(config.payloads.load(SeqCst), 0);
        let kv = active.views["client"]
            .user()
            .config::<Text>("phenix.test/settings")
            .unwrap();
        let first = kv
            .compare_exchange("key", None, &Text("one".into()))
            .unwrap();
        let second = kv
            .compare_exchange("key", Some(&first), &Text("two".into()))
            .unwrap();
        assert_ne!(first, second);
        assert!(matches!(kv.get("key"), Err(StorageError::Denied)));
        assert!(matches!(
            kv.get_revisioned("key"),
            Err(StorageError::Denied)
        ));
        assert_eq!(config.payloads.load(SeqCst), 2);
    }

    #[test]
    fn rejected_offer_mounts_do_not_invoke_factory() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut catalog = PluginCatalog::new();
        let handle = catalog
            .register("provider", |config: ProviderConfig| Ok(Provider(config)))
            .unwrap();
        let mut plugins = catalog.instance();
        let config = provider_config(false);
        let snapshot = plugins.load(&handle, config.clone()).unwrap();
        let registry = Registry::new();
        registry.mount_memory("main").unwrap();
        assert!(matches!(
            mount_offer(&registry, &snapshot, "tracked", "INVALID"),
            Err(Error::Storage(StorageError::InvalidAddress))
        ));
        assert_eq!(config.opens.load(SeqCst), 0);
        assert!(matches!(
            mount_offer(&registry, &snapshot, "tracked", "main"),
            Err(Error::Storage(StorageError::DuplicateMount))
        ));
        assert_eq!(config.opens.load(SeqCst), 0);
    }

    #[cfg(unix)]
    #[test]
    fn activation_rechecks_aliases_introduced_after_prepare() {
        use std::os::unix::fs::symlink;
        for case in ["symlink", "hardlink", "parent-symlink"] {
            let root = temporary_path(case);
            std::fs::create_dir(&root).unwrap();
            let first = root.join("one/database.sqlite");
            let second = root.join("two/database.sqlite");
            let mut other = request([Operation::Read]);
            other.spec.name = "phenix.test/other".into();
            let requirements =
                requirements(vec![("one", request([Operation::Read])), ("two", other)]);
            let mut policy = policy(&requirements, BackendPlacement::sqlite(first.clone()));
            policy
                .mounts
                .insert("other".into(), BackendPlacement::sqlite(second.clone()));
            policy.resources.values_mut().last().unwrap().target =
                BindingTarget::Dataset(Address::parse("store://other/settings").unwrap());
            let prepared =
                PreparedStorage::prepare(requirements, Identity::user("alice").unwrap(), policy)
                    .unwrap();
            std::fs::create_dir(root.join("one")).unwrap();
            std::fs::File::create(&first).unwrap();
            if case == "parent-symlink" {
                symlink(root.join("one"), root.join("two")).unwrap();
            } else {
                std::fs::create_dir(root.join("two")).unwrap();
                if case == "symlink" {
                    symlink(&first, &second).unwrap();
                } else {
                    std::fs::hard_link(&first, &second).unwrap();
                }
            }
            assert!(
                matches!(prepared.activate(), Err(Error::PhysicalAlias)),
                "{case}"
            );
            assert_eq!(std::fs::metadata(&first).unwrap().len(), 0);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn missing_advertised_and_false_demanded_capabilities_fail_without_payloads_or_registration() {
        use std::sync::atomic::Ordering::SeqCst;
        for (capability, operation) in [
            (Capability::TypedKv, Operation::Read),
            (Capability::RevisionedRead, Operation::Read),
            (Capability::AtomicReadMany, Operation::Read),
            (Capability::CompareExchange, Operation::Write),
            (Capability::Sql, Operation::SqlRead),
        ] {
            for advertised in [false, true] {
                let mut catalog = PluginCatalog::new();
                let handle = catalog
                    .register("provider", |config: ProviderConfig| Ok(Provider(config)))
                    .unwrap();
                let mut plugins = catalog.instance();
                let mut config = provider_config(false);
                config.features = BackendFeatures::SQLITE;
                let mut unsupported = BackendFeatures::SQLITE;
                match capability {
                    Capability::TypedKv => unsupported.typed_kv = false,
                    Capability::RevisionedRead => unsupported.revisioned_read = false,
                    Capability::AtomicReadMany => unsupported.atomic_read_many = false,
                    Capability::CompareExchange => unsupported.compare_exchange = false,
                    Capability::Sql => unsupported.sql = false,
                }
                if advertised {
                    config.actual = unsupported;
                } else {
                    config.features = unsupported;
                }
                let snapshot = plugins.load(&handle, config.clone()).unwrap();
                let mut request = request([operation]).require(capability).require(capability);
                assert_eq!(request.required.len(), 1);
                if capability == Capability::Sql {
                    request.spec.kind = ResourceKind::Sql;
                }
                let requirements = requirements(vec![("client", request)]);
                let policy = policy(
                    &requirements,
                    BackendPlacement::Offer {
                        snapshot,
                        id: "tracked".into(),
                    },
                );
                let prepared = PreparedStorage::prepare(
                    requirements,
                    Identity::user("alice").unwrap(),
                    policy,
                );
                assert_eq!(config.opens.load(SeqCst), 0);
                if advertised {
                    assert!(
                        matches!(
                            prepared.unwrap().activate(),
                            Err(Error::UnsupportedCapability)
                        ),
                        "{capability:?}"
                    );
                    assert_eq!(config.opens.load(SeqCst), 1);
                    assert_eq!(config.backend_drops.load(SeqCst), 1);
                } else {
                    assert!(
                        matches!(prepared, Err(Error::UnsupportedCapability)),
                        "{capability:?}"
                    );
                    assert_eq!(config.opens.load(SeqCst), 0);
                }
                assert_eq!(config.registrations.load(SeqCst), 0);
                assert_eq!(config.payloads.load(SeqCst), 0);
                plugins.unload(&handle).unwrap();
                assert_eq!(config.provider_drops.load(SeqCst), 1);
            }
        }
    }

    #[test]
    fn invalid_requirements_cannot_borrow_kind_or_authority_from_another_request() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut catalog = PluginCatalog::new();
        let handle = catalog
            .register("provider", |config: ProviderConfig| Ok(Provider(config)))
            .unwrap();
        let mut plugins = catalog.instance();
        let config = provider_config(false);
        let _provider = plugins.load(&handle, config.clone()).unwrap();
        for capability in [
            Capability::TypedKv,
            Capability::RevisionedRead,
            Capability::AtomicReadMany,
            Capability::CompareExchange,
            Capability::Sql,
        ] {
            for sql in [false, true] {
                for operations in [
                    vec![],
                    vec![Operation::Subscribe],
                    vec![Operation::Read],
                    vec![Operation::Write],
                    vec![Operation::SqlRead],
                ] {
                    let valid_operations = !operations.is_empty()
                        && operations.iter().all(|op| match op {
                            Operation::Read | Operation::Write => !sql,
                            Operation::SqlRead => sql,
                            Operation::Subscribe => true,
                            _ => unreachable!(),
                        });
                    let valid_requirement = match capability {
                        Capability::TypedKv => !sql,
                        Capability::Sql => sql,
                        Capability::RevisionedRead | Capability::AtomicReadMany => {
                            !sql && operations.contains(&Operation::Read)
                        }
                        Capability::CompareExchange => {
                            !sql && operations.contains(&Operation::Write)
                        }
                    };
                    let mut candidate = request(operations.clone()).require(capability);
                    if sql {
                        candidate.spec.kind = ResourceKind::Sql;
                    }
                    let result = ResourceRequirements::collect(
                        snapshots(vec![
                            ("candidate", candidate),
                            ("other", request([Operation::Read, Operation::Write])),
                        ]),
                        &Identity::user("alice").unwrap(),
                    );
                    if !valid_operations || !valid_requirement {
                        assert!(
                            matches!(result, Err(Error::InvalidRequest)),
                            "{capability:?} {sql} {operations:?}"
                        );
                    } else if sql {
                        assert!(matches!(result, Err(Error::SchemaConflict)));
                    } else {
                        assert!(result.is_ok());
                    }
                    assert_eq!(config.opens.load(SeqCst), 0);
                }
            }
        }
    }

    #[test]
    fn provider_preflight_and_actual_capabilities_fail_closed() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut catalog = PluginCatalog::new();
        let handle = catalog
            .register("provider", |config: ProviderConfig| Ok(Provider(config)))
            .unwrap();
        let mut plugins = catalog.instance();
        let mut config = provider_config(false);
        config.features.compare_exchange = false;
        let snapshot = plugins.load(&handle, config.clone()).unwrap();
        let requirements = requirements(vec![(
            "writer",
            request([Operation::Write]).require(Capability::CompareExchange),
        )]);
        let policy = policy(
            &requirements,
            BackendPlacement::Offer {
                snapshot,
                id: "tracked".into(),
            },
        );
        assert!(matches!(
            PreparedStorage::prepare(requirements, Identity::user("alice").unwrap(), policy),
            Err(Error::UnsupportedCapability)
        ));
        assert_eq!(config.opens.load(SeqCst), 0);

        let mut config = provider_config(false);
        config.features = BackendFeatures::SQLITE;
        let snapshot = plugins.reload(&handle, config.clone()).unwrap();
        let mut sql = request([Operation::SqlRead]);
        sql.spec.kind = ResourceKind::Sql;
        let requirements = self::requirements(vec![("reader", sql)]);
        let policy = self::policy(
            &requirements,
            BackendPlacement::Offer {
                snapshot,
                id: "tracked".into(),
            },
        );
        let prepared =
            PreparedStorage::prepare(requirements, Identity::user("alice").unwrap(), policy)
                .unwrap();
        assert_eq!(config.opens.load(SeqCst), 0);
        assert!(matches!(
            prepared.activate(),
            Err(Error::UnsupportedCapability)
        ));
        assert_eq!(config.opens.load(SeqCst), 1);
        assert_eq!(config.backend_drops.load(SeqCst), 1);

        let config = provider_config(true);
        let snapshot = plugins.reload(&handle, config.clone()).unwrap();
        let requirements = self::requirements(vec![("reader", request([Operation::Read]))]);
        let policy = self::policy(
            &requirements,
            BackendPlacement::Offer {
                snapshot,
                id: "tracked".into(),
            },
        );
        let prepared =
            PreparedStorage::prepare(requirements, Identity::user("alice").unwrap(), policy)
                .unwrap();
        assert!(matches!(prepared.activate(), Err(Error::Open(_))));
        assert_eq!(config.opens.load(SeqCst), 1);
        assert_eq!(config.backend_drops.load(SeqCst), 0);
    }

    #[cfg(unix)]
    #[test]
    fn distinct_mounts_cannot_alias_files_through_symlinks_or_hardlinks() {
        use std::os::unix::fs::symlink;
        let root =
            std::env::temp_dir().join(format!("phenix-placement-alias-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        std::fs::File::create(root.join("one.sqlite")).unwrap();
        symlink(root.join("one.sqlite"), root.join("symlink.sqlite")).unwrap();
        std::fs::hard_link(root.join("one.sqlite"), root.join("hardlink.sqlite")).unwrap();
        for alias in ["symlink.sqlite", "hardlink.sqlite"] {
            let mut other = request([Operation::Read]);
            other.spec.name = "phenix.test/other".into();
            let requirements =
                requirements(vec![("one", request([Operation::Read])), ("two", other)]);
            let mut policy = policy(
                &requirements,
                BackendPlacement::sqlite(root.join("one.sqlite")),
            );
            policy
                .mounts
                .insert("other".into(), BackendPlacement::sqlite(root.join(alias)));
            policy.resources.values_mut().last().unwrap().target =
                BindingTarget::Dataset(Address::parse("store://other/settings").unwrap());
            assert!(matches!(
                PreparedStorage::prepare(requirements, Identity::user("alice").unwrap(), policy),
                Err(Error::PhysicalAlias)
            ));
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn prepared_and_active_backends_pin_exact_provider_revision() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut catalog = PluginCatalog::new();
        let handle = catalog
            .register("provider", |config: ProviderConfig| Ok(Provider(config)))
            .unwrap();
        let mut plugins = catalog.instance();
        let old = provider_config(false);
        let snapshot = plugins.load(&handle, old.clone()).unwrap();
        let requirements = requirements(vec![("reader", request([Operation::Read]))]);
        let policy = policy(
            &requirements,
            BackendPlacement::Offer {
                snapshot: snapshot.clone(),
                id: "tracked".into(),
            },
        );
        let prepared =
            PreparedStorage::prepare(requirements, Identity::user("alice").unwrap(), policy)
                .unwrap();
        assert_eq!(old.opens.load(SeqCst), 0);
        assert_eq!(prepared.mounts()["main"].offer_id(), Some("tracked"));
        assert_eq!(
            prepared.mounts()["main"].capabilities(),
            Some(BackendFeatures::MEMORY)
        );
        let new = provider_config(false);
        drop(plugins.reload(&handle, new.clone()).unwrap());
        plugins.unload(&handle).unwrap();
        drop(snapshot);
        assert_eq!(old.provider_drops.load(SeqCst), 0);
        assert_eq!(new.provider_drops.load(SeqCst), 1);
        let active = prepared.activate().unwrap();
        assert_eq!(old.opens.load(SeqCst), 1);
        assert_eq!(new.opens.load(SeqCst), 0);
        drop(active);
        assert_eq!(old.backend_drops.load(SeqCst), 1);
        assert_eq!(old.provider_drops.load(SeqCst), 1);
    }

    #[test]
    fn failed_candidate_activation_preserves_old_bindings_and_data() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut catalog = PluginCatalog::new();
        let handle = catalog
            .register("provider", |config: ProviderConfig| Ok(Provider(config)))
            .unwrap();
        let mut plugins = catalog.instance();
        let old_config = provider_config(false);
        let snapshot = plugins.load(&handle, old_config.clone()).unwrap();
        let declarations = snapshots(vec![
            (
                "writer",
                request([Operation::Write]).require(Capability::CompareExchange),
            ),
            (
                "reader",
                request([Operation::Read]).require(Capability::RevisionedRead),
            ),
        ]);
        let identity = Identity::user("alice").unwrap();
        let requirements = ResourceRequirements::collect(declarations.clone(), &identity).unwrap();
        let policy = policy(
            &requirements,
            BackendPlacement::Offer {
                snapshot,
                id: "tracked".into(),
            },
        );
        let old = PreparedStorage::prepare(requirements, identity.clone(), policy)
            .unwrap()
            .activate()
            .unwrap();
        let writer = old.views["writer"]
            .user()
            .config::<Text>("phenix.test/settings")
            .unwrap();
        let revision = writer
            .compare_exchange("key", None, &Text("original".into()))
            .unwrap();

        let mut candidate_config = provider_config(false);
        candidate_config.actual = PLAIN; // Declared capabilities pass; opened backend fails.
        let snapshot = plugins.reload(&handle, candidate_config.clone()).unwrap();
        let requirements = ResourceRequirements::collect(declarations, &identity).unwrap();
        let policy = self::policy(
            &requirements,
            BackendPlacement::Offer {
                snapshot,
                id: "tracked".into(),
            },
        );
        let candidate = PreparedStorage::prepare(requirements, identity, policy).unwrap();
        assert!(matches!(
            candidate.activate(),
            Err(Error::UnsupportedCapability)
        ));
        assert_eq!(candidate_config.opens.load(SeqCst), 1);
        assert_eq!(candidate_config.backend_drops.load(SeqCst), 1);
        assert_eq!(old_config.opens.load(SeqCst), 1);
        assert_eq!(old_config.backend_drops.load(SeqCst), 0);
        assert_eq!(old_config.provider_drops.load(SeqCst), 0);

        // Resolve again through the old activation, not just an extracted handle.
        let reader = old.views["reader"]
            .user()
            .config::<Text>("phenix.test/settings")
            .unwrap();
        let found = reader.get_revisioned("key").unwrap().unwrap();
        assert_eq!(found.value.0, "original");
        assert_eq!(found.revision, revision);
        let updated = writer
            .compare_exchange("key", Some(&revision), &Text("still active".into()))
            .unwrap();
        let found = reader.get_revisioned("key").unwrap().unwrap();
        assert_eq!(found.value.0, "still active");
        assert_eq!(found.revision, updated);
    }

    #[test]
    fn extracted_scoped_kv_outlives_activation_and_unload_with_provider_alive_at_backend_drop() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut catalog = PluginCatalog::new();
        let handle = catalog
            .register("provider", |config: ProviderConfig| Ok(Provider(config)))
            .unwrap();
        let mut plugins = catalog.instance();
        let config = provider_config(false);
        let snapshot = plugins.load(&handle, config.clone()).unwrap();
        let requirements = requirements(vec![(
            "client",
            request([Operation::Read, Operation::Write])
                .require(Capability::RevisionedRead)
                .require(Capability::CompareExchange),
        )]);
        let policy = policy(
            &requirements,
            BackendPlacement::Offer {
                snapshot,
                id: "tracked".into(),
            },
        );
        let active =
            PreparedStorage::prepare(requirements, Identity::user("alice").unwrap(), policy)
                .unwrap()
                .activate()
                .unwrap();
        let kv = active.views["client"]
            .user()
            .config::<Text>("phenix.test/settings")
            .unwrap();
        kv.put("key", &Text("retained".into())).unwrap();
        drop(active);
        plugins.unload(&handle).unwrap();
        assert_eq!(config.backend_drops.load(SeqCst), 0);
        assert_eq!(config.provider_drops.load(SeqCst), 0);
        let found = kv.get_revisioned("key").unwrap().unwrap();
        assert_eq!(found.value.0, "retained");
        kv.compare_exchange("key", Some(&found.revision), &Text("after unload".into()))
            .unwrap();
        assert_eq!(kv.get("key").unwrap().unwrap().0, "after unload");
        // TrackedBackend::drop asserts its provider is still alive at this point.
        drop(kv);
        assert_eq!(config.backend_drops.load(SeqCst), 1);
        assert_eq!(config.provider_drops.load(SeqCst), 1);
    }

    #[test]
    fn failed_activation_drops_earlier_backends_and_can_retry() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut catalog = PluginCatalog::new();
        let handle = catalog
            .register("provider", |config: ProviderConfig| Ok(Provider(config)))
            .unwrap();
        let mut plugins = catalog.instance();
        let config = provider_config(false);
        let snapshot = plugins.load(&handle, config.clone()).unwrap();
        let mut other = request([Operation::Read]);
        other.spec.name = "phenix.test/other".into();
        let declarations = snapshots(vec![("one", request([Operation::Read])), ("two", other)]);
        let failing = provider_config(true);
        let failing_snapshot = plugins.reload(&handle, failing.clone()).unwrap();
        let identity = Identity::user("alice").unwrap();
        let requirements = ResourceRequirements::collect(declarations.clone(), &identity).unwrap();
        let mut policy = self::policy(
            &requirements,
            BackendPlacement::Offer {
                snapshot: snapshot.clone(),
                id: "tracked".into(),
            },
        );
        policy.mounts.insert(
            "z-failure".into(),
            BackendPlacement::Offer {
                snapshot: failing_snapshot,
                id: "tracked".into(),
            },
        );
        policy.resources.values_mut().last().unwrap().target =
            BindingTarget::Dataset(Address::parse("store://z-failure/other").unwrap());
        let prepared = PreparedStorage::prepare(requirements, identity.clone(), policy).unwrap();
        assert!(prepared.activate().is_err());
        assert_eq!(config.opens.load(SeqCst), 1);
        assert_eq!(config.backend_drops.load(SeqCst), 1);
        let requirements = ResourceRequirements::collect(declarations, &identity).unwrap();
        let mut policy = self::policy(
            &requirements,
            BackendPlacement::Offer {
                snapshot,
                id: "tracked".into(),
            },
        );
        policy.resources.values_mut().last().unwrap().target =
            BindingTarget::Dataset(Address::parse("store://main/other").unwrap());
        let active = PreparedStorage::prepare(requirements, identity, policy)
            .unwrap()
            .activate()
            .unwrap();
        drop(active);
        assert_eq!(config.backend_drops.load(SeqCst), 2);
    }

    struct MemoryPlugin;
    impl PluginInstance for MemoryPlugin {
        fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
            registrar.add(BackendOffer::new("memory", || {
                Ok(Box::new(MemoryBackend::default()))
            }))
        }
    }

    #[derive(Default)]
    struct TextCodec;
    impl Codec for TextCodec {
        type Value = String;
        fn schema(&self) -> Schema {
            Schema {
                name: "phenix.test.utf8".into(),
                revision: 1,
                structure: "utf8-string".into(),
                codec: "utf8".into(),
            }
        }
        fn encode(&self, value: &String) -> phenix_storage::Result<Vec<u8>> {
            Ok(value.as_bytes().to_vec())
        }
        fn decode(&self, bytes: &[u8]) -> phenix_storage::Result<String> {
            String::from_utf8(bytes.to_vec()).map_err(|error| StorageError::CodecCause {
                summary: "invalid codec bytes",
                cause: Box::new(error),
            })
        }
    }

    #[test]
    fn independent_resource_offers_share_identity_only_when_schemas_match() {
        let identity = Identity::user("alice")
            .unwrap()
            .project("project-1")
            .unwrap();
        let reader = ResourceOffer::kv_schema(
            "reader-declaration",
            ScopeKind::Project,
            StorageClass::Config,
            "phenix.agent/settings",
            TextCodec.schema(),
            [Operation::Read],
        )
        .request
        .require(Capability::RevisionedRead);
        let writer = ResourceOffer::kv_schema(
            "writer-declaration",
            ScopeKind::Project,
            StorageClass::Config,
            "phenix.agent/settings",
            TextCodec.schema(),
            [Operation::Write],
        )
        .request
        .require(Capability::CompareExchange);
        let id = reader.spec.resolve(&identity).unwrap();
        assert_eq!(writer.spec.resolve(&identity).unwrap(), id);
        for reverse in [false, true] {
            let mut declarations = vec![("reader", reader.clone()), ("writer", writer.clone())];
            if reverse {
                declarations.reverse();
            }
            // snapshots() registers and loads in this order as well.
            let declarations = snapshots(declarations);
            let requirements =
                ResourceRequirements::collect(declarations.clone(), &identity).unwrap();
            assert_eq!(
                requirements.iter().collect::<Vec<_>>(),
                vec![(&id, &reader.spec.kind)]
            );
            assert_eq!(
                requirements.requests()["reader"][0].1.required,
                [Capability::RevisionedRead].into()
            );
            assert_eq!(
                requirements.requests()["writer"][0].1.required,
                [Capability::CompareExchange].into()
            );
            let mut policy = policy(&requirements, BackendPlacement::memory());
            policy.resources.get_mut(&id).unwrap().allowed =
                [Operation::Read, Operation::Write].into();
            policy.plugin_access.insert(
                "reader".into(),
                [(id.clone(), [Operation::Read].into())].into(),
            );
            policy.plugin_access.insert(
                "writer".into(),
                [(id.clone(), [Operation::Write].into())].into(),
            );
            let active = PreparedStorage::prepare(requirements, identity.clone(), policy)
                .unwrap()
                .activate()
                .unwrap();
            let read = active.views["reader"]
                .project()
                .unwrap()
                .config::<Text>("phenix.agent/settings")
                .unwrap();
            let write = active.views["writer"]
                .project()
                .unwrap()
                .config::<Text>("phenix.agent/settings")
                .unwrap();
            let revision = write
                .compare_exchange("key", None, &Text("shared".into()))
                .unwrap();
            assert_eq!(read.get("key").unwrap().unwrap().0, "shared");
            let found = read.get_revisioned("key").unwrap().unwrap();
            assert_eq!(found.revision, revision);
            assert_eq!(found.value.0, "shared");
            assert!(matches!(
                read.put("key", &Text("denied".into())),
                Err(StorageError::Denied)
            ));
            assert!(matches!(
                read.compare_exchange("key", None, &Text("denied".into())),
                Err(StorageError::Denied)
            ));
            assert!(matches!(write.get("key"), Err(StorageError::Denied)));
            assert!(matches!(
                write.get_revisioned("key"),
                Err(StorageError::Denied)
            ));

            let requirements = ResourceRequirements::collect(declarations, &identity).unwrap();
            let mut policy = self::policy(&requirements, BackendPlacement::memory());
            policy
                .plugin_access
                .get_mut("reader")
                .unwrap()
                .insert(id.clone(), [Operation::Write].into());
            assert!(matches!(
                PreparedStorage::prepare(requirements, identity.clone(), policy),
                Err(Error::Denied)
            ));

            let mut incompatible = writer.clone();
            if let ResourceKind::TypedKv(schema) = &mut incompatible.spec.kind {
                schema.revision += 1;
            }
            let mut declarations = vec![("reader", reader.clone()), ("writer", incompatible)];
            if reverse {
                declarations.reverse();
            }
            assert!(matches!(
                ResourceRequirements::collect(snapshots(declarations), &identity),
                Err(Error::SchemaConflict)
            ));
        }
    }

    #[test]
    fn plugin_offer_mounts_and_outlives_logical_plugin_unload() {
        let mut catalog = PluginCatalog::new();
        let plugin = catalog
            .register("storage-provider", |(): ()| Ok(MemoryPlugin))
            .unwrap();
        let mut plugins = catalog.instance();
        let snapshot = plugins.load(&plugin, ()).unwrap();
        let registry = Registry::new();
        mount_offer(&registry, &snapshot, "memory", "shared").unwrap();
        assert!(matches!(
            mount_offer(&registry, &snapshot, "unknown", "other"),
            Err(Error::MissingOffer)
        ));
        plugins.unload(&plugin).unwrap();
        drop(snapshot);

        let address = Address::parse("store://shared/values").unwrap();
        registry.register(&address, TextCodec.schema()).unwrap();
        let session = registry.session([Grant::new(
            Scope::Dataset(address.clone()),
            [Operation::Read, Operation::Write],
        )]);
        session
            .put(&address, "key", &"value".to_string(), &TextCodec)
            .unwrap();
        assert_eq!(
            session.get(&address, "key", &TextCodec).unwrap(),
            Some("value".into())
        );
    }
}
