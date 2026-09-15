use crate::{PluginId, ResourceNamespace};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    error::Error,
    fmt::{self, Display, Formatter},
    path::Path,
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum BackendFeature {
    Transactions,
    UniqueKeys,
    ForeignKeys,
    OrderedAppend,
    IndexedRange,
    Migrations,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableSchema {
    pub namespace: ResourceNamespace,
    pub version: u32,
    pub required_features: BTreeSet<BackendFeature>,
}

impl DurableSchema {
    pub fn new(namespace: ResourceNamespace, version: u32) -> Self {
        Self {
            namespace,
            version,
            required_features: BTreeSet::new(),
        }
    }

    pub fn requiring(
        namespace: ResourceNamespace,
        version: u32,
        features: impl IntoIterator<Item = BackendFeature>,
    ) -> Self {
        Self {
            namespace,
            version,
            required_features: features.into_iter().collect(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TransactionOp {
    Put {
        key: String,
        value: Vec<u8>,
    },
    Delete {
        key: String,
    },
    AssertValue {
        key: String,
        expected: Option<Vec<u8>>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NamespaceTransaction {
    pub owner: PluginId,
    pub namespace: ResourceNamespace,
    pub operations: Vec<TransactionOp>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaMigration {
    pub from_version: u32,
    pub to_version: u32,
    pub operations: Vec<TransactionOp>,
}

#[derive(Debug)]
pub enum PersistenceError {
    Sql(rusqlite::Error),
    NamespaceCollision {
        namespace: ResourceNamespace,
        owner: PluginId,
    },
    IncompatibleSchema {
        namespace: ResourceNamespace,
        stored: u32,
        requested: u32,
    },
    MissingMigration {
        namespace: ResourceNamespace,
        from_version: u32,
        to_version: u32,
    },
    UnsupportedFeature {
        namespace: ResourceNamespace,
        feature: BackendFeature,
    },
    UnregisteredNamespace(ResourceNamespace),
    WrongNamespaceOwner {
        namespace: ResourceNamespace,
        owner: PluginId,
        caller: PluginId,
    },
    AssertionFailed {
        namespace: ResourceNamespace,
        key: String,
    },
}

impl Display for PersistenceError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sql(error) => write!(f, "local persistence: {error}"),
            Self::NamespaceCollision { namespace, owner } => {
                write!(f, "durable namespace {namespace} is owned by {owner}")
            }
            Self::IncompatibleSchema {
                namespace,
                stored,
                requested,
            } => write!(
                f,
                "durable namespace {namespace} has schema {stored}, requested {requested}"
            ),
            Self::MissingMigration {
                namespace,
                from_version,
                to_version,
            } => write!(
                f,
                "durable namespace {namespace} is missing migration {from_version}->{to_version}"
            ),
            Self::UnsupportedFeature { namespace, feature } => write!(
                f,
                "durable namespace {namespace} requires unsupported backend feature {feature:?}"
            ),
            Self::UnregisteredNamespace(namespace) => {
                write!(f, "durable namespace is not registered: {namespace}")
            }
            Self::WrongNamespaceOwner {
                namespace,
                owner,
                caller,
            } => write!(
                f,
                "plugin {caller} cannot mutate namespace {namespace} owned by {owner}"
            ),
            Self::AssertionFailed { namespace, key } => {
                write!(f, "transaction assertion failed for {namespace}/{key}")
            }
        }
    }
}

impl Error for PersistenceError {}

impl From<rusqlite::Error> for PersistenceError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sql(value)
    }
}

pub trait PersistenceBackend: Send {
    fn supported_features(&self) -> BTreeSet<BackendFeature>;

    fn register_schema(
        &mut self,
        owner: &PluginId,
        schema: &DurableSchema,
    ) -> Result<(), PersistenceError>;

    fn migrate_schema(
        &mut self,
        _owner: &PluginId,
        schema: &DurableSchema,
        _migrations: &[SchemaMigration],
    ) -> Result<(), PersistenceError> {
        Err(PersistenceError::UnsupportedFeature {
            namespace: schema.namespace.clone(),
            feature: BackendFeature::Migrations,
        })
    }

    fn read(
        &self,
        caller: &PluginId,
        namespace: &ResourceNamespace,
        key: &str,
    ) -> Result<Option<Vec<u8>>, PersistenceError>;

    fn transact_many(
        &mut self,
        transactions: &[NamespaceTransaction],
    ) -> Result<(), PersistenceError>;

    fn transact(
        &mut self,
        caller: &PluginId,
        namespace: &ResourceNamespace,
        operations: &[TransactionOp],
    ) -> Result<(), PersistenceError> {
        self.transact_many(&[NamespaceTransaction {
            owner: caller.clone(),
            namespace: namespace.clone(),
            operations: operations.to_vec(),
        }])
    }
}

pub struct LocalPersistence {
    connection: Connection,
}

impl LocalPersistence {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, PersistenceError> {
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    pub fn open_in_memory() -> Result<Self, PersistenceError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> Result<Self, PersistenceError> {
        connection.execute_batch(
            "
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS kernel_plugin_schemas (
                namespace TEXT PRIMARY KEY,
                owner TEXT NOT NULL,
                version INTEGER NOT NULL CHECK(version > 0)
            );
            CREATE TABLE IF NOT EXISTS kernel_plugin_records (
                namespace TEXT NOT NULL,
                record_key TEXT NOT NULL,
                record_value BLOB NOT NULL,
                PRIMARY KEY(namespace, record_key),
                FOREIGN KEY(namespace) REFERENCES kernel_plugin_schemas(namespace)
            );
            ",
        )?;
        Ok(Self { connection })
    }

    fn require_owner(
        &self,
        caller: &PluginId,
        namespace: &ResourceNamespace,
    ) -> Result<(), PersistenceError> {
        let owner = self
            .connection
            .query_row(
                "SELECT owner FROM kernel_plugin_schemas WHERE namespace = ?1",
                [namespace.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| PersistenceError::UnregisteredNamespace(namespace.clone()))?;

        if owner == caller.as_str() {
            return Ok(());
        }

        Err(PersistenceError::WrongNamespaceOwner {
            namespace: namespace.clone(),
            owner: PluginId::parse(owner)
                .expect("persisted plugin IDs were validated at registration"),
            caller: caller.clone(),
        })
    }

    fn require_features(&self, schema: &DurableSchema) -> Result<(), PersistenceError> {
        let supported = self.supported_features();
        if let Some(feature) = schema
            .required_features
            .iter()
            .find(|feature| !supported.contains(feature))
        {
            return Err(PersistenceError::UnsupportedFeature {
                namespace: schema.namespace.clone(),
                feature: *feature,
            });
        }
        Ok(())
    }

    fn stored_owner_and_version(
        &self,
        namespace: &ResourceNamespace,
    ) -> Result<Option<(PluginId, u32)>, PersistenceError> {
        self.connection
            .query_row(
                "SELECT owner, version FROM kernel_plugin_schemas WHERE namespace = ?1",
                [namespace.as_str()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
            .map(|(owner, version)| {
                Ok((
                    PluginId::parse(owner)
                        .expect("persisted plugin IDs were validated at registration"),
                    u32::try_from(version).unwrap_or(u32::MAX),
                ))
            })
            .transpose()
    }
}

impl PersistenceBackend for LocalPersistence {
    fn supported_features(&self) -> BTreeSet<BackendFeature> {
        [
            BackendFeature::Transactions,
            BackendFeature::UniqueKeys,
            BackendFeature::Migrations,
        ]
        .into_iter()
        .collect()
    }

    fn register_schema(
        &mut self,
        owner: &PluginId,
        schema: &DurableSchema,
    ) -> Result<(), PersistenceError> {
        self.require_features(schema)?;
        match self.stored_owner_and_version(&schema.namespace)? {
            None => {
                self.connection.execute(
                    "INSERT INTO kernel_plugin_schemas(namespace, owner, version) VALUES (?1, ?2, ?3)",
                    params![schema.namespace.as_str(), owner.as_str(), schema.version],
                )?;
                Ok(())
            }
            Some((stored_owner, _)) if &stored_owner != owner => {
                Err(PersistenceError::NamespaceCollision {
                    namespace: schema.namespace.clone(),
                    owner: stored_owner,
                })
            }
            Some((_, version)) if version != schema.version => {
                Err(PersistenceError::IncompatibleSchema {
                    namespace: schema.namespace.clone(),
                    stored: version,
                    requested: schema.version,
                })
            }
            Some(_) => Ok(()),
        }
    }

    fn migrate_schema(
        &mut self,
        owner: &PluginId,
        schema: &DurableSchema,
        migrations: &[SchemaMigration],
    ) -> Result<(), PersistenceError> {
        self.require_features(schema)?;
        let Some((stored_owner, stored_version)) =
            self.stored_owner_and_version(&schema.namespace)?
        else {
            return Err(PersistenceError::UnregisteredNamespace(
                schema.namespace.clone(),
            ));
        };
        if &stored_owner != owner {
            return Err(PersistenceError::WrongNamespaceOwner {
                namespace: schema.namespace.clone(),
                owner: stored_owner,
                caller: owner.clone(),
            });
        }
        if stored_version > schema.version {
            return Err(PersistenceError::IncompatibleSchema {
                namespace: schema.namespace.clone(),
                stored: stored_version,
                requested: schema.version,
            });
        }
        if stored_version == schema.version {
            return Ok(());
        }

        let mut current_version = stored_version;
        let mut plan = Vec::new();
        while current_version < schema.version {
            let next_version = current_version + 1;
            let Some(migration) = migrations.iter().find(|migration| {
                migration.from_version == current_version && migration.to_version == next_version
            }) else {
                return Err(PersistenceError::MissingMigration {
                    namespace: schema.namespace.clone(),
                    from_version: current_version,
                    to_version: next_version,
                });
            };
            plan.push(migration);
            current_version = next_version;
        }

        let transaction = self.connection.transaction()?;
        for migration in plan {
            apply_operations(&transaction, &schema.namespace, &migration.operations)?;
        }
        transaction.execute(
            "UPDATE kernel_plugin_schemas SET version = ?2 WHERE namespace = ?1",
            params![schema.namespace.as_str(), schema.version],
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn read(
        &self,
        caller: &PluginId,
        namespace: &ResourceNamespace,
        key: &str,
    ) -> Result<Option<Vec<u8>>, PersistenceError> {
        self.require_owner(caller, namespace)?;
        Ok(self
            .connection
            .query_row(
                "SELECT record_value FROM kernel_plugin_records WHERE namespace = ?1 AND record_key = ?2",
                params![namespace.as_str(), key],
                |row| row.get(0),
            )
            .optional()?)
    }

    fn transact_many(
        &mut self,
        transactions: &[NamespaceTransaction],
    ) -> Result<(), PersistenceError> {
        for participant in transactions {
            self.require_owner(&participant.owner, &participant.namespace)?;
        }

        let transaction = self.connection.transaction()?;
        for participant in transactions {
            apply_operations(
                &transaction,
                &participant.namespace,
                &participant.operations,
            )?;
        }
        transaction.commit()?;
        Ok(())
    }
}

fn apply_operations(
    transaction: &Transaction<'_>,
    namespace: &ResourceNamespace,
    operations: &[TransactionOp],
) -> Result<(), PersistenceError> {
    for operation in operations {
        match operation {
            TransactionOp::Put { key, value } => {
                transaction.execute(
                    "
                    INSERT INTO kernel_plugin_records(namespace, record_key, record_value)
                    VALUES (?1, ?2, ?3)
                    ON CONFLICT(namespace, record_key)
                    DO UPDATE SET record_value = excluded.record_value
                    ",
                    params![namespace.as_str(), key, value],
                )?;
            }
            TransactionOp::Delete { key } => {
                transaction.execute(
                    "DELETE FROM kernel_plugin_records WHERE namespace = ?1 AND record_key = ?2",
                    params![namespace.as_str(), key],
                )?;
            }
            TransactionOp::AssertValue { key, expected } => {
                let actual = transaction
                    .query_row(
                        "SELECT record_value FROM kernel_plugin_records WHERE namespace = ?1 AND record_key = ?2",
                        params![namespace.as_str(), key],
                        |row| row.get::<_, Vec<u8>>(0),
                    )
                    .optional()?;
                if &actual != expected {
                    return Err(PersistenceError::AssertionFailed {
                        namespace: namespace.clone(),
                        key: key.clone(),
                    });
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn plugin(value: &str) -> PluginId {
        PluginId::parse(value).unwrap()
    }

    fn namespace(value: &str) -> ResourceNamespace {
        ResourceNamespace::parse(value).unwrap()
    }

    #[test]
    fn namespace_ownership_and_schema_compatibility_are_enforced() {
        let mut store = LocalPersistence::open_in_memory().unwrap();
        let owner = plugin("owner");
        let other = plugin("other");
        let schema = DurableSchema::new(namespace("owner.state"), 1);

        store.register_schema(&owner, &schema).unwrap();
        assert!(matches!(
            store.register_schema(&other, &schema),
            Err(PersistenceError::NamespaceCollision { .. })
        ));

        let incompatible = DurableSchema::new(schema.namespace.clone(), 2);
        assert!(matches!(
            store.register_schema(&owner, &incompatible),
            Err(PersistenceError::IncompatibleSchema { .. })
        ));
    }

    #[test]
    fn unsupported_backend_feature_is_rejected_before_schema_registration() {
        let mut store = LocalPersistence::open_in_memory().unwrap();
        let schema =
            DurableSchema::requiring(namespace("owner.state"), 1, [BackendFeature::IndexedRange]);

        assert!(matches!(
            store.register_schema(&plugin("owner"), &schema),
            Err(PersistenceError::UnsupportedFeature {
                feature: BackendFeature::IndexedRange,
                ..
            })
        ));
    }

    #[test]
    fn failed_transaction_rolls_back_all_writes() {
        let mut store = LocalPersistence::open_in_memory().unwrap();
        let owner = plugin("owner");
        let namespace = namespace("owner.state");
        store
            .register_schema(&owner, &DurableSchema::new(namespace.clone(), 1))
            .unwrap();

        let error = store
            .transact(
                &owner,
                &namespace,
                &[
                    TransactionOp::Put {
                        key: "first".into(),
                        value: b"value".to_vec(),
                    },
                    TransactionOp::AssertValue {
                        key: "missing".into(),
                        expected: Some(b"expected".to_vec()),
                    },
                ],
            )
            .unwrap_err();
        assert!(matches!(error, PersistenceError::AssertionFailed { .. }));
        assert_eq!(store.read(&owner, &namespace, "first").unwrap(), None);
    }

    #[test]
    fn multi_plugin_transaction_commits_or_rolls_back_as_one_unit() {
        let mut store = LocalPersistence::open_in_memory().unwrap();
        let first_owner = plugin("first");
        let second_owner = plugin("second");
        let first_namespace = namespace("first.state");
        let second_namespace = namespace("second.state");
        store
            .register_schema(
                &first_owner,
                &DurableSchema::new(first_namespace.clone(), 1),
            )
            .unwrap();
        store
            .register_schema(
                &second_owner,
                &DurableSchema::new(second_namespace.clone(), 1),
            )
            .unwrap();

        let failed = store.transact_many(&[
            NamespaceTransaction {
                owner: first_owner.clone(),
                namespace: first_namespace.clone(),
                operations: vec![TransactionOp::Put {
                    key: "record".into(),
                    value: b"first".to_vec(),
                }],
            },
            NamespaceTransaction {
                owner: second_owner.clone(),
                namespace: second_namespace.clone(),
                operations: vec![TransactionOp::AssertValue {
                    key: "missing".into(),
                    expected: Some(b"expected".to_vec()),
                }],
            },
        ]);
        assert!(matches!(
            failed,
            Err(PersistenceError::AssertionFailed { .. })
        ));
        assert_eq!(
            store
                .read(&first_owner, &first_namespace, "record")
                .unwrap(),
            None
        );

        store
            .transact_many(&[
                NamespaceTransaction {
                    owner: first_owner.clone(),
                    namespace: first_namespace.clone(),
                    operations: vec![TransactionOp::Put {
                        key: "record".into(),
                        value: b"first".to_vec(),
                    }],
                },
                NamespaceTransaction {
                    owner: second_owner.clone(),
                    namespace: second_namespace.clone(),
                    operations: vec![TransactionOp::Put {
                        key: "record".into(),
                        value: b"second".to_vec(),
                    }],
                },
            ])
            .unwrap();

        assert_eq!(
            store
                .read(&first_owner, &first_namespace, "record")
                .unwrap(),
            Some(b"first".to_vec())
        );
        assert_eq!(
            store
                .read(&second_owner, &second_namespace, "record")
                .unwrap(),
            Some(b"second".to_vec())
        );
    }

    #[test]
    fn schema_migration_is_transactional_and_version_gated() {
        let mut store = LocalPersistence::open_in_memory().unwrap();
        let owner = plugin("owner");
        let namespace = namespace("owner.state");
        store
            .register_schema(&owner, &DurableSchema::new(namespace.clone(), 1))
            .unwrap();
        store
            .transact(
                &owner,
                &namespace,
                &[TransactionOp::Put {
                    key: "old".into(),
                    value: b"value".to_vec(),
                }],
            )
            .unwrap();

        let target = DurableSchema::requiring(namespace.clone(), 2, [BackendFeature::Migrations]);
        store
            .migrate_schema(
                &owner,
                &target,
                &[SchemaMigration {
                    from_version: 1,
                    to_version: 2,
                    operations: vec![
                        TransactionOp::AssertValue {
                            key: "old".into(),
                            expected: Some(b"value".to_vec()),
                        },
                        TransactionOp::Put {
                            key: "new".into(),
                            value: b"migrated".to_vec(),
                        },
                    ],
                }],
            )
            .unwrap();

        store.register_schema(&owner, &target).unwrap();
        assert_eq!(
            store.read(&owner, &namespace, "new").unwrap(),
            Some(b"migrated".to_vec())
        );

        let failed_target =
            DurableSchema::requiring(namespace.clone(), 3, [BackendFeature::Migrations]);
        let error = store
            .migrate_schema(
                &owner,
                &failed_target,
                &[SchemaMigration {
                    from_version: 2,
                    to_version: 3,
                    operations: vec![
                        TransactionOp::Put {
                            key: "partial".into(),
                            value: b"must-rollback".to_vec(),
                        },
                        TransactionOp::AssertValue {
                            key: "missing".into(),
                            expected: Some(b"expected".to_vec()),
                        },
                    ],
                }],
            )
            .unwrap_err();
        assert!(matches!(error, PersistenceError::AssertionFailed { .. }));
        store
            .register_schema(&owner, &DurableSchema::new(namespace.clone(), 2))
            .unwrap();
        assert_eq!(store.read(&owner, &namespace, "partial").unwrap(), None);
    }

    #[test]
    fn missing_schema_migration_fails_before_mutation() {
        let mut store = LocalPersistence::open_in_memory().unwrap();
        let owner = plugin("owner");
        let namespace = namespace("owner.state");
        store
            .register_schema(&owner, &DurableSchema::new(namespace.clone(), 1))
            .unwrap();

        let target = DurableSchema::requiring(namespace.clone(), 3, [BackendFeature::Migrations]);
        assert!(matches!(
            store.migrate_schema(
                &owner,
                &target,
                &[SchemaMigration {
                    from_version: 1,
                    to_version: 2,
                    operations: vec![TransactionOp::Put {
                        key: "partial".into(),
                        value: b"must-not-run".to_vec(),
                    }],
                }],
            ),
            Err(PersistenceError::MissingMigration {
                from_version: 2,
                to_version: 3,
                ..
            })
        ));
        store
            .register_schema(&owner, &DurableSchema::new(namespace.clone(), 1))
            .unwrap();
        assert_eq!(store.read(&owner, &namespace, "partial").unwrap(), None);
    }

    #[test]
    fn persisted_plugin_state_survives_local_backend_restart() {
        let path = std::env::temp_dir().join(format!(
            "phenix-kernel-persistence-{}-{}.sqlite",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let owner = plugin("owner");
        let namespace = namespace("owner.state");

        {
            let mut store = LocalPersistence::open(&path).unwrap();
            store
                .register_schema(&owner, &DurableSchema::new(namespace.clone(), 1))
                .unwrap();
            store
                .transact(
                    &owner,
                    &namespace,
                    &[TransactionOp::Put {
                        key: "record".into(),
                        value: b"durable".to_vec(),
                    }],
                )
                .unwrap();
        }

        let restored = LocalPersistence::open(&path).unwrap();
        assert_eq!(
            restored.read(&owner, &namespace, "record").unwrap(),
            Some(b"durable".to_vec())
        );
        fs::remove_file(path).unwrap();
    }
}
