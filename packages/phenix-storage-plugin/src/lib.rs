//! Optional integration between host-owned storage mounts and the existing
//! in-process plugin Registrar. No second loader, database engine or kernel API.
use phenix_plugin::{Contribution, ContributionHandle, PluginSnapshot};
use phenix_storage::{
    scoped::ResourceId, Backend, Error as StorageError, Observation, Registry, Schema,
    SqlBackend, TypedKv,
};
use std::collections::BTreeMap;

pub type OpenBackend = dyn Fn() -> Result<Box<dyn Backend>, String> + Send + Sync;

/// A plugin *offers* a backend factory. Only the trusted host can choose a
/// physical mount name, location, configuration and storage authority.
pub struct BackendOffer {
    id: String,
    open: Box<OpenBackend>,
}
impl BackendOffer {
    pub fn new(
        id: impl Into<String>,
        open: impl Fn() -> Result<Box<dyn Backend>, String> + Send + Sync + 'static,
    ) -> Self {
        Self { id: id.into(), open: Box::new(open) }
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

struct LeasedBackend {
    // Field order is deliberate: backend state and its trait methods must be
    // dropped before its source contribution and implementation are released.
    backend: Box<dyn Backend>,
    _lease: ContributionHandle<BackendOffer>,
}
impl Backend for LeasedBackend {
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
        Box::new(LeasedBackend { backend, _lease: lease }),
    )?;
    Ok(())
}


/** A declarative request for one logical resource. Independent plugins can
 * offer the same ResourceId provided their canonical Schema agrees.
 * Declaration grants no permission and never opens a database.
 */
pub struct ResourceOffer {
    id: String,
    pub resource: ResourceId,
    pub schema: Schema,
}
impl ResourceOffer {
    pub fn new(id: impl Into<String>, resource: ResourceId, schema: Schema) -> Self {
        Self { id: id.into(), resource, schema }
    }
}
impl Contribution for ResourceOffer {
    const KIND: &'static str = "storage.resource";
    fn id(&self) -> &str {
        &self.id
    }
}

/// Pure declarations after schema validation. The trusted host supplies
/// physical bindings, provisions datasets, and issues separate Session grants.
pub struct ResourceRequirements {
    entries: BTreeMap<ResourceId, Schema>,
}
impl ResourceRequirements {
    pub fn collect(snapshots: impl IntoIterator<Item = PluginSnapshot>) -> Result<Self, Error> {
        let mut entries = BTreeMap::new();
        for snapshot in snapshots {
            for (_, kind, id) in snapshot.keys() {
                if kind != ResourceOffer::KIND {
                    continue;
                }
                let offer = snapshot
                    .get::<ResourceOffer>(id)
                    .ok_or(Error::MissingResourceOffer)?;
                if let Some(previous) = entries.get(&offer.resource) {
                    if previous != &offer.schema {
                        return Err(Error::SchemaConflict);
                    }
                } else {
                    entries.insert(offer.resource.clone(), offer.schema.clone());
                }
            }
        }
        Ok(Self { entries })
    }
    pub fn iter(&self) -> impl Iterator<Item = (&ResourceId, &Schema)> {
        self.entries.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
    use phenix_storage::{
        Address, Codec, Grant, MemoryBackend, Operation, Schema, Scope,
    };

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
        use phenix_storage::scoped::{ScopeId, StorageClass};
        struct Offers(u32);
        impl PluginInstance for Offers {
            fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
                let resource = ResourceId::new(
                    ScopeId::Project("project-1".into()),
                    StorageClass::Config,
                    "phenix.agent/settings",
                ).unwrap();
                let mut schema = TextCodec.schema();
                schema.revision = self.0;
                registrar.add(ResourceOffer::new("settings", resource, schema))
            }
        }
        let mut catalog = PluginCatalog::new();
        let a = catalog.register("a", |revision: u32| Ok(Offers(revision))).unwrap();
        let b = catalog.register("b", |revision: u32| Ok(Offers(revision))).unwrap();
        let mut instances = catalog.instance();
        instances.load(&a, 1).unwrap();
        instances.load(&b, 1).unwrap();
        let requirements = ResourceRequirements::collect(instances.snapshots()).unwrap();
        assert_eq!(requirements.iter().count(), 1);

        instances.reload(&b, 2).unwrap();
        assert!(matches!(
            ResourceRequirements::collect(instances.snapshots()),
            Err(Error::SchemaConflict)
        ));
    }

    #[test]
    fn plugin_offer_mounts_and_outlives_logical_plugin_unload() {
        let mut catalog = PluginCatalog::new();
        let plugin = catalog.register("storage-provider", |(): ()| Ok(MemoryPlugin)).unwrap();
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
        session.put(&address, "key", &"value".to_string(), &TextCodec).unwrap();
        assert_eq!(session.get(&address, "key", &TextCodec).unwrap(), Some("value".into()));
    }
}
