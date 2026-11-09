//! Optional integration between host-owned storage mounts and the existing
//! in-process plugin Registrar. No second loader, database engine or kernel API.
use phenix_plugin::{Contribution, ContributionHandle, PluginSnapshot};
use phenix_storage::{
    Address, Backend, Capability, Codec, Error as StorageError, Grant, Observation, Operation,
    Registry, Schema, Scope, SqlBackend, TypedKv,
    scoped::{
        self, BindingTarget, Identity, ResourceCatalog, ResourceId, ResourceKind, ResourceRequest,
        ResourceSpec, ScopeKind, Storage, StorageClass, Stored, XdgPaths,
    },
};
use std::collections::BTreeMap;

pub type OpenBackend = dyn Fn() -> Result<Box<dyn Backend>, String> + Send + Sync;

/// A plugin *offers* a backend factory. Only the trusted host can choose a
/// physical mount name, location, configuration and storage authority.
pub struct BackendOffer {
    id: String,
    open: Box<OpenBackend>,
    features: Option<BackendFeatures>,
}
impl BackendOffer {
    pub fn new(
        id: impl Into<String>,
        open: impl Fn() -> Result<Box<dyn Backend>, String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            open: Box::new(open),
            features: None,
        }
    }
    /// Trusted host preflight relies on this provider's declared contract.
    pub fn with_features(mut self, features: BackendFeatures) -> Self {
        self.features = Some(features);
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
    Open(String),
    Storage(StorageError),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
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
    let backend = (lease.open)().map_err(Error::Open)?;
    registry.mount_boxed(
        mount_name,
        Box::new(LeasedBackend {
            backend,
            _lease: lease,
        }),
    )?;
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
    Memory,
    Sqlite(std::path::PathBuf),
    Offer {
        snapshot: PluginSnapshot,
        id: String,
    },
}
pub struct ResourceBinding {
    pub target: BindingTarget,
    pub allowed: std::collections::BTreeSet<Operation>,
}
/// Trusted host decisions, not plugin contributions. SQL mount sharing requires
/// an explicit allowlist. Reserved KV tables remain protected even on shared mounts.
#[derive(Default)]
pub struct StoragePolicy {
    pub mounts: BTreeMap<String, BackendPlacement>,
    pub resources: BTreeMap<ResourceId, ResourceBinding>,
    pub shared_sql_mounts: std::collections::BTreeSet<String>,
    /// Host approvals for each plugin/resource pair. Missing entries deny access;
    /// these and ResourceBinding::allowed must both approve every operation.
    pub plugin_access:
        BTreeMap<String, BTreeMap<ResourceId, std::collections::BTreeSet<Operation>>>,
    xdg_paths: Option<XdgPaths>,
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
            policy.mounts.insert(mount, BackendPlacement::Sqlite(path));
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
pub enum PreparedBackend {
    Memory,
    Sqlite(std::path::PathBuf),
    Offer(ContributionHandle<BackendOffer>),
}
impl PreparedBackend {
    /// Provider-declared capabilities; activation verifies the opened backend.
    pub fn capabilities(&self) -> Option<BackendFeatures> {
        match self {
            Self::Memory => Some(BackendFeatures::MEMORY),
            Self::Sqlite(_) => Some(BackendFeatures::SQLITE),
            Self::Offer(lease) => lease.features,
        }
    }
}
pub struct PreparedStorage {
    requirements: ResourceRequirements,
    identity: Identity,
    resources: BTreeMap<ResourceId, ResourceBinding>,
    mounts: BTreeMap<String, PreparedBackend>,
    xdg_paths: Option<XdgPaths>,
    plugin_access: BTreeMap<String, BTreeMap<ResourceId, std::collections::BTreeSet<Operation>>>,
    shared_sql_mounts: std::collections::BTreeSet<String>,
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
        let mut files = std::collections::BTreeSet::new();
        #[cfg(unix)]
        let mut file_ids = std::collections::BTreeSet::new();
        for (name, placement) in policy.mounts {
            // Reuse canonical mount validation without allocating a registry.
            Address::parse(&format!("store://{name}/validation"))?;
            let (backend, caps) = match placement {
                BackendPlacement::Memory => (PreparedBackend::Memory, BackendFeatures::MEMORY),
                BackendPlacement::Sqlite(path) => {
                    if !path.is_absolute()
                        || path
                            .components()
                            .any(|c| matches!(c, std::path::Component::ParentDir))
                    {
                        return Err(Error::PhysicalAlias);
                    }
                    // Resolve the existing prefix without creating anything, so
                    // symlinked parents and spelling aliases cannot hide sharing.
                    let mut prefix = path.as_path();
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
                        let metadata = std::fs::metadata(&path)?;
                        if !file_ids.insert((metadata.dev(), metadata.ino())) {
                            return Err(Error::PhysicalAlias);
                        }
                    }
                    (PreparedBackend::Sqlite(path), BackendFeatures::SQLITE)
                }
                BackendPlacement::Offer { snapshot, id } => {
                    let lease = snapshot
                        .get::<BackendOffer>(&id)
                        .ok_or(Error::MissingOffer)?;
                    let caps = lease.features.ok_or(Error::UnsupportedCapability)?;
                    (PreparedBackend::Offer(lease), caps)
                }
            };
            mounts.insert(name.clone(), backend);
            features.insert(name, caps);
        }
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
            if let PreparedBackend::Sqlite(path) = backend {
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
                if let PreparedBackend::Sqlite(path) = backend {
                    let root = xdg_root(paths, &self.resources, mount, path)?;
                    private_xdg_file(&root, path, false)?;
                    private_roots.insert(mount.clone(), root);
                }
            }
        }
        let registry = Registry::new();
        for (name, backend) in self.mounts {
            match backend {
                PreparedBackend::Memory => registry.mount_memory(&name)?,
                PreparedBackend::Sqlite(path) => {
                    if let Some(root) = private_roots.get(&name) {
                        private_xdg_file(root, &path, true)?;
                    } else if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    registry.mount_sqlite(&name, &path)?;
                    if let Some(root) = private_roots.get(&name) {
                        private_xdg_file(root, &path, false)?;
                    }
                }
                PreparedBackend::Offer(lease) => {
                    let mut backend = (lease.open)().map_err(Error::Open)?;
                    // Probe the provider before mounting it or issuing sessions.
                    // Built-in memory/SQLite capabilities are fixed.
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
                    registry.mount_boxed(
                        &name,
                        Box::new(LeasedBackend {
                            backend,
                            _lease: lease,
                        }),
                    )?;
                }
            }
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
            BackendPlacement::Sqlite(path) => path.clone(),
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
                BackendPlacement::Sqlite(path) => path.clone(),
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
            BackendPlacement::Sqlite(path) => path.clone(),
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
            BackendPlacement::Sqlite(root.join("manual.sqlite"));
        assert!(matches!(
            PreparedStorage::prepare(requirements, identity.clone(), policy),
            Err(Error::UnsafeXdgPath)
        ));
        assert!(!root.join("manual.sqlite").exists());

        let requirements = ResourceRequirements::collect(declarations, &identity).unwrap();
        let manual = self::policy(
            &requirements,
            BackendPlacement::Sqlite(root.join("manual.sqlite")),
        );
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
            let mut policy = self::policy(&requirements, BackendPlacement::Sqlite(path.clone()));
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
        let mut policy = policy(&requirements, BackendPlacement::Memory);
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
        let policy = self::policy(&requirements, BackendPlacement::Memory);
        assert!(matches!(
            PreparedStorage::prepare(requirements, identity.clone(), policy),
            Err(Error::UnsupportedCapability)
        ));

        let requirements = self::requirements(vec![("writer", request([Operation::Write]))]);
        let mut policy = self::policy(&requirements, BackendPlacement::Memory);
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
        let mut policy = self::policy(&requirements, BackendPlacement::Memory);
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
            matches!(prepared.mounts().values().next().unwrap(), PreparedBackend::Sqlite(path) if path.starts_with(&root))
        );
    }

    #[test]
    fn prepared_plan_exposes_existing_data_without_opening_or_granting() {
        let requirements = requirements(vec![(
            "writer",
            request([Operation::Write]).require(Capability::CompareExchange),
        )]);
        let id = requirements.iter().next().unwrap().0.clone();
        let mut policy = policy(&requirements, BackendPlacement::Memory);
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
        assert!(
            prepared.requirements().requests()["writer"][0]
                .1
                .required
                .contains(&Capability::CompareExchange)
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
        assert!(matches!(prepared.mounts()["main"], PreparedBackend::Memory));
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
        let policy = policy(&requirements, BackendPlacement::Memory);
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
            ("reader", request([Operation::Read])),
        ]);
        assert_eq!(requirements.iter().count(), 1);
        assert_eq!(
            requirements.requests()["reader"][0].1.operations,
            [Operation::Read].into()
        );
        let policy = policy(&requirements, BackendPlacement::Memory);
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
            BackendPlacement::Sqlite(std::env::temp_dir().join("phenix-prepare-only.sqlite")),
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
        let policy = policy(&requirements, BackendPlacement::Memory);
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
            BackendPlacement::Sqlite(
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

    #[derive(Clone)]
    struct ProviderConfig {
        opens: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        backend_drops: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        provider_drops: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        fail: bool,
        features: BackendFeatures,
        legacy: bool,
    }
    struct Provider(ProviderConfig);
    impl Drop for Provider {
        fn drop(&mut self) {
            self.0
                .provider_drops
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
    struct TrackedBackend(
        MemoryBackend,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        bool,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
    );
    impl Backend for TrackedBackend {
        fn typed_kv(&mut self) -> Option<&mut dyn TypedKv> {
            if self.2 {
                Some(self)
            } else {
                Some(&mut self.0)
            }
        }
    }
    impl TypedKv for TrackedBackend {
        fn get(&mut self, dataset: &str, key: &str) -> phenix_storage::Result<Option<Vec<u8>>> {
            self.0.get(dataset, key)
        }
        fn put(&mut self, dataset: &str, key: &str, value: Vec<u8>) -> phenix_storage::Result<()> {
            self.0.put(dataset, key, value)
        }
        fn delete(&mut self, dataset: &str, key: &str) -> phenix_storage::Result<bool> {
            self.0.delete(dataset, key)
        }
    }
    impl Drop for TrackedBackend {
        fn drop(&mut self) {
            assert_eq!(
                self.3.load(std::sync::atomic::Ordering::SeqCst),
                0,
                "backend must drop before its provider"
            );
            self.1.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
                    Ok(Box::new(TrackedBackend(
                        MemoryBackend::default(),
                        config.backend_drops.clone(),
                        config.legacy,
                        config.provider_drops.clone(),
                    )))
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
            legacy: false,
        }
    }

    /// Provider whose backend supports coherent snapshots without conditional writes.
    #[derive(Default)]
    struct BatchReadBackend(MemoryBackend);
    impl Backend for BatchReadBackend {
        fn typed_kv(&mut self) -> Option<&mut dyn TypedKv> {
            Some(self)
        }
    }
    impl TypedKv for BatchReadBackend {
        fn supports_revisioned_reads(&self) -> bool {
            true
        }
        fn supports_atomic_read_many(&self) -> bool {
            true
        }
        fn read_many(
            &mut self,
            keys: &[phenix_storage::snapshot::ReadKey<'_>],
        ) -> phenix_storage::Result<Vec<Option<phenix_storage::Revisioned<Vec<u8>>>>> {
            self.0.read_many(keys)
        }
        fn get_revisioned(
            &mut self,
            dataset: &str,
            key: &str,
        ) -> phenix_storage::Result<Option<phenix_storage::Revisioned<Vec<u8>>>> {
            self.0.get_revisioned(dataset, key)
        }
        fn get(&mut self, dataset: &str, key: &str) -> phenix_storage::Result<Option<Vec<u8>>> {
            self.0.get(dataset, key)
        }
        fn put(&mut self, dataset: &str, key: &str, value: Vec<u8>) -> phenix_storage::Result<()> {
            self.0.put(dataset, key, value)
        }
        fn delete(&mut self, dataset: &str, key: &str) -> phenix_storage::Result<bool> {
            self.0.delete(dataset, key)
        }
    }
    struct BatchReaderProvider;
    impl PluginInstance for BatchReaderProvider {
        fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
            registrar.add(
                BackendOffer::new("read-only-batch", || {
                    Ok(Box::new(BatchReadBackend::default()))
                })
                .with_features(BackendFeatures {
                    typed_kv: true,
                    revisioned_read: true,
                    atomic_read_many: true,
                    compare_exchange: false,
                    sql: false,
                    invalidation: true,
                }),
            )
        }
    }

    #[test]
    fn read_only_atomic_batch_provider_works_without_compare_exchange() {
        let mut catalog = PluginCatalog::new();
        let handle = catalog
            .register("batch", |_: ()| Ok(BatchReaderProvider))
            .unwrap();
        let mut plugins = catalog.instance();
        let snapshot = plugins.load(&handle, ()).unwrap();
        let spec = phenix_storage::scoped::CellSpec::<Text>::new(
            ScopeKind::User,
            StorageClass::Config,
            "phenix.test/settings",
        );
        let requirements = requirements(vec![("reader", spec.request([Operation::Read]))]);
        assert_eq!(
            requirements.requests()["reader"][0].1.required,
            [Capability::AtomicReadMany].into()
        );
        let policy = policy(
            &requirements,
            BackendPlacement::Offer {
                snapshot,
                id: "read-only-batch".into(),
            },
        );
        let active =
            PreparedStorage::prepare(requirements, Identity::user("alice").unwrap(), policy)
                .unwrap()
                .activate()
                .unwrap();
        let cell = active.views["reader"].bind(spec.read()).unwrap();
        assert!(cell.optional().get().unwrap().is_none());
        let kv = active.views["reader"]
            .user()
            .config::<Text>("phenix.test/settings")
            .unwrap();
        assert!(kv.get("value").unwrap().is_none());
        assert!(kv.get_revisioned("value").unwrap().is_none());
        assert!(matches!(
            kv.compare_exchange("value", None, &Text("denied".into())),
            Err(phenix_storage::Error::Denied)
        ));
    }

    #[test]
    fn plain_scoped_reads_need_neither_revision_nor_cas() {
        let mut catalog = PluginCatalog::new();
        let handle = catalog
            .register("legacy", |config: ProviderConfig| Ok(Provider(config)))
            .unwrap();
        let mut plugins = catalog.instance();
        let mut config = provider_config(false);
        config.legacy = true;
        config.features.revisioned_read = false;
        config.features.atomic_read_many = false;
        config.features.compare_exchange = false;
        let snapshot = plugins.load(&handle, config).unwrap();
        let requirements = requirements(vec![("reader", request([Operation::Read]))]);
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
        let kv = active.views["reader"]
            .user()
            .config::<Text>("phenix.test/settings")
            .unwrap();
        assert!(kv.get("missing").unwrap().is_none());
        let spec = phenix_storage::scoped::CellSpec::<Text>::new(
            ScopeKind::User,
            StorageClass::Config,
            "phenix.test/settings",
        );
        assert!(matches!(
            active.views["reader"].bind(spec.read()),
            Err(scoped::Error::Storage(
                phenix_storage::Error::UnsupportedCapability {
                    capability: Capability::AtomicReadMany
                }
            ))
        ));
        assert!(matches!(
            kv.get_revisioned("missing"),
            Err(phenix_storage::Error::UnsupportedCapability {
                capability: Capability::RevisionedRead
            })
        ));
    }

    #[test]
    fn snapshot_requirement_rejects_cas_only_and_false_batch_offers() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut catalog = PluginCatalog::new();
        let handle = catalog
            .register("provider", |config: ProviderConfig| Ok(Provider(config)))
            .unwrap();
        let mut plugins = catalog.instance();
        let spec = phenix_storage::scoped::CellSpec::<Text>::new(
            ScopeKind::User,
            StorageClass::Config,
            "phenix.test/settings",
        );
        let mut declared = provider_config(false);
        declared.features.atomic_read_many = false;
        let snapshot = plugins.load(&handle, declared.clone()).unwrap();
        let requirements = requirements(vec![("reader", spec.request([Operation::Read]))]);
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
        assert_eq!(declared.opens.load(SeqCst), 0);

        let mut dishonest = provider_config(false);
        dishonest.legacy = true; // Implements plain KV only.
        dishonest.features.revisioned_read = false;
        dishonest.features.atomic_read_many = true; // Incorrect declaration.
        dishonest.features.compare_exchange = false;
        let snapshot = plugins.reload(&handle, dishonest.clone()).unwrap();
        let requirements = requirements(vec![("reader", spec.request([Operation::Read]))]);
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
        assert!(matches!(
            prepared.activate(),
            Err(Error::UnsupportedCapability)
        ));
        assert_eq!(dishonest.opens.load(SeqCst), 1);
        assert_eq!(dishonest.backend_drops.load(SeqCst), 1);
    }

    #[test]
    fn explicitly_revisioned_reads_require_backend_support_in_prepare_and_activate() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut catalog = PluginCatalog::new();
        let handle = catalog
            .register("legacy", |config: ProviderConfig| Ok(Provider(config)))
            .unwrap();
        let mut plugins = catalog.instance();
        for declared_revisioned in [false, true] {
            let mut config = provider_config(false);
            config.legacy = true;
            config.features.revisioned_read = declared_revisioned;
            let snapshot = if declared_revisioned {
                plugins.reload(&handle, config.clone()).unwrap()
            } else {
                plugins.load(&handle, config.clone()).unwrap()
            };
            let requirements = requirements(vec![(
                "reader",
                request([Operation::Read]).require(Capability::RevisionedRead),
            )]);
            assert!(
                requirements.requests()["reader"][0]
                    .1
                    .required
                    .contains(&Capability::RevisionedRead)
            );
            let policy = policy(
                &requirements,
                BackendPlacement::Offer {
                    snapshot,
                    id: "tracked".into(),
                },
            );
            let prepared =
                PreparedStorage::prepare(requirements, Identity::user("alice").unwrap(), policy);
            assert_eq!(config.opens.load(SeqCst), 0);
            if declared_revisioned {
                assert!(matches!(
                    prepared.unwrap().activate(),
                    Err(Error::UnsupportedCapability)
                ));
                assert_eq!(config.opens.load(SeqCst), 1);
                assert_eq!(config.backend_drops.load(SeqCst), 1);
            } else {
                assert!(matches!(prepared, Err(Error::UnsupportedCapability)));
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
                BackendPlacement::Sqlite(root.join("one.sqlite")),
            );
            policy
                .mounts
                .insert("other".into(), BackendPlacement::Sqlite(root.join(alias)));
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
        assert!(
            matches!(&prepared.mounts()["main"], PreparedBackend::Offer(offer) if offer.id() == "tracked")
        );
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
            ("writer", request([Operation::Write])),
            ("reader", request([Operation::Read])),
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
        candidate_config.legacy = true; // Declared capabilities pass; opened backend fails.
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
            request([Operation::Read, Operation::Write]),
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
        assert_eq!(
            kv.get_revisioned("key").unwrap().unwrap().value.0,
            "after unload"
        );
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
            String::from_utf8(bytes.to_vec())
                .map_err(|error| StorageError::Codec(error.to_string()))
        }
    }

    #[test]
    fn independent_resource_offers_share_identity_only_when_schemas_match() {
        use phenix_storage::scoped::StorageClass;
        struct Offers(u32);
        impl PluginInstance for Offers {
            fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
                let mut schema = TextCodec.schema();
                schema.revision = self.0;
                registrar.add(ResourceOffer::kv_schema(
                    "settings",
                    ScopeKind::Project,
                    StorageClass::Config,
                    "phenix.agent/settings",
                    schema,
                    [Operation::Read],
                ))
            }
        }
        let mut catalog = PluginCatalog::new();
        let a = catalog
            .register("a", |revision: u32| Ok(Offers(revision)))
            .unwrap();
        let b = catalog
            .register("b", |revision: u32| Ok(Offers(revision)))
            .unwrap();
        let mut instances = catalog.instance();
        instances.load(&a, 1).unwrap();
        instances.load(&b, 1).unwrap();
        let identity = Identity::user("alice")
            .unwrap()
            .project("project-1")
            .unwrap();
        let requirements = ResourceRequirements::collect(instances.snapshots(), &identity).unwrap();
        assert_eq!(requirements.iter().count(), 1);
        let mut reversed: Vec<_> = instances.snapshots().collect();
        reversed.reverse();
        let reverse = ResourceRequirements::collect(reversed, &identity).unwrap();
        assert_eq!(
            requirements.iter().collect::<Vec<_>>(),
            reverse.iter().collect::<Vec<_>>()
        );
        assert_eq!(
            requirements.requests().keys().collect::<Vec<_>>(),
            reverse.requests().keys().collect::<Vec<_>>()
        );

        instances.reload(&b, 2).unwrap();
        assert!(matches!(
            ResourceRequirements::collect(instances.snapshots(), &identity),
            Err(Error::SchemaConflict)
        ));
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
