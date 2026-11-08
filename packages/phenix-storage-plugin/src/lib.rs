//! Optional integration between host-owned storage mounts and the existing
//! in-process plugin Registrar. No second loader, database engine or kernel API.
use phenix_plugin::{Contribution, ContributionHandle, PluginSnapshot};
use phenix_storage::{
    Backend, Error as StorageError, Observation, Registry, SqlBackend, TypedKv,
};

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
