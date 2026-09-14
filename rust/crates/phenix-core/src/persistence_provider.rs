use crate::{
    resolve_persistence_bootstrap, BackendFeature, DurableSchemaRegistration, PersistenceBackend,
    PersistenceBootstrapError, PersistenceError, PersistenceProviderDescriptor,
    PersistenceProviderTransition, PluginId, ResolvedPersistenceBootstrap, StoreBinding,
};
use std::collections::BTreeSet;

/// Infrastructure provider capable of opening one Store Binding.
///
/// Provider-native handles remain behind the returned `PersistenceBackend`.
/// Resolution and feature negotiation happen before `prepare` is called.
/// Preparation must execute the selected transition or return an error. Candidate
/// writes must remain isolated from the active Store until activation succeeds.
pub trait PersistenceProvider: Send {
    fn descriptor(&self) -> PersistenceProviderDescriptor;

    fn prepare(
        &mut self,
        plan: &ResolvedPersistenceBootstrap,
        active: Option<&ResolvedPersistenceBootstrap>,
    ) -> Result<Box<dyn PersistenceBackend>, PersistenceProviderError>;
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{message}")]
pub struct PersistenceProviderError {
    pub message: String,
}

impl PersistenceProviderError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PersistenceCandidateError {
    #[error(transparent)]
    Bootstrap(#[from] PersistenceBootstrapError),
    #[error("Persistence Provider {provider} preparation failed: {error}")]
    Provider {
        provider: PluginId,
        #[source]
        error: PersistenceProviderError,
    },
    #[error("durable schema preparation for {plugin} failed: {error}")]
    Schema {
        plugin: PluginId,
        #[source]
        error: PersistenceError,
    },
}

pub struct PreparedPersistence {
    plan: ResolvedPersistenceBootstrap,
    backend: Box<dyn PersistenceBackend>,
}

impl PreparedPersistence {
    #[must_use]
    pub fn plan(&self) -> &ResolvedPersistenceBootstrap {
        &self.plan
    }

    /// Consume the prepared candidate as one atomic plan/backend pair.
    ///
    /// The backend is already opened and schema-prepared for exactly this plan.
    #[must_use]
    pub fn into_parts(self) -> (ResolvedPersistenceBootstrap, Box<dyn PersistenceBackend>) {
        (self.plan, self.backend)
    }
}

/// Resolve and fully prepare a persistence candidate without mutating active
/// kernel state. Store opening occurs only after bootstrap eligibility is known.
pub fn prepare_persistence_candidate(
    provider: &mut dyn PersistenceProvider,
    other_providers: impl IntoIterator<Item = PersistenceProviderDescriptor>,
    pre_store_plugins: &BTreeSet<PluginId>,
    binding: StoreBinding,
    schemas: &[DurableSchemaRegistration],
    active: Option<&ResolvedPersistenceBootstrap>,
    transition: Option<PersistenceProviderTransition>,
) -> Result<PreparedPersistence, PersistenceCandidateError> {
    let selected = provider.descriptor();
    let selected_id = selected.plugin.clone();
    let descriptors = std::iter::once(selected)
        .chain(other_providers)
        .collect::<Vec<_>>();
    let plan = resolve_persistence_bootstrap(
        &selected_id,
        descriptors,
        pre_store_plugins,
        binding,
        schemas,
        active,
        transition,
    )?;

    let mut backend =
        provider
            .prepare(&plan, active)
            .map_err(|error| PersistenceCandidateError::Provider {
                provider: selected_id,
                error,
            })?;
    prepare_durable_schema_set(backend.as_mut(), schemas)?;
    Ok(PreparedPersistence { plan, backend })
}

pub(crate) fn prepare_durable_schema_set(
    backend: &mut dyn PersistenceBackend,
    registrations: &[DurableSchemaRegistration],
) -> Result<(), PersistenceCandidateError> {
    let supported = backend.supported_features();
    for registration in registrations {
        for feature in &registration.schema.required_features {
            if !supported.contains(feature) {
                return Err(schema_error(
                    registration,
                    PersistenceError::UnsupportedFeature {
                        namespace: registration.schema.namespace.clone(),
                        feature: *feature,
                    },
                ));
            }
        }
        if !registration.migrations.is_empty() && !supported.contains(&BackendFeature::Migrations) {
            return Err(schema_error(
                registration,
                PersistenceError::UnsupportedFeature {
                    namespace: registration.schema.namespace.clone(),
                    feature: BackendFeature::Migrations,
                },
            ));
        }
    }

    for registration in registrations {
        match backend.register_schema(&registration.owner, &registration.schema) {
            Ok(()) => {}
            Err(PersistenceError::IncompatibleSchema {
                stored, requested, ..
            }) if stored < requested => {
                backend
                    .migrate_schema(
                        &registration.owner,
                        &registration.schema,
                        &registration.migrations,
                    )
                    .map_err(|error| schema_error(registration, error))?;
                backend
                    .register_schema(&registration.owner, &registration.schema)
                    .map_err(|error| schema_error(registration, error))?;
            }
            Err(error) => return Err(schema_error(registration, error)),
        }
    }
    Ok(())
}

fn schema_error(
    registration: &DurableSchemaRegistration,
    error: PersistenceError,
) -> PersistenceCandidateError {
    PersistenceCandidateError::Schema {
        plugin: registration.owner.clone(),
        error,
    }
}

#[cfg(test)]
mod tests;
