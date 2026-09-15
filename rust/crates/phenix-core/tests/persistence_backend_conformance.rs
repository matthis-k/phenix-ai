use phenix_core::{
    BackendFeature, DurableSchema, LocalPersistence, NamespaceTransaction, PersistenceBackend,
    PersistenceError, PluginId, ResourceNamespace, SchemaMigration, TransactionOp,
};
use std::collections::{BTreeMap, BTreeSet};

fn plugin(value: &str) -> PluginId {
    PluginId::parse(value).unwrap()
}

fn namespace(value: &str) -> ResourceNamespace {
    ResourceNamespace::parse(value).unwrap()
}

#[derive(Clone, Debug, Default)]
struct MemoryPersistence {
    schemas: BTreeMap<ResourceNamespace, (PluginId, u32)>,
    records: BTreeMap<(ResourceNamespace, String), Vec<u8>>,
}

impl MemoryPersistence {
    fn require_owner(
        &self,
        caller: &PluginId,
        namespace: &ResourceNamespace,
    ) -> Result<(), PersistenceError> {
        let Some((owner, _)) = self.schemas.get(namespace) else {
            return Err(PersistenceError::UnregisteredNamespace(namespace.clone()));
        };
        if owner == caller {
            return Ok(());
        }
        Err(PersistenceError::WrongNamespaceOwner {
            namespace: namespace.clone(),
            owner: owner.clone(),
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

    fn apply_operations(
        records: &mut BTreeMap<(ResourceNamespace, String), Vec<u8>>,
        namespace: &ResourceNamespace,
        operations: &[TransactionOp],
    ) -> Result<(), PersistenceError> {
        for operation in operations {
            let record_key = |key: &str| (namespace.clone(), key.to_owned());
            match operation {
                TransactionOp::Put { key, value } => {
                    records.insert(record_key(key), value.clone());
                }
                TransactionOp::Delete { key } => {
                    records.remove(&record_key(key));
                }
                TransactionOp::AssertValue { key, expected } => {
                    let actual = records.get(&record_key(key)).cloned();
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
}

impl PersistenceBackend for MemoryPersistence {
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
        match self.schemas.get(&schema.namespace) {
            None => {
                self.schemas
                    .insert(schema.namespace.clone(), (owner.clone(), schema.version));
                Ok(())
            }
            Some((stored_owner, _)) if stored_owner != owner => {
                Err(PersistenceError::NamespaceCollision {
                    namespace: schema.namespace.clone(),
                    owner: stored_owner.clone(),
                })
            }
            Some((_, stored_version)) if *stored_version != schema.version => {
                Err(PersistenceError::IncompatibleSchema {
                    namespace: schema.namespace.clone(),
                    stored: *stored_version,
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
        self.require_owner(owner, &schema.namespace)?;
        let stored_version = self.schemas[&schema.namespace].1;
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

        let mut current = stored_version;
        let mut plan = Vec::new();
        while current < schema.version {
            let next = current + 1;
            let Some(migration) = migrations.iter().find(|migration| {
                migration.from_version == current && migration.to_version == next
            }) else {
                return Err(PersistenceError::MissingMigration {
                    namespace: schema.namespace.clone(),
                    from_version: current,
                    to_version: next,
                });
            };
            plan.push(migration);
            current = next;
        }

        let mut staged = self.records.clone();
        for migration in plan {
            Self::apply_operations(&mut staged, &schema.namespace, &migration.operations)?;
        }
        self.records = staged;
        self.schemas
            .get_mut(&schema.namespace)
            .expect("registered schema exists")
            .1 = schema.version;
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
            .records
            .get(&(namespace.clone(), key.to_owned()))
            .cloned())
    }

    fn transact_many(
        &mut self,
        transactions: &[NamespaceTransaction],
    ) -> Result<(), PersistenceError> {
        for participant in transactions {
            self.require_owner(&participant.owner, &participant.namespace)?;
        }

        let mut staged = self.records.clone();
        for participant in transactions {
            Self::apply_operations(&mut staged, &participant.namespace, &participant.operations)?;
        }
        self.records = staged;
        Ok(())
    }
}

fn assert_backend_conformance(mut backend: impl PersistenceBackend) {
    let first_owner = plugin("first");
    let second_owner = plugin("second");
    let outsider = plugin("outsider");
    let first_namespace = namespace("first.state");
    let second_namespace = namespace("second.state");

    backend
        .register_schema(
            &first_owner,
            &DurableSchema::requiring(
                first_namespace.clone(),
                1,
                [BackendFeature::Transactions, BackendFeature::UniqueKeys],
            ),
        )
        .unwrap();
    backend
        .register_schema(
            &second_owner,
            &DurableSchema::new(second_namespace.clone(), 1),
        )
        .unwrap();

    assert!(matches!(
        backend.read(&outsider, &first_namespace, "record"),
        Err(PersistenceError::WrongNamespaceOwner { .. })
    ));

    let failed = backend.transact_many(&[
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
        backend
            .read(&first_owner, &first_namespace, "record")
            .unwrap(),
        None
    );

    backend
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
        backend
            .read(&first_owner, &first_namespace, "record")
            .unwrap(),
        Some(b"first".to_vec())
    );
    assert_eq!(
        backend
            .read(&second_owner, &second_namespace, "record")
            .unwrap(),
        Some(b"second".to_vec())
    );

    let migrated_schema = DurableSchema::requiring(
        first_namespace.clone(),
        2,
        [BackendFeature::Transactions, BackendFeature::Migrations],
    );
    backend
        .migrate_schema(
            &first_owner,
            &migrated_schema,
            &[SchemaMigration {
                from_version: 1,
                to_version: 2,
                operations: vec![
                    TransactionOp::AssertValue {
                        key: "record".into(),
                        expected: Some(b"first".to_vec()),
                    },
                    TransactionOp::Put {
                        key: "migrated".into(),
                        value: b"yes".to_vec(),
                    },
                ],
            }],
        )
        .unwrap();
    backend
        .register_schema(&first_owner, &migrated_schema)
        .unwrap();
    assert_eq!(
        backend
            .read(&first_owner, &first_namespace, "migrated")
            .unwrap(),
        Some(b"yes".to_vec())
    );
}

fn assert_unsupported_feature(mut backend: impl PersistenceBackend) {
    let requested =
        DurableSchema::requiring(namespace("feature.test"), 1, [BackendFeature::IndexedRange]);
    assert!(matches!(
        backend.register_schema(&plugin("owner"), &requested),
        Err(PersistenceError::UnsupportedFeature {
            feature: BackendFeature::IndexedRange,
            ..
        })
    ));
}

#[test]
fn local_sqlite_backend_matches_generic_persistence_contract() {
    assert_backend_conformance(LocalPersistence::open_in_memory().unwrap());
}

#[test]
fn alternate_memory_backend_matches_generic_persistence_contract() {
    assert_backend_conformance(MemoryPersistence::default());
}

#[test]
fn unsupported_features_fail_before_namespace_claim() {
    assert_unsupported_feature(LocalPersistence::open_in_memory().unwrap());
    assert_unsupported_feature(MemoryPersistence::default());
}
