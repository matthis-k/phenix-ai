use super::*;
use crate::{
    persistence_provider::prepare_durable_schema_set, DurableSchemaRegistration,
    PersistenceCandidateError, PersistenceError,
};

impl Kernel {
    /// Prepare the complete durable schema set for a resolved composition.
    ///
    /// Core validates namespace ownership before the provider-neutral schema
    /// materialization path negotiates features and mutates the Store.
    pub fn prepare_durable_schemas(
        &mut self,
        registrations: &[DurableSchemaRegistration],
    ) -> Result<(), KernelError> {
        let mut seen = BTreeSet::new();
        for registration in registrations {
            let namespace = &registration.schema.namespace;
            if !seen.insert(namespace.clone()) {
                return Err(KernelError::Persistence {
                    plugin: registration.owner.clone(),
                    message: format!("durable schema is declared more than once: {namespace}"),
                });
            }
            match self.config.resource_owner(namespace) {
                Some(owner) if owner == &registration.owner => {}
                Some(owner) => {
                    return Err(KernelError::Persistence {
                        plugin: registration.owner.clone(),
                        message: format!(
                            "durable namespace {namespace} is owned by {owner}, not {}",
                            registration.owner
                        ),
                    });
                }
                None => {
                    return Err(KernelError::Persistence {
                        plugin: registration.owner.clone(),
                        message: format!(
                            "durable namespace {namespace} is not declared by the resolved composition"
                        ),
                    });
                }
            }
        }

        let mut persistence = self
            .persistence
            .lock()
            .expect("persistence backend mutex poisoned");
        prepare_durable_schema_set(persistence.as_mut(), registrations).map_err(|error| match error
        {
            PersistenceCandidateError::Schema { plugin, error } => {
                persistence_error(&plugin, error)
            }
            PersistenceCandidateError::Bootstrap(_)
            | PersistenceCandidateError::Provider { .. } => {
                unreachable!("schema materialization does not resolve or open Providers")
            }
        })
    }
}

fn persistence_error(plugin: &PluginId, error: PersistenceError) -> KernelError {
    KernelError::Persistence {
        plugin: plugin.clone(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BackendFeature, DurableSchema, NamespaceTransaction, ResourceNamespace, TransactionOp,
    };
    use std::{
        collections::{BTreeMap, BTreeSet},
        sync::{Arc, Mutex},
    };

    #[derive(Default)]
    struct RecordingState {
        schemas: BTreeMap<ResourceNamespace, u32>,
        registrations: Vec<ResourceNamespace>,
        migrations: Vec<ResourceNamespace>,
    }

    struct RecordingBackend {
        features: BTreeSet<BackendFeature>,
        state: Arc<Mutex<RecordingState>>,
    }

    impl PersistenceBackend for RecordingBackend {
        fn supported_features(&self) -> BTreeSet<BackendFeature> {
            self.features.clone()
        }

        fn register_schema(
            &mut self,
            _owner: &PluginId,
            schema: &DurableSchema,
        ) -> Result<(), PersistenceError> {
            let mut state = self.state.lock().unwrap();
            state.registrations.push(schema.namespace.clone());
            match state.schemas.get(&schema.namespace).copied() {
                Some(stored) if stored != schema.version => {
                    Err(PersistenceError::IncompatibleSchema {
                        namespace: schema.namespace.clone(),
                        stored,
                        requested: schema.version,
                    })
                }
                Some(_) => Ok(()),
                None => {
                    state
                        .schemas
                        .insert(schema.namespace.clone(), schema.version);
                    Ok(())
                }
            }
        }

        fn migrate_schema(
            &mut self,
            _owner: &PluginId,
            schema: &DurableSchema,
            migrations: &[SchemaMigration],
        ) -> Result<(), PersistenceError> {
            let mut state = self.state.lock().unwrap();
            let stored = state.schemas[&schema.namespace];
            let mut version = stored;
            while version < schema.version {
                let next = version + 1;
                if !migrations.iter().any(|migration| {
                    migration.from_version == version && migration.to_version == next
                }) {
                    return Err(PersistenceError::MissingMigration {
                        namespace: schema.namespace.clone(),
                        from_version: version,
                        to_version: next,
                    });
                }
                version = next;
            }
            state
                .schemas
                .insert(schema.namespace.clone(), schema.version);
            state.migrations.push(schema.namespace.clone());
            Ok(())
        }

        fn read(
            &self,
            _caller: &PluginId,
            _namespace: &ResourceNamespace,
            _key: &str,
        ) -> Result<Option<Vec<u8>>, PersistenceError> {
            Ok(None)
        }

        fn transact_many(
            &mut self,
            _transactions: &[NamespaceTransaction],
        ) -> Result<(), PersistenceError> {
            Ok(())
        }
    }

    fn plugin(value: &str) -> PluginId {
        PluginId::parse(value).unwrap()
    }

    fn namespace(value: &str) -> ResourceNamespace {
        ResourceNamespace::parse(value).unwrap()
    }

    fn manifest(owner: &PluginId, namespace: &ResourceNamespace) -> PluginManifest {
        let mut manifest = PluginManifest::resource_only(owner.clone());
        manifest.resource_namespaces.push(namespace.clone());
        manifest
    }

    #[test]
    fn feature_negotiation_finishes_before_any_schema_registration() {
        let first_owner = plugin("first");
        let second_owner = plugin("second");
        let first_namespace = namespace("first.state");
        let second_namespace = namespace("second.state");
        let state = Arc::new(Mutex::new(RecordingState::default()));
        let backend = RecordingBackend {
            features: BTreeSet::from([BackendFeature::Transactions]),
            state: Arc::clone(&state),
        };
        let config = KernelConfig::new([
            manifest(&first_owner, &first_namespace),
            manifest(&second_owner, &second_namespace),
        ])
        .unwrap();
        let mut kernel = Kernel::with_persistence(config, backend);
        let registrations = vec![
            DurableSchemaRegistration::new(first_owner, DurableSchema::new(first_namespace, 1)),
            DurableSchemaRegistration::new(
                second_owner,
                DurableSchema::requiring(second_namespace, 1, [BackendFeature::IndexedRange]),
            ),
        ];

        assert!(matches!(
            kernel.prepare_durable_schemas(&registrations),
            Err(KernelError::Persistence { .. })
        ));
        assert!(state.lock().unwrap().registrations.is_empty());
    }

    #[test]
    fn older_schema_is_migrated_then_revalidated_at_target_version() {
        let owner = plugin("owner");
        let namespace = namespace("owner.state");
        let state = Arc::new(Mutex::new(RecordingState {
            schemas: BTreeMap::from([(namespace.clone(), 1)]),
            ..RecordingState::default()
        }));
        let backend = RecordingBackend {
            features: BTreeSet::from([BackendFeature::Migrations]),
            state: Arc::clone(&state),
        };
        let config = KernelConfig::new([manifest(&owner, &namespace)]).unwrap();
        let mut kernel = Kernel::with_persistence(config, backend);
        let registration =
            DurableSchemaRegistration::new(owner, DurableSchema::new(namespace.clone(), 2))
                .with_migrations(vec![SchemaMigration {
                    from_version: 1,
                    to_version: 2,
                    operations: Vec::<TransactionOp>::new(),
                }]);

        kernel.prepare_durable_schemas(&[registration]).unwrap();

        let state = state.lock().unwrap();
        assert_eq!(state.schemas[&namespace], 2);
        assert_eq!(state.migrations, vec![namespace.clone()]);
        assert_eq!(state.registrations, vec![namespace.clone(), namespace]);
    }
}
