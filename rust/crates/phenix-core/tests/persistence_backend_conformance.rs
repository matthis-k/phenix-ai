use phenix_core::{
    BackendFeature, DurableKeyRange, DurableRecord, DurableSchema, LocalPersistence,
    NamespaceTransaction, PersistenceBackend, PersistenceError, PluginId, ResourceNamespace,
    ScanDirection, SchemaMigration, TransactionOp,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Bound,
};

fn plugin(value: &str) -> PluginId {
    PluginId::parse(value).unwrap()
}

fn namespace(value: &str) -> ResourceNamespace {
    ResourceNamespace::parse(value).unwrap()
}

#[derive(Clone, Debug)]
struct MemoryPersistence {
    schemas: BTreeMap<ResourceNamespace, (PluginId, u32)>,
    records: BTreeMap<(ResourceNamespace, String), Vec<u8>>,
    supports_migrations: bool,
}

impl Default for MemoryPersistence {
    fn default() -> Self {
        Self {
            schemas: BTreeMap::new(),
            records: BTreeMap::new(),
            supports_migrations: true,
        }
    }
}

impl MemoryPersistence {
    fn without_migrations() -> Self {
        Self {
            supports_migrations: false,
            ..Self::default()
        }
    }
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
        if self.supports_migrations {
            BTreeSet::from([BackendFeature::Migrations])
        } else {
            BTreeSet::new()
        }
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

    fn scan(
        &self,
        caller: &PluginId,
        namespace: &ResourceNamespace,
        range: &DurableKeyRange,
        direction: ScanDirection,
        limit: Option<usize>,
    ) -> Result<Vec<DurableRecord>, PersistenceError> {
        self.require_owner(caller, namespace)?;
        let in_range = |key: &str| {
            let above_lower = match range.lower() {
                Bound::Included(bound) => key >= bound.as_str(),
                Bound::Excluded(bound) => key > bound.as_str(),
                Bound::Unbounded => true,
            };
            let below_upper = match range.upper() {
                Bound::Included(bound) => key <= bound.as_str(),
                Bound::Excluded(bound) => key < bound.as_str(),
                Bound::Unbounded => true,
            };
            above_lower && below_upper
        };
        let mut records: Vec<_> = self
            .records
            .iter()
            .filter(|((record_namespace, key), _)| record_namespace == namespace && in_range(key))
            .map(|((_, key), value)| DurableRecord {
                key: key.clone(),
                value: value.clone(),
            })
            .collect();
        if direction == ScanDirection::Reverse {
            records.reverse();
        }
        if let Some(limit) = limit {
            records.truncate(limit);
        }
        Ok(records)
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
            &DurableSchema::new(first_namespace.clone(), 1),
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

    backend
        .transact(
            &first_owner,
            &first_namespace,
            &[
                TransactionOp::Put {
                    key: "alpha".into(),
                    value: b"a".to_vec(),
                },
                TransactionOp::Put {
                    key: "beta".into(),
                    value: b"b".to_vec(),
                },
                TransactionOp::Put {
                    key: "beta-2".into(),
                    value: b"b2".to_vec(),
                },
                TransactionOp::Put {
                    key: "gamma".into(),
                    value: b"g".to_vec(),
                },
            ],
        )
        .unwrap();

    let bounded = backend
        .scan(
            &first_owner,
            &first_namespace,
            &DurableKeyRange::new(
                Bound::Included("alpha".into()),
                Bound::Excluded("gamma".into()),
            ),
            ScanDirection::Forward,
            Some(10),
        )
        .unwrap();
    assert_eq!(
        bounded
            .iter()
            .map(|record| record.key.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha", "beta", "beta-2"]
    );

    let unbounded = backend
        .scan(
            &first_owner,
            &first_namespace,
            &DurableKeyRange::all(),
            ScanDirection::Forward,
            None,
        )
        .unwrap();
    assert_eq!(
        unbounded
            .iter()
            .map(|record| record.key.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha", "beta", "beta-2", "gamma", "record"]
    );

    let prefixed = backend
        .scan(
            &first_owner,
            &first_namespace,
            &DurableKeyRange::prefix("beta"),
            ScanDirection::Forward,
            Some(10),
        )
        .unwrap();
    assert_eq!(
        prefixed
            .iter()
            .map(|record| record.key.as_str())
            .collect::<Vec<_>>(),
        vec!["beta", "beta-2"]
    );

    let reverse = backend
        .scan(
            &first_owner,
            &first_namespace,
            &DurableKeyRange::all(),
            ScanDirection::Reverse,
            Some(2),
        )
        .unwrap();
    assert_eq!(
        reverse
            .iter()
            .map(|record| record.key.as_str())
            .collect::<Vec<_>>(),
        vec!["record", "gamma"]
    );
    assert!(backend
        .scan(
            &first_owner,
            &first_namespace,
            &DurableKeyRange::all(),
            ScanDirection::Forward,
            Some(0),
        )
        .unwrap()
        .is_empty());
    assert!(matches!(
        backend.scan(
            &outsider,
            &first_namespace,
            &DurableKeyRange::all(),
            ScanDirection::Forward,
            Some(1),
        ),
        Err(PersistenceError::WrongNamespaceOwner { .. })
    ));
    assert_eq!(
        backend
            .scan(
                &second_owner,
                &second_namespace,
                &DurableKeyRange::all(),
                ScanDirection::Forward,
                Some(10),
            )
            .unwrap(),
        vec![DurableRecord {
            key: "record".into(),
            value: b"second".to_vec(),
        }]
    );

    let migrated_schema =
        DurableSchema::requiring(first_namespace.clone(), 2, [BackendFeature::Migrations]);
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
        DurableSchema::requiring(namespace("feature.test"), 1, [BackendFeature::Migrations]);
    assert!(matches!(
        backend.register_schema(&plugin("owner"), &requested),
        Err(PersistenceError::UnsupportedFeature {
            feature: BackendFeature::Migrations,
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
    assert_unsupported_feature(MemoryPersistence::without_migrations());
}
