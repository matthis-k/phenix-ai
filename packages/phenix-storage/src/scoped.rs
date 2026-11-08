//! Stable scoped resource identities over existing authorized storage sessions.
//!
//! This module does not choose database engines, issue grants or create filesystem
//! paths. A trusted host binds logical resources to provisioned physical datasets.
//! The low-level Registry/Session API remains available unchanged.
use crate::{Address, Codec, Scope, Session, Sql, Subscription};
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
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || byte == b'-'
                || byte == b'_'
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
    pub fn user(id: impl Into<String>) -> Result<Self> {
        let id = id.into();
        if !component(&id) {
            return Err(Error::InvalidIdentity);
        }
        Ok(Self { user: id, project: None, session: None })
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
    logical: BTreeMap<ResourceId, Address>,
    physical: BTreeMap<Address, ResourceId>,
}
impl ResourceCatalog {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn bind(&mut self, resource: ResourceId, address: Address) -> Result<()> {
        if !resource.scope.valid() {
            return Err(Error::InvalidIdentity);
        }
        if !resource_name(&resource.name) {
            return Err(Error::InvalidResource);
        }
        if self.logical.contains_key(&resource) {
            return Err(Error::DuplicateResource);
        }
        if self.physical.contains_key(&address) {
            return Err(Error::PhysicalAlias);
        }
        self.physical.insert(address.clone(), resource.clone());
        self.logical.insert(resource, address);
        Ok(())
    }
    pub fn freeze(self) -> Bindings {
        Bindings { entries: Arc::new(self.logical) }
    }
}

/// Immutable lookup shared by independent plugin instances, each carrying
/// its own restricted Session. A binding never grants storage authority.
#[derive(Clone)]
pub struct Bindings {
    entries: Arc<BTreeMap<ResourceId, Address>>,
}
impl Bindings {
    pub fn access(&self, session: Session, identity: Identity) -> Storage {
        Storage { bindings: self.clone(), session, identity }
    }
}

pub struct Storage {
    bindings: Bindings,
    session: Session,
    identity: Identity,
}
impl Storage {
    pub fn user(&self) -> View<'_> {
        View { store: self, scope: ScopeId::User(self.identity.user.clone()) }
    }
    pub fn project(&self) -> Result<View<'_>> {
        let project = self.identity.project.clone().ok_or(Error::MissingProject)?;
        Ok(View { store: self, scope: ScopeId::Project(project) })
    }
    pub fn session(&self) -> Result<View<'_>> {
        let session = self.identity.session.clone().ok_or(Error::MissingSession)?;
        Ok(View { store: self, scope: ScopeId::Session(session) })
    }
    fn resolve(&self, resource: &ResourceId) -> Result<Address> {
        self.bindings.entries.get(resource).cloned().ok_or(Error::UnknownResource)
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
        Ok(Kv {
            address: self.store.resolve(&key)?,
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
    /// SQL remains mount-scoped in the underlying Session authority.
    /// A logical resource does not turn SQL into dataset-scoped access.
    pub fn sql(&self, class: StorageClass, name: &str) -> Result<SqlResource> {
        let key = ResourceId::new(self.scope.clone(), class, name)?;
        Ok(SqlResource {
            address: self.store.resolve(&key)?,
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

/// Typed KV view; no extra hidden cache and no implicit provisioning.
pub struct Kv<T: Stored> {
    address: Address,
    session: Session,
    format: T::Format,
    _value: PhantomData<fn() -> T>,
}
impl<T: Stored> Kv<T> {
    pub fn get(&self, key: &str) -> crate::Result<Option<T>> {
        self.session.get(&self.address, key, &self.format)
    }
    pub fn put(&self, key: &str, value: &T) -> crate::Result<()> {
        self.session.put(&self.address, key, value, &self.format)
    }
    pub fn delete(&self, key: &str) -> crate::Result<bool> {
        self.session.delete(&self.address, key)
    }
    pub fn changed(&self, capacity: usize) -> crate::Result<Subscription> {
        self.session.subscribe(Scope::Dataset(self.address.clone()), capacity)
    }
}

/// Native SQL facade over the existing authorized transaction API.
pub struct SqlResource {
    address: Address,
    session: Session,
}
impl SqlResource {
    pub fn transaction<T>(
        &self,
        f: impl FnOnce(&Sql<'_>) -> crate::Result<T>,
    ) -> crate::Result<T> {
        self.session.transaction(&self.address, f)
    }
}

/// Host-only XDG placement policy; returns directories, never silently opens
/// a database or changes its location. Invalid relative XDG overrides are
/// ignored in favor of the HOME defaults.
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
        if [ &config, &data, &state, &cache ].iter().any(|p| !p.is_absolute())
            || runtime.as_ref().is_some_and(|p| !p.is_absolute())
        {
            return Err(Error::InvalidBaseDirectory);
        }
        Ok(Self { config, data, state, cache, runtime })
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
        let base = match resource.class {
            StorageClass::Config => &self.config,
            StorageClass::Data => &self.data,
            StorageClass::State => &self.state,
            StorageClass::Cache => &self.cache,
            StorageClass::Runtime => self.runtime.as_ref().ok_or(Error::MissingRuntimeDirectory)?,
        };
        let root = base.join("phenix");
        Ok(match &resource.scope {
            ScopeId::User(id) => root.join("users").join(id),
            ScopeId::Project(id) => root.join("projects").join(id),
            ScopeId::Session(id) => root.join("sessions").join(id),
        })
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
    fn two_plugins_share_one_resource_but_not_permissions() {
        let registry = Registry::new();
        registry.mount_memory("main").unwrap();
        let address = Address::parse("store://main/settings").unwrap();
        registry.register(&address, TextCodec.schema()).unwrap();
        let resource = ResourceId::new(
            ScopeId::User("alice".into()), StorageClass::Config, "phenix.agent/settings",
        ).unwrap();
        let mut catalog = ResourceCatalog::new();
        catalog.bind(resource, address.clone()).unwrap();
        let bindings = catalog.freeze();
        let identity = Identity::user("alice").unwrap();
        let writer = bindings.access(
            registry.session([Grant::new(Scope::Dataset(address.clone()), [Operation::Write])]),
            identity.clone(),
        );
        let reader = bindings.access(
            registry.session([Grant::new(Scope::Dataset(address.clone()), [Operation::Read])]),
            identity,
        );
        writer.user().config::<Text>("phenix.agent/settings").unwrap()
            .put("default", &Text("selected".into())).unwrap();
        let value = reader.user().config::<Text>("phenix.agent/settings").unwrap()
            .get("default").unwrap().unwrap();
        assert_eq!(value.0, "selected");
        assert!(reader.user().config::<Text>("phenix.agent/settings").unwrap()
            .put("default", &Text("denied".into())).is_err());
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
        catalog.bind(
            ResourceId::new(ScopeId::User("alice".into()), StorageClass::Config, "phenix.agent/settings").unwrap(),
            user.clone(),
        ).unwrap();
        catalog.bind(
            ResourceId::new(ScopeId::Project("project-1".into()), StorageClass::Config, "phenix.agent/settings").unwrap(),
            project.clone(),
        ).unwrap();
        let access = catalog.freeze().access(
            registry.session([Grant::new(Scope::Mount("main".into()), [Operation::Read, Operation::Write])]),
            Identity::user("alice").unwrap().project("project-1").unwrap(),
        );
        access.user().config::<Text>("phenix.agent/settings").unwrap().put("a", &Text("user".into())).unwrap();
        access.project().unwrap().config::<Text>("phenix.agent/settings").unwrap().put("a", &Text("project".into())).unwrap();
        assert_eq!(access.user().config::<Text>("phenix.agent/settings").unwrap().get("a").unwrap().unwrap().0, "user");
        assert_eq!(access.project().unwrap().config::<Text>("phenix.agent/settings").unwrap().get("a").unwrap().unwrap().0, "project");
    }

    #[test]
    fn bindings_reject_aliases_missing_scopes_and_bad_names() {
        let a = Address::parse("store://main/item").unwrap();
        let key = ResourceId::new(ScopeId::User("u".into()), StorageClass::Data, "phenix.model/records").unwrap();
        let mut catalog = ResourceCatalog::new();
        catalog.bind(key.clone(), a.clone()).unwrap();
        assert!(matches!(catalog.bind(key, a.clone()), Err(Error::DuplicateResource)));
        let other = ResourceId::new(ScopeId::User("u".into()), StorageClass::Config, "phenix.model/records").unwrap();
        assert!(matches!(catalog.bind(other, a), Err(Error::PhysicalAlias)));
        assert!(ResourceId::new(ScopeId::Project("../unsafe".into()), StorageClass::Cache, "x/cache").is_err());
        assert!(ResourceId::new(ScopeId::User("u".into()), StorageClass::Data, "a/../bad").is_err());
        let storage = catalog.freeze().access(
            Registry::new().session([]),
            Identity::user("u").unwrap(),
        );
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
        ).unwrap();
        let open = |registry: &Registry| {
            let mut catalog = ResourceCatalog::new();
            catalog.bind(resource.clone(), address.clone()).unwrap();
            catalog.freeze().access(
                registry.session([Grant::new(
                    Scope::Dataset(address.clone()),
                    [Operation::Read, Operation::Write],
                )]),
                Identity::user("alice").unwrap().project("stable-repo").unwrap(),
            )
        };

        {
            let registry = Registry::new();
            registry.mount_sqlite("main", &path).unwrap();
            registry.register(&address, TextCodec.schema()).unwrap();
            let access = open(&registry);
            access.project().unwrap().config::<Text>("phenix.agent/settings").unwrap()
                .put("default", &Text("persisted".into())).unwrap();
        }

        {
            let registry = Registry::new();
            registry.mount_sqlite("main", &path).unwrap();
            registry.register(&address, TextCodec.schema()).unwrap();
            let access = open(&registry);
            let found = access.project().unwrap().config::<Text>("phenix.agent/settings").unwrap()
                .get("default").unwrap().unwrap();
            assert_eq!(found.0, "persisted");
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn xdg_placement_uses_stable_identity_not_workspace_path() {
        let xdg = XdgPaths::new(
            "/tmp/config".into(), "/tmp/data".into(), "/tmp/state".into(),
            "/tmp/cache".into(), None,
        ).unwrap();
        let project = ResourceId::new(
            ScopeId::Project("stable-id".into()), StorageClass::Data, "phenix.code/index",
        ).unwrap();
        let session = ResourceId::new(
            ScopeId::Session("turn-7".into()), StorageClass::State, "phenix.agent/history",
        ).unwrap();
        assert_eq!(xdg.directory(&project).unwrap(), PathBuf::from("/tmp/data/phenix/projects/stable-id"));
        assert_eq!(xdg.directory(&session).unwrap(), PathBuf::from("/tmp/state/phenix/sessions/turn-7"));
        assert!(matches!(xdg.directory(&ResourceId::new(
            ScopeId::User("alice".into()), StorageClass::Runtime, "phenix.agent/sockets",
        ).unwrap()), Err(Error::MissingRuntimeDirectory)));
    }
}
