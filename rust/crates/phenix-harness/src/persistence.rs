use crate::{HarnessBuildError, HarnessBuilder, PhenixHarness};
use phenix_core::{prepare_persistence_candidate, Kernel, PersistenceProvider, StoreBinding};
use std::collections::BTreeSet;

impl HarnessBuilder {
    /// Select an already constructed bootstrap Provider before opening its Store.
    /// Store-backed plugins start only after the resolved schemas are prepared.
    pub fn build_with_persistence_provider(
        self,
        provider: &mut dyn PersistenceProvider,
        binding: StoreBinding,
    ) -> Result<PhenixHarness, HarnessBuildError> {
        self.build_using(|resolved| {
            let prepared = prepare_persistence_candidate(
                provider,
                [],
                &BTreeSet::new(),
                binding,
                resolved.durable_schemas(),
                None,
                None,
            )?;
            Ok(Kernel::with_prepared_persistence(
                resolved.kernel_config().clone(),
                prepared,
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{
        Authority, BackendFeature, CapabilityId, DurableSchema, DurableSchemaRegistration,
        LocalPersistence, PersistenceBackend, PersistenceBootstrapDependency,
        PersistenceBootstrapError, PersistenceCandidateError, PersistenceProviderDescriptor,
        PersistenceProviderError, PluginExecution, PluginHost, PluginId, PluginInstance,
        PluginManifest, ResolvedPersistenceBootstrap, ResourceNamespace, StoreBindingId,
    };
    use std::sync::{Arc, Mutex};

    struct Provider {
        descriptor: PersistenceProviderDescriptor,
        calls: Arc<Mutex<Vec<&'static str>>>,
    }

    impl PersistenceProvider for Provider {
        fn descriptor(&self) -> PersistenceProviderDescriptor {
            self.descriptor.clone()
        }

        fn prepare(
            &mut self,
            plan: &ResolvedPersistenceBootstrap,
            active: Option<&ResolvedPersistenceBootstrap>,
        ) -> Result<Box<dyn PersistenceBackend>, PersistenceProviderError> {
            assert_eq!(plan.provider.plugin, self.descriptor.plugin);
            assert!(active.is_none());
            self.calls.lock().unwrap().push("open");
            LocalPersistence::open_in_memory()
                .map(|backend| Box::new(backend) as Box<dyn PersistenceBackend>)
                .map_err(|error| PersistenceProviderError::new(error.to_string()))
        }
    }

    struct StatePlugin {
        calls: Arc<Mutex<Vec<&'static str>>>,
    }

    impl PluginInstance for StatePlugin {
        fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
            host.read_durable(&namespace(), "record")
                .map_err(|error| error.to_string())?;
            self.calls.lock().unwrap().push("start");
            Ok(())
        }
    }

    fn namespace() -> ResourceNamespace {
        ResourceNamespace::parse("fixture.state.records").unwrap()
    }

    fn binding() -> StoreBinding {
        StoreBinding::new(StoreBindingId::parse("fixture.store").unwrap(), "sqlite-v1").unwrap()
    }

    fn fixture(feature: BackendFeature) -> (HarnessBuilder, Provider) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let owner = PluginId::parse("fixture.state").unwrap();
        let mut manifest = PluginManifest::resource_only(owner.clone());
        manifest.execution = PluginExecution::Embedded;
        manifest.resource_namespaces.push(namespace());
        manifest.maximum_authority =
            Authority::new([CapabilityId::parse("kernel.persistence.read").unwrap()]);
        let mut builder = HarnessBuilder::new();
        let plugin_calls = Arc::clone(&calls);
        builder
            .add_embedded(manifest, move || {
                Box::new(StatePlugin {
                    calls: Arc::clone(&plugin_calls),
                })
            })
            .unwrap();
        builder.add_durable_schema(DurableSchemaRegistration::new(
            owner,
            DurableSchema::requiring(namespace(), 1, [feature]),
        ));
        let provider = Provider {
            descriptor: PersistenceProviderDescriptor::new(
                PluginId::parse("fixture.persistence").unwrap(),
                [BackendFeature::Transactions],
                ["sqlite-v1".to_owned()],
            ),
            calls,
        };
        (builder, provider)
    }

    #[test]
    fn selected_provider_prepares_store_before_plugin_start() {
        let (builder, mut provider) = fixture(BackendFeature::Transactions);
        let binding = binding();
        let mut harness = builder
            .build_with_persistence_provider(&mut provider, binding.clone())
            .unwrap();

        let plan = harness.kernel().persistence_bootstrap().unwrap();
        assert_eq!(plan.provider.plugin, provider.descriptor.plugin);
        assert_eq!(plan.binding, binding);
        assert_eq!(provider.calls.lock().unwrap().as_slice(), &["open"]);
        harness.activate().unwrap();
        assert_eq!(
            provider.calls.lock().unwrap().as_slice(),
            &["open", "start"]
        );
    }

    #[test]
    fn unsupported_schema_features_prevent_store_open_and_plugin_start() {
        let (builder, mut provider) = fixture(BackendFeature::IndexedRange);
        let result = builder.build_with_persistence_provider(&mut provider, binding());

        assert!(matches!(
            result,
            Err(HarnessBuildError::Persistence(
                PersistenceCandidateError::Bootstrap(
                    PersistenceBootstrapError::UnsupportedFeatures { .. }
                )
            ))
        ));
        assert!(provider.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn bootstrap_cycle_prevents_store_open_and_plugin_start() {
        let (builder, mut provider) = fixture(BackendFeature::Transactions);
        provider.descriptor.bootstrap_dependencies =
            vec![PersistenceBootstrapDependency::TargetStore];
        let result = builder.build_with_persistence_provider(&mut provider, binding());

        assert!(matches!(
            result,
            Err(HarnessBuildError::Persistence(
                PersistenceCandidateError::Bootstrap(
                    PersistenceBootstrapError::TargetStoreBootstrapCycle(_)
                )
            ))
        ));
        assert!(provider.calls.lock().unwrap().is_empty());
    }
}
