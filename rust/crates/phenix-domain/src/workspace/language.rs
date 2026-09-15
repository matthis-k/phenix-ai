use crate::WorkspaceId;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{btree_map::Entry, BTreeMap};
use std::fmt::{self, Display, Formatter};
use std::path::PathBuf;

domain_id_type!(LanguageServiceKind);
domain_id_type!(LanguageProviderId);

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct LanguageProviderCapabilities {
    #[serde(default)]
    pub requests: bool,
    #[serde(default)]
    pub notifications: bool,
    #[serde(default)]
    pub shared_diagnostics: bool,
    #[serde(default)]
    pub background_documents: bool,
    #[serde(default)]
    pub dirty_buffers: bool,
}

impl LanguageProviderCapabilities {
    #[must_use]
    pub fn satisfies(&self, required: &Self) -> bool {
        (!required.requests || self.requests)
            && (!required.notifications || self.notifications)
            && (!required.shared_diagnostics || self.shared_diagnostics)
            && (!required.background_documents || self.background_documents)
            && (!required.dirty_buffers || self.dirty_buffers)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedLanguageProviderDefinition {
    pub service: LanguageServiceKind,
    pub provider: LanguageProviderId,
    pub command: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    pub capabilities: LanguageProviderCapabilities,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageServiceRequirement {
    pub service: LanguageServiceKind,
    #[serde(default)]
    pub required_capabilities: LanguageProviderCapabilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_provider: Option<LanguageProviderId>,
}

#[derive(Clone, Debug, Default)]
pub struct LanguageServiceConfiguration {
    managed: BTreeMap<(LanguageServiceKind, LanguageProviderId), ManagedLanguageProviderDefinition>,
    requirements: BTreeMap<LanguageServiceKind, LanguageServiceRequirement>,
}

impl LanguageServiceConfiguration {
    pub fn register_managed(
        &mut self,
        definition: ManagedLanguageProviderDefinition,
    ) -> Result<(), LanguageServiceError> {
        let key = (definition.service.clone(), definition.provider.clone());
        match self.managed.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(definition);
                Ok(())
            }
            Entry::Occupied(_) => Err(LanguageServiceError::DuplicateManagedProvider),
        }
    }

    pub fn set_requirement(&mut self, requirement: LanguageServiceRequirement) {
        self.requirements
            .insert(requirement.service.clone(), requirement);
    }

    #[must_use]
    pub fn managed_for(
        &self,
        service: &LanguageServiceKind,
    ) -> Vec<ManagedLanguageProviderDefinition> {
        self.managed
            .iter()
            .filter(|((kind, _), _)| kind == service)
            .map(|(_, definition)| definition.clone())
            .collect()
    }

    #[must_use]
    pub fn requirement_for(&self, service: &LanguageServiceKind) -> LanguageServiceRequirement {
        self.requirements
            .get(service)
            .cloned()
            .unwrap_or_else(|| LanguageServiceRequirement {
                service: service.clone(),
                required_capabilities: LanguageProviderCapabilities::default(),
                preferred_provider: None,
            })
    }

    #[must_use]
    pub fn semantic_manifest(&self) -> Value {
        json!({
            "managed": self.managed.values().collect::<Vec<_>>(),
            "requirements": self.requirements,
        })
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum LanguageProviderSource {
    Frontend { connection: u64 },
    Managed { generation: u64 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanguageProviderCandidate {
    pub service: LanguageServiceKind,
    pub provider: LanguageProviderId,
    pub capabilities: LanguageProviderCapabilities,
    pub source: LanguageProviderSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveLanguageProvider {
    pub workspace: WorkspaceId,
    pub service: LanguageServiceKind,
    pub provider: LanguageProviderId,
    pub capabilities: LanguageProviderCapabilities,
    pub source: LanguageProviderSource,
    pub epoch: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanguageProviderLease {
    pub workspace: WorkspaceId,
    pub service: LanguageServiceKind,
    pub provider: LanguageProviderId,
    pub epoch: u64,
}

#[derive(Clone, Debug, Default)]
pub struct LanguageServiceManager {
    active: BTreeMap<(WorkspaceId, LanguageServiceKind), ActiveLanguageProvider>,
    next_epoch: BTreeMap<(WorkspaceId, LanguageServiceKind), u64>,
}

impl LanguageServiceManager {
    pub fn reconcile(
        &mut self,
        workspace: &WorkspaceId,
        service: &LanguageServiceKind,
        configuration: &LanguageServiceConfiguration,
        frontend: impl IntoIterator<Item = LanguageProviderCandidate>,
        live_managed: &BTreeMap<LanguageProviderId, u64>,
    ) -> Option<ActiveLanguageProvider> {
        let requirement = configuration.requirement_for(service);
        let mut candidates = frontend
            .into_iter()
            .filter(|candidate| candidate.service == *service)
            .filter(|candidate| {
                candidate
                    .capabilities
                    .satisfies(&requirement.required_capabilities)
            })
            .collect::<Vec<_>>();
        candidates.extend(configuration.managed_for(service).into_iter().filter_map(
            |definition| {
                let generation = *live_managed.get(&definition.provider)?;
                definition
                    .capabilities
                    .satisfies(&requirement.required_capabilities)
                    .then_some(LanguageProviderCandidate {
                        service: definition.service,
                        provider: definition.provider,
                        capabilities: definition.capabilities,
                        source: LanguageProviderSource::Managed { generation },
                    })
            },
        ));
        candidates.sort_by(|left, right| {
            let left_preferred = requirement
                .preferred_provider
                .as_ref()
                .is_some_and(|provider| provider == &left.provider);
            let right_preferred = requirement
                .preferred_provider
                .as_ref()
                .is_some_and(|provider| provider == &right.provider);
            right_preferred
                .cmp(&left_preferred)
                .then_with(|| source_rank(&left.source).cmp(&source_rank(&right.source)))
                .then_with(|| left.provider.cmp(&right.provider))
                .then_with(|| left.source.cmp(&right.source))
        });

        let key = (workspace.clone(), service.clone());
        let selected = if let Some(current) = self.active.get(&key) {
            candidates
                .iter()
                .find(|candidate| same_provider(current, candidate))
                .cloned()
                .or_else(|| candidates.first().cloned())
        } else {
            candidates.first().cloned()
        };
        match selected {
            None => {
                self.active.remove(&key);
                None
            }
            Some(candidate) => {
                if self
                    .active
                    .get(&key)
                    .is_some_and(|active| same_provider(active, &candidate))
                {
                    return self.active.get(&key).cloned();
                }
                let epoch = self
                    .next_epoch
                    .entry(key.clone())
                    .and_modify(|epoch| *epoch = epoch.saturating_add(1))
                    .or_insert(1);
                let active = ActiveLanguageProvider {
                    workspace: workspace.clone(),
                    service: service.clone(),
                    provider: candidate.provider,
                    capabilities: candidate.capabilities,
                    source: candidate.source,
                    epoch: *epoch,
                };
                self.active.insert(key, active.clone());
                Some(active)
            }
        }
    }

    #[must_use]
    pub fn active(
        &self,
        workspace: &WorkspaceId,
        service: &LanguageServiceKind,
    ) -> Option<&ActiveLanguageProvider> {
        self.active.get(&(workspace.clone(), service.clone()))
    }

    pub fn lease(
        &self,
        workspace: &WorkspaceId,
        service: &LanguageServiceKind,
    ) -> Result<LanguageProviderLease, LanguageServiceError> {
        let active = self
            .active(workspace, service)
            .ok_or(LanguageServiceError::Unavailable)?;
        Ok(LanguageProviderLease {
            workspace: workspace.clone(),
            service: service.clone(),
            provider: active.provider.clone(),
            epoch: active.epoch,
        })
    }

    pub fn validate_lease(
        &self,
        lease: &LanguageProviderLease,
    ) -> Result<(), LanguageServiceError> {
        let active = self
            .active(&lease.workspace, &lease.service)
            .ok_or(LanguageServiceError::ProviderChanged)?;
        if active.provider == lease.provider && active.epoch == lease.epoch {
            Ok(())
        } else {
            Err(LanguageServiceError::ProviderChanged)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LanguageServiceError {
    DuplicateManagedProvider,
    Unavailable,
    ProviderChanged,
}

impl Display for LanguageServiceError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateManagedProvider => {
                f.write_str("managed language provider is already registered")
            }
            Self::Unavailable => f.write_str("language service is unavailable"),
            Self::ProviderChanged => f.write_str("language provider changed during the request"),
        }
    }
}

impl std::error::Error for LanguageServiceError {}

fn source_rank(source: &LanguageProviderSource) -> u8 {
    match source {
        LanguageProviderSource::Frontend { .. } => 0,
        LanguageProviderSource::Managed { .. } => 1,
    }
}

fn same_provider(active: &ActiveLanguageProvider, candidate: &LanguageProviderCandidate) -> bool {
    active.provider == candidate.provider
        && active.source == candidate.source
        && active.capabilities == candidate.capabilities
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind() -> LanguageServiceKind {
        LanguageServiceKind::parse("rust").unwrap()
    }

    fn provider(name: &str) -> LanguageProviderId {
        LanguageProviderId::parse(name).unwrap()
    }

    fn capable() -> LanguageProviderCapabilities {
        LanguageProviderCapabilities {
            requests: true,
            notifications: true,
            shared_diagnostics: true,
            background_documents: true,
            dirty_buffers: false,
        }
    }

    fn frontend(connection: u64, name: &str) -> LanguageProviderCandidate {
        LanguageProviderCandidate {
            service: kind(),
            provider: provider(name),
            capabilities: capable(),
            source: LanguageProviderSource::Frontend { connection },
        }
    }

    fn managed(command: &str) -> ManagedLanguageProviderDefinition {
        ManagedLanguageProviderDefinition {
            service: kind(),
            provider: provider("managed"),
            command: PathBuf::from(command),
            args: Vec::new(),
            capabilities: capable(),
        }
    }

    #[test]
    fn language_ids_reject_blank_wire_values() {
        assert!(serde_json::from_str::<LanguageServiceKind>("\"\"").is_err());
        assert!(serde_json::from_str::<LanguageProviderId>("\"  \"").is_err());
    }

    #[test]
    fn duplicate_managed_provider_does_not_replace_existing_definition() {
        let mut configuration = LanguageServiceConfiguration::default();
        configuration.register_managed(managed("first")).unwrap();
        assert_eq!(
            configuration.register_managed(managed("second")),
            Err(LanguageServiceError::DuplicateManagedProvider)
        );
        assert_eq!(
            configuration.managed_for(&kind())[0].command,
            PathBuf::from("first")
        );
    }

    #[test]
    fn capability_satisfaction_requires_every_requested_behavior() {
        let available = capable();
        assert!(available.satisfies(&LanguageProviderCapabilities {
            requests: true,
            notifications: true,
            ..LanguageProviderCapabilities::default()
        }));
        assert!(!available.satisfies(&LanguageProviderCapabilities {
            dirty_buffers: true,
            ..LanguageProviderCapabilities::default()
        }));
    }

    #[test]
    fn selection_keeps_current_eligible_provider() {
        let workspace = WorkspaceId::parse("workspace:test").unwrap();
        let mut manager = LanguageServiceManager::default();
        let configuration = LanguageServiceConfiguration::default();
        let first = manager
            .reconcile(
                &workspace,
                &kind(),
                &configuration,
                [frontend(2, "b")],
                &BTreeMap::new(),
            )
            .unwrap();
        let second = manager
            .reconcile(
                &workspace,
                &kind(),
                &configuration,
                [frontend(1, "a"), frontend(2, "b")],
                &BTreeMap::new(),
            )
            .unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn configured_preference_wins_and_frontend_precedes_managed_by_default() {
        let workspace = WorkspaceId::parse("workspace:test").unwrap();
        let mut configuration = LanguageServiceConfiguration::default();
        configuration
            .register_managed(managed("rust-analyzer"))
            .unwrap();
        configuration.set_requirement(LanguageServiceRequirement {
            service: kind(),
            required_capabilities: LanguageProviderCapabilities::default(),
            preferred_provider: Some(provider("managed")),
        });
        let mut manager = LanguageServiceManager::default();
        let active = manager
            .reconcile(
                &workspace,
                &kind(),
                &configuration,
                [frontend(1, "frontend")],
                &BTreeMap::from([(provider("managed"), 1)]),
            )
            .unwrap();
        assert_eq!(active.provider, provider("managed"));

        configuration.set_requirement(LanguageServiceRequirement {
            service: kind(),
            required_capabilities: LanguageProviderCapabilities::default(),
            preferred_provider: None,
        });
        let mut manager = LanguageServiceManager::default();
        let active = manager
            .reconcile(
                &workspace,
                &kind(),
                &configuration,
                [frontend(1, "frontend")],
                &BTreeMap::from([(provider("managed"), 1)]),
            )
            .unwrap();
        assert_eq!(active.provider, provider("frontend"));
    }

    #[test]
    fn insufficient_provider_is_not_composed_with_another_provider() {
        let workspace = WorkspaceId::parse("workspace:test").unwrap();
        let mut configuration = LanguageServiceConfiguration::default();
        configuration.set_requirement(LanguageServiceRequirement {
            service: kind(),
            required_capabilities: LanguageProviderCapabilities {
                dirty_buffers: true,
                ..capable()
            },
            preferred_provider: None,
        });
        let mut manager = LanguageServiceManager::default();
        assert!(manager
            .reconcile(
                &workspace,
                &kind(),
                &configuration,
                [frontend(1, "frontend")],
                &BTreeMap::new(),
            )
            .is_none());
    }

    #[test]
    fn provider_change_invalidates_old_epoch() {
        let workspace = WorkspaceId::parse("workspace:test").unwrap();
        let mut manager = LanguageServiceManager::default();
        let configuration = LanguageServiceConfiguration::default();
        manager.reconcile(
            &workspace,
            &kind(),
            &configuration,
            [frontend(1, "a")],
            &BTreeMap::new(),
        );
        let lease = manager.lease(&workspace, &kind()).unwrap();
        manager.reconcile(
            &workspace,
            &kind(),
            &configuration,
            [frontend(2, "b")],
            &BTreeMap::new(),
        );
        assert_eq!(
            manager.validate_lease(&lease),
            Err(LanguageServiceError::ProviderChanged)
        );
        assert_eq!(manager.active(&workspace, &kind()).unwrap().epoch, 2);
    }

    #[test]
    fn managed_restart_changes_epoch_even_when_provider_id_is_stable() {
        let workspace = WorkspaceId::parse("workspace:test").unwrap();
        let mut configuration = LanguageServiceConfiguration::default();
        configuration
            .register_managed(managed("rust-analyzer"))
            .unwrap();
        let mut manager = LanguageServiceManager::default();
        manager.reconcile(
            &workspace,
            &kind(),
            &configuration,
            [],
            &BTreeMap::from([(provider("managed"), 1)]),
        );
        let lease = manager.lease(&workspace, &kind()).unwrap();
        manager.reconcile(
            &workspace,
            &kind(),
            &configuration,
            [],
            &BTreeMap::from([(provider("managed"), 2)]),
        );
        assert_eq!(manager.active(&workspace, &kind()).unwrap().epoch, 2);
        assert_eq!(
            manager.validate_lease(&lease),
            Err(LanguageServiceError::ProviderChanged)
        );
    }

    #[test]
    fn managed_definition_and_requirement_change_semantic_manifest() {
        let mut configuration = LanguageServiceConfiguration::default();
        let before = configuration.semantic_manifest();
        let mut definition = managed("rust-analyzer");
        definition.args = vec!["--stdio".to_owned()];
        configuration.register_managed(definition).unwrap();
        configuration.set_requirement(LanguageServiceRequirement {
            service: kind(),
            required_capabilities: capable(),
            preferred_provider: Some(provider("managed")),
        });
        assert_ne!(before, configuration.semantic_manifest());
    }
}
