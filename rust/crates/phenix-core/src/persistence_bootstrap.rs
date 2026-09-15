use crate::{BackendFeature, DurableSchema, PluginId, SchemaMigration};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt::{self, Display, Formatter},
};

/// One plugin-owned durable schema prepared by Core before plugin startup.
///
/// The owner is explicit so persistence bootstrap never derives authority from
/// product-domain calls. Migrations are ordered by schema version by the
/// persistence backend when the target store is prepared.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableSchemaRegistration {
    pub owner: PluginId,
    pub schema: DurableSchema,
    pub migrations: Vec<SchemaMigration>,
}

impl DurableSchemaRegistration {
    #[must_use]
    pub fn new(owner: PluginId, schema: DurableSchema) -> Self {
        Self {
            owner,
            schema,
            migrations: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_migrations(mut self, migrations: Vec<SchemaMigration>) -> Self {
        self.migrations = migrations;
        self
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct StoreBindingId(String);

impl StoreBindingId {
    pub fn parse(value: impl Into<String>) -> Result<Self, StoreBindingIdParseError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(StoreBindingIdParseError);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoreBindingIdParseError;

impl Display for StoreBindingIdParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("store binding identity must not be empty")
    }
}

impl Error for StoreBindingIdParseError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreBinding {
    pub id: StoreBindingId,
    pub storage_format: String,
}

impl StoreBinding {
    pub fn new(
        id: StoreBindingId,
        storage_format: impl Into<String>,
    ) -> Result<Self, PersistenceBootstrapError> {
        let storage_format = storage_format.into();
        if storage_format.trim().is_empty() {
            return Err(PersistenceBootstrapError::EmptyStorageFormat);
        }
        Ok(Self { id, storage_format })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PersistenceBootstrapDependency {
    Plugin(PluginId),
    TargetStore,
}

/// Static capabilities required to select a Persistence Provider before opening
/// the target Store. Provider-native connection or database handles never enter
/// this descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistenceProviderDescriptor {
    pub plugin: PluginId,
    pub supported_features: BTreeSet<BackendFeature>,
    pub compatible_storage_formats: BTreeSet<String>,
    pub bootstrap_dependencies: Vec<PersistenceBootstrapDependency>,
}

impl PersistenceProviderDescriptor {
    #[must_use]
    pub fn new(
        plugin: PluginId,
        supported_features: impl IntoIterator<Item = BackendFeature>,
        storage_formats: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            plugin,
            supported_features: supported_features.into_iter().collect(),
            compatible_storage_formats: storage_formats.into_iter().collect(),
            bootstrap_dependencies: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_dependencies(
        mut self,
        dependencies: impl IntoIterator<Item = PersistenceBootstrapDependency>,
    ) -> Self {
        self.bootstrap_dependencies = dependencies.into_iter().collect();
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PersistenceProviderTransition {
    CompatibleFormat,
    Migration { operation: String },
    ExportImport { operation: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedPersistenceBootstrap {
    pub provider: PersistenceProviderDescriptor,
    pub binding: StoreBinding,
    pub required_features: BTreeSet<BackendFeature>,
    pub transition: Option<PersistenceProviderTransition>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PersistenceBootstrapError {
    EmptyStorageFormat,
    DuplicateProvider(PluginId),
    ProviderUnavailable(PluginId),
    BootstrapDependencyUnavailable {
        provider: PluginId,
        dependency: PluginId,
    },
    BootstrapCycle(Vec<PluginId>),
    TargetStoreBootstrapCycle(PluginId),
    UnsupportedFeatures {
        provider: PluginId,
        missing: BTreeSet<BackendFeature>,
    },
    StorageFormatUnsupported {
        provider: PluginId,
        storage_format: String,
    },
    ProviderChangeRequiresTransition {
        binding: StoreBindingId,
        active_provider: PluginId,
        candidate_provider: PluginId,
    },
    StorageFormatChangeRequiresTransition {
        binding: StoreBindingId,
        active_format: String,
        candidate_format: String,
    },
    CompatibleFormatMismatch {
        active_format: String,
        candidate_format: String,
    },
    EmptyTransitionOperation,
}

impl Display for PersistenceBootstrapError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyStorageFormat => f.write_str("persistence storage format must not be empty"),
            Self::DuplicateProvider(provider) => {
                write!(f, "duplicate Persistence Provider descriptor: {provider}")
            }
            Self::ProviderUnavailable(provider) => {
                write!(f, "selected Persistence Provider is unavailable: {provider}")
            }
            Self::BootstrapDependencyUnavailable {
                provider,
                dependency,
            } => write!(
                f,
                "Persistence Provider {provider} requires unavailable pre-Store Plugin {dependency}"
            ),
            Self::BootstrapCycle(path) => write!(
                f,
                "Persistence Provider bootstrap dependency cycle: {}",
                path.iter()
                    .map(PluginId::as_str)
                    .collect::<Vec<_>>()
                    .join(" -> ")
            ),
            Self::TargetStoreBootstrapCycle(provider) => write!(
                f,
                "Persistence Provider {provider} depends on the target Store before it can open that Store"
            ),
            Self::UnsupportedFeatures { provider, missing } => write!(
                f,
                "Persistence Provider {provider} is missing required features: {missing:?}"
            ),
            Self::StorageFormatUnsupported {
                provider,
                storage_format,
            } => write!(
                f,
                "Persistence Provider {provider} cannot open storage format {storage_format}"
            ),
            Self::ProviderChangeRequiresTransition {
                binding,
                active_provider,
                candidate_provider,
            } => write!(
                f,
                "Store Binding {} cannot change Persistence Provider from {active_provider} to {candidate_provider} without an explicit transition",
                binding.as_str()
            ),
            Self::StorageFormatChangeRequiresTransition {
                binding,
                active_format,
                candidate_format,
            } => write!(
                f,
                "Store Binding {} cannot change storage format from {active_format} to {candidate_format} without an explicit transition",
                binding.as_str()
            ),
            Self::CompatibleFormatMismatch {
                active_format,
                candidate_format,
            } => write!(
                f,
                "compatible-format transition requires one shared format, got {active_format} and {candidate_format}"
            ),
            Self::EmptyTransitionOperation => {
                f.write_str("persistence transition operation identity must not be empty")
            }
        }
    }
}

impl Error for PersistenceBootstrapError {}

pub fn resolve_persistence_bootstrap(
    selected_provider: &PluginId,
    providers: impl IntoIterator<Item = PersistenceProviderDescriptor>,
    pre_store_plugins: &BTreeSet<PluginId>,
    binding: StoreBinding,
    schemas: &[DurableSchemaRegistration],
    active: Option<&ResolvedPersistenceBootstrap>,
    transition: Option<PersistenceProviderTransition>,
) -> Result<ResolvedPersistenceBootstrap, PersistenceBootstrapError> {
    let mut providers_by_id = BTreeMap::new();
    for provider in providers {
        let id = provider.plugin.clone();
        if providers_by_id.insert(id.clone(), provider).is_some() {
            return Err(PersistenceBootstrapError::DuplicateProvider(id));
        }
    }
    let provider = providers_by_id
        .get(selected_provider)
        .cloned()
        .ok_or_else(|| PersistenceBootstrapError::ProviderUnavailable(selected_provider.clone()))?;

    validate_bootstrap_dependencies(&provider.plugin, &providers_by_id, pre_store_plugins)?;

    let required_features = schemas
        .iter()
        .flat_map(|registration| registration.schema.required_features.iter().copied())
        .chain(
            schemas
                .iter()
                .filter(|registration| !registration.migrations.is_empty())
                .map(|_| BackendFeature::Migrations),
        )
        .collect::<BTreeSet<_>>();
    let missing = required_features
        .difference(&provider.supported_features)
        .copied()
        .collect::<BTreeSet<_>>();
    if !missing.is_empty() {
        return Err(PersistenceBootstrapError::UnsupportedFeatures {
            provider: provider.plugin.clone(),
            missing,
        });
    }
    if !provider
        .compatible_storage_formats
        .contains(&binding.storage_format)
    {
        return Err(PersistenceBootstrapError::StorageFormatUnsupported {
            provider: provider.plugin.clone(),
            storage_format: binding.storage_format.clone(),
        });
    }

    validate_transition(active, &provider, &binding, transition.as_ref())?;
    validate_transition_operation(transition.as_ref())?;

    Ok(ResolvedPersistenceBootstrap {
        provider,
        binding,
        required_features,
        transition,
    })
}

fn validate_bootstrap_dependencies(
    selected: &PluginId,
    providers: &BTreeMap<PluginId, PersistenceProviderDescriptor>,
    pre_store_plugins: &BTreeSet<PluginId>,
) -> Result<(), PersistenceBootstrapError> {
    fn visit(
        plugin: &PluginId,
        providers: &BTreeMap<PluginId, PersistenceProviderDescriptor>,
        pre_store_plugins: &BTreeSet<PluginId>,
        visiting: &mut Vec<PluginId>,
        complete: &mut BTreeSet<PluginId>,
    ) -> Result<(), PersistenceBootstrapError> {
        if complete.contains(plugin) {
            return Ok(());
        }
        if let Some(position) = visiting.iter().position(|candidate| candidate == plugin) {
            let mut path = visiting[position..].to_vec();
            path.push(plugin.clone());
            return Err(PersistenceBootstrapError::BootstrapCycle(path));
        }
        let Some(provider) = providers.get(plugin) else {
            return Ok(());
        };
        visiting.push(plugin.clone());
        for dependency in &provider.bootstrap_dependencies {
            match dependency {
                PersistenceBootstrapDependency::TargetStore => {
                    return Err(PersistenceBootstrapError::TargetStoreBootstrapCycle(
                        plugin.clone(),
                    ));
                }
                PersistenceBootstrapDependency::Plugin(dependency) => {
                    if !pre_store_plugins.contains(dependency)
                        && !providers.contains_key(dependency)
                    {
                        return Err(PersistenceBootstrapError::BootstrapDependencyUnavailable {
                            provider: plugin.clone(),
                            dependency: dependency.clone(),
                        });
                    }
                    visit(dependency, providers, pre_store_plugins, visiting, complete)?;
                }
            }
        }
        visiting.pop();
        complete.insert(plugin.clone());
        Ok(())
    }

    visit(
        selected,
        providers,
        pre_store_plugins,
        &mut Vec::new(),
        &mut BTreeSet::new(),
    )
}

fn validate_transition(
    active: Option<&ResolvedPersistenceBootstrap>,
    candidate_provider: &PersistenceProviderDescriptor,
    candidate_binding: &StoreBinding,
    transition: Option<&PersistenceProviderTransition>,
) -> Result<(), PersistenceBootstrapError> {
    let Some(active) = active else {
        return Ok(());
    };
    if active.binding.id != candidate_binding.id {
        return Ok(());
    }

    let provider_changed = active.provider.plugin != candidate_provider.plugin;
    let format_changed = active.binding.storage_format != candidate_binding.storage_format;
    if provider_changed && transition.is_none() {
        return Err(
            PersistenceBootstrapError::ProviderChangeRequiresTransition {
                binding: candidate_binding.id.clone(),
                active_provider: active.provider.plugin.clone(),
                candidate_provider: candidate_provider.plugin.clone(),
            },
        );
    }
    if format_changed && transition.is_none() {
        return Err(
            PersistenceBootstrapError::StorageFormatChangeRequiresTransition {
                binding: candidate_binding.id.clone(),
                active_format: active.binding.storage_format.clone(),
                candidate_format: candidate_binding.storage_format.clone(),
            },
        );
    }
    if matches!(
        transition,
        Some(PersistenceProviderTransition::CompatibleFormat)
    ) && active.binding.storage_format != candidate_binding.storage_format
    {
        return Err(PersistenceBootstrapError::CompatibleFormatMismatch {
            active_format: active.binding.storage_format.clone(),
            candidate_format: candidate_binding.storage_format.clone(),
        });
    }
    Ok(())
}

fn validate_transition_operation(
    transition: Option<&PersistenceProviderTransition>,
) -> Result<(), PersistenceBootstrapError> {
    let operation = match transition {
        Some(PersistenceProviderTransition::Migration { operation })
        | Some(PersistenceProviderTransition::ExportImport { operation }) => Some(operation),
        Some(PersistenceProviderTransition::CompatibleFormat) | None => None,
    };
    if operation.is_some_and(|operation| operation.trim().is_empty()) {
        return Err(PersistenceBootstrapError::EmptyTransitionOperation);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ResourceNamespace;

    fn plugin(value: &str) -> PluginId {
        PluginId::parse(value).unwrap()
    }

    fn binding(id: &str, format: &str) -> StoreBinding {
        StoreBinding::new(StoreBindingId::parse(id).unwrap(), format).unwrap()
    }

    fn provider(
        id: &str,
        features: &[BackendFeature],
        formats: &[&str],
    ) -> PersistenceProviderDescriptor {
        PersistenceProviderDescriptor::new(
            plugin(id),
            features.iter().copied(),
            formats.iter().map(|format| (*format).to_owned()),
        )
    }

    fn schema(features: &[BackendFeature]) -> DurableSchemaRegistration {
        DurableSchemaRegistration::new(
            plugin("fixture.owner"),
            DurableSchema::requiring(
                ResourceNamespace::parse("fixture.owner.state").unwrap(),
                1,
                features.iter().copied(),
            ),
        )
    }

    #[test]
    fn unsupported_features_reject_provider_before_store_plan_resolves() {
        let error = resolve_persistence_bootstrap(
            &plugin("fixture.provider"),
            [provider(
                "fixture.provider",
                &[BackendFeature::Transactions],
                &["fixture-v1"],
            )],
            &BTreeSet::new(),
            binding("primary", "fixture-v1"),
            &[schema(&[BackendFeature::IndexedRange])],
            None,
            None,
        )
        .unwrap_err();

        assert_eq!(
            error,
            PersistenceBootstrapError::UnsupportedFeatures {
                provider: plugin("fixture.provider"),
                missing: BTreeSet::from([BackendFeature::IndexedRange]),
            }
        );
    }

    #[test]
    fn target_store_dependency_is_rejected_as_bootstrap_cycle() {
        let descriptor = provider("fixture.provider", &[], &["fixture-v1"])
            .with_dependencies([PersistenceBootstrapDependency::TargetStore]);
        let error = resolve_persistence_bootstrap(
            &plugin("fixture.provider"),
            [descriptor],
            &BTreeSet::new(),
            binding("primary", "fixture-v1"),
            &[],
            None,
            None,
        )
        .unwrap_err();

        assert_eq!(
            error,
            PersistenceBootstrapError::TargetStoreBootstrapCycle(plugin("fixture.provider"))
        );
    }

    #[test]
    fn provider_dependency_cycle_is_rejected_before_store_open() {
        let first = provider("fixture.first", &[], &["fixture-v1"]).with_dependencies([
            PersistenceBootstrapDependency::Plugin(plugin("fixture.second")),
        ]);
        let second = provider("fixture.second", &[], &["fixture-v1"]).with_dependencies([
            PersistenceBootstrapDependency::Plugin(plugin("fixture.first")),
        ]);
        let error = resolve_persistence_bootstrap(
            &plugin("fixture.first"),
            [first, second],
            &BTreeSet::new(),
            binding("primary", "fixture-v1"),
            &[],
            None,
            None,
        )
        .unwrap_err();

        assert_eq!(
            error,
            PersistenceBootstrapError::BootstrapCycle(vec![
                plugin("fixture.first"),
                plugin("fixture.second"),
                plugin("fixture.first"),
            ])
        );
    }

    #[test]
    fn provider_change_on_existing_binding_requires_explicit_transition() {
        let active_provider = provider("fixture.first", &[], &["shared-v1"]);
        let active = resolve_persistence_bootstrap(
            &active_provider.plugin,
            [active_provider.clone()],
            &BTreeSet::new(),
            binding("primary", "shared-v1"),
            &[],
            None,
            None,
        )
        .unwrap();
        let candidate = provider("fixture.second", &[], &["shared-v1"]);

        assert!(matches!(
            resolve_persistence_bootstrap(
                &candidate.plugin,
                [candidate.clone()],
                &BTreeSet::new(),
                binding("primary", "shared-v1"),
                &[],
                Some(&active),
                None,
            ),
            Err(PersistenceBootstrapError::ProviderChangeRequiresTransition { .. })
        ));

        let resolved = resolve_persistence_bootstrap(
            &candidate.plugin,
            [candidate.clone()],
            &BTreeSet::new(),
            binding("primary", "shared-v1"),
            &[],
            Some(&active),
            Some(PersistenceProviderTransition::CompatibleFormat),
        )
        .unwrap();
        assert!(matches!(
            resolved.transition,
            Some(PersistenceProviderTransition::CompatibleFormat)
        ));
    }

    #[test]
    fn new_store_binding_allows_provider_change_without_reusing_active_store() {
        let active_provider = provider("fixture.first", &[], &["first-v1"]);
        let active = resolve_persistence_bootstrap(
            &active_provider.plugin,
            [active_provider.clone()],
            &BTreeSet::new(),
            binding("primary", "first-v1"),
            &[],
            None,
            None,
        )
        .unwrap();
        let candidate = provider("fixture.second", &[], &["second-v1"]);
        let resolved = resolve_persistence_bootstrap(
            &candidate.plugin,
            [candidate.clone()],
            &BTreeSet::new(),
            binding("replacement", "second-v1"),
            &[],
            Some(&active),
            None,
        )
        .unwrap();

        assert_eq!(resolved.provider, candidate);
        assert_eq!(resolved.binding.id.as_str(), "replacement");
    }
}
