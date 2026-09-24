use crate::{Authority, ConfigurationFrontendId, PhenixValue};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct ConfigNamespace(String);

impl ConfigNamespace {
    pub fn parse(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        if value.is_empty() {
            return Err("configuration namespace must not be empty");
        }
        if !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':' | b'@')
        }) {
            return Err("configuration namespace contains unsupported characters");
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ConfigNamespace {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl TryFrom<String> for ConfigNamespace {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConfigContributionSource {
    pub frontend: ConfigurationFrontendId,
    pub source_identity: String,
    pub source_revision: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConfigContribution {
    pub source: ConfigContributionSource,
    pub namespace: ConfigNamespace,
    pub contract_version: u64,
    pub precedence: i32,
    pub value: PhenixValue,
    pub requested_authority: Authority,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigSourceClass {
    Materialized,
    EnvironmentBinding,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConfigurationFrontendMetadata {
    pub id: ConfigurationFrontendId,
    pub version: u64,
    pub accepted_source_kinds: BTreeSet<String>,
    pub exposed_namespaces: BTreeSet<ConfigNamespace>,
    pub watch: bool,
    pub required_authority: Authority,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrontendConfigContribution {
    pub source_kind: String,
    pub source_identity: String,
    pub source_revision: String,
    pub source_class: ConfigSourceClass,
    pub namespace: ConfigNamespace,
    pub contract_version: u64,
    pub precedence: i32,
    pub value: PhenixValue,
    pub requested_authority: Authority,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrontendConfigError {
    InvalidMetadataVersion,
    InvalidContractVersion,
    UnsupportedSourceKind(String),
    NamespaceNotExposed(ConfigNamespace),
    MissingSourceIdentity,
    MissingSourceRevision,
    SourceAuthorityDenied,
    EnvironmentBindingChangesSemantics,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigMergeError {
    InvalidContractVersion {
        namespace: ConfigNamespace,
    },
    MissingSourceIdentity {
        namespace: ConfigNamespace,
    },
    MissingSourceRevision {
        namespace: ConfigNamespace,
    },
    ConflictingContributions {
        namespace: ConfigNamespace,
        contract_version: u64,
        precedence: i32,
    },
}

impl FrontendConfigContribution {
    pub fn lower(
        self,
        metadata: &ConfigurationFrontendMetadata,
        authority_ceiling: &Authority,
    ) -> Result<ConfigContribution, FrontendConfigError> {
        if metadata.version == 0 {
            return Err(FrontendConfigError::InvalidMetadataVersion);
        }
        if self.contract_version == 0 {
            return Err(FrontendConfigError::InvalidContractVersion);
        }
        if !metadata.accepted_source_kinds.contains(&self.source_kind) {
            return Err(FrontendConfigError::UnsupportedSourceKind(self.source_kind));
        }
        if !metadata.exposed_namespaces.contains(&self.namespace) {
            return Err(FrontendConfigError::NamespaceNotExposed(self.namespace));
        }
        if self.source_identity.trim().is_empty() {
            return Err(FrontendConfigError::MissingSourceIdentity);
        }
        if self.source_revision.trim().is_empty() {
            return Err(FrontendConfigError::MissingSourceRevision);
        }
        if !authority_ceiling.permits_all(&metadata.required_authority) {
            return Err(FrontendConfigError::SourceAuthorityDenied);
        }
        if matches!(self.source_class, ConfigSourceClass::EnvironmentBinding) {
            return Err(FrontendConfigError::EnvironmentBindingChangesSemantics);
        }

        Ok(ConfigContribution {
            source: ConfigContributionSource {
                frontend: metadata.id.clone(),
                source_identity: self.source_identity,
                source_revision: self.source_revision,
            },
            namespace: self.namespace,
            contract_version: self.contract_version,
            precedence: self.precedence,
            value: self.value,
            requested_authority: self.requested_authority,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConfigContributionAttribution {
    pub source: ConfigContributionSource,
    pub requested_authority: Authority,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResolvedConfigContribution {
    pub namespace: ConfigNamespace,
    pub contract_version: u64,
    pub value: PhenixValue,
    pub attributions: Vec<ConfigContributionAttribution>,
    pub granted_authority: Authority,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ResolvedConfigContributions {
    entries: Vec<ResolvedConfigContribution>,
}

impl ResolvedConfigContributions {
    pub fn try_resolve(
        contributions: impl IntoIterator<Item = ConfigContribution>,
        authority_ceiling: &Authority,
    ) -> Result<Self, ConfigMergeError> {
        let mut contributions: Vec<_> = contributions.into_iter().collect();
        for contribution in &contributions {
            if contribution.contract_version == 0 {
                return Err(ConfigMergeError::InvalidContractVersion {
                    namespace: contribution.namespace.clone(),
                });
            }
            if contribution.source.source_identity.trim().is_empty() {
                return Err(ConfigMergeError::MissingSourceIdentity {
                    namespace: contribution.namespace.clone(),
                });
            }
            if contribution.source.source_revision.trim().is_empty() {
                return Err(ConfigMergeError::MissingSourceRevision {
                    namespace: contribution.namespace.clone(),
                });
            }
        }
        contributions.sort_by(|left, right| {
            left.namespace
                .cmp(&right.namespace)
                .then_with(|| left.contract_version.cmp(&right.contract_version))
                .then_with(|| right.precedence.cmp(&left.precedence))
                .then_with(|| left.source.frontend.cmp(&right.source.frontend))
                .then_with(|| {
                    left.source
                        .source_identity
                        .cmp(&right.source.source_identity)
                })
                .then_with(|| {
                    left.source
                        .source_revision
                        .cmp(&right.source.source_revision)
                })
                .then_with(|| {
                    left.requested_authority
                        .capabilities()
                        .cmp(right.requested_authority.capabilities())
                })
        });

        let mut resolved =
            BTreeMap::<(ConfigNamespace, u64), (i32, ResolvedConfigContribution)>::new();
        for contribution in contributions {
            let key = (
                contribution.namespace.clone(),
                contribution.contract_version,
            );
            let precedence = contribution.precedence;
            let granted_authority = authority_ceiling.attenuate(&contribution.requested_authority);
            let attribution = ConfigContributionAttribution {
                source: contribution.source,
                requested_authority: contribution.requested_authority,
            };
            let candidate = ResolvedConfigContribution {
                namespace: contribution.namespace,
                contract_version: contribution.contract_version,
                value: contribution.value,
                attributions: vec![attribution.clone()],
                granted_authority,
            };
            match resolved.get_mut(&key) {
                None => {
                    resolved.insert(key, (precedence, candidate));
                }
                Some((current_precedence, existing)) if precedence > *current_precedence => {
                    *current_precedence = precedence;
                    *existing = candidate;
                }
                Some((current_precedence, existing)) if precedence == *current_precedence => {
                    if existing.value != candidate.value
                        || existing.granted_authority != candidate.granted_authority
                    {
                        return Err(ConfigMergeError::ConflictingContributions {
                            namespace: key.0,
                            contract_version: key.1,
                            precedence,
                        });
                    }
                    if !existing.attributions.contains(&attribution) {
                        existing.attributions.push(attribution);
                        existing.attributions.sort_by(|left, right| {
                            left.source
                                .frontend
                                .cmp(&right.source.frontend)
                                .then_with(|| {
                                    left.source
                                        .source_identity
                                        .cmp(&right.source.source_identity)
                                })
                                .then_with(|| {
                                    left.source
                                        .source_revision
                                        .cmp(&right.source.source_revision)
                                })
                                .then_with(|| {
                                    left.requested_authority
                                        .capabilities()
                                        .cmp(right.requested_authority.capabilities())
                                })
                        });
                    }
                }
                Some(_) => {}
            }
        }

        Ok(Self {
            entries: resolved.into_values().map(|(_, entry)| entry).collect(),
        })
    }

    pub fn entries(&self) -> &[ResolvedConfigContribution] {
        &self.entries
    }

    pub fn by_namespace(&self) -> BTreeMap<&ConfigNamespace, Vec<&ResolvedConfigContribution>> {
        let mut grouped: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for entry in &self.entries {
            grouped.entry(&entry.namespace).or_default().push(entry);
        }
        grouped
    }

    pub fn semantic_payload(&self) -> serde_json::Value {
        serde_json::Value::Array(
            self.entries
                .iter()
                .map(|entry| {
                    serde_json::json!({
                        "namespace": &entry.namespace,
                        "contract_version": entry.contract_version,
                        "value": &entry.value,
                        "granted_authority": &entry.granted_authority,
                    })
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CapabilityId;

    fn cap(value: &str) -> CapabilityId {
        CapabilityId::parse(value).unwrap()
    }

    fn resolve(
        contributions: impl IntoIterator<Item = ConfigContribution>,
        authority_ceiling: &Authority,
    ) -> ResolvedConfigContributions {
        ResolvedConfigContributions::try_resolve(contributions, authority_ceiling).unwrap()
    }

    fn contribution(
        frontend: &str,
        source_identity: &str,
        revision: &str,
        precedence: i32,
        requested_authority: Authority,
    ) -> ConfigContribution {
        ConfigContribution {
            source: ConfigContributionSource {
                frontend: ConfigurationFrontendId::parse(frontend).unwrap(),
                source_identity: source_identity.into(),
                source_revision: revision.into(),
            },
            namespace: ConfigNamespace::parse("acme.engineering@1").unwrap(),
            contract_version: 1,
            precedence,
            value: serde_json::json!({"review":"strict","team":"compiler"}).into(),
            requested_authority,
        }
    }

    fn frontend_metadata(required_authority: Authority) -> ConfigurationFrontendMetadata {
        ConfigurationFrontendMetadata {
            id: ConfigurationFrontendId::parse("phenix-config-lua").unwrap(),
            version: 1,
            accepted_source_kinds: BTreeSet::from(["lua".into()]),
            exposed_namespaces: BTreeSet::from([
                ConfigNamespace::parse("acme.engineering@1").unwrap()
            ]),
            watch: true,
            required_authority,
        }
    }

    fn frontend_contribution(source_class: ConfigSourceClass) -> FrontendConfigContribution {
        FrontendConfigContribution {
            source_kind: "lua".into(),
            source_identity: "file:phenix.lua".into(),
            source_revision: "sha256:fixture".into(),
            source_class,
            namespace: ConfigNamespace::parse("acme.engineering@1").unwrap(),
            contract_version: 1,
            precedence: 10,
            value: serde_json::json!({"review":"strict"}).into(),
            requested_authority: Authority::default(),
        }
    }

    #[test]
    fn namespace_deserialization_preserves_parser_invariants() {
        assert!(serde_json::from_str::<ConfigNamespace>("\"\"").is_err());
        assert!(serde_json::from_str::<ConfigNamespace>("\"has space\"").is_err());
        assert_eq!(
            serde_json::from_str::<ConfigNamespace>("\"acme.engineering@1\"")
                .unwrap()
                .as_str(),
            "acme.engineering@1"
        );
    }

    #[test]
    fn equivalent_frontends_have_the_same_semantic_payload() {
        let authority = Authority::new([cap("workspace.read")]);
        let nix = resolve(
            [contribution(
                "phenix-config-nix",
                "flake:acme",
                "nix-rev",
                10,
                authority.clone(),
            )],
            &authority,
        );
        let lua = resolve(
            [contribution(
                "phenix-config-lua",
                "file:phenix.lua",
                "lua-rev",
                10,
                authority.clone(),
            )],
            &authority,
        );

        assert_ne!(nix.entries()[0].attributions, lua.entries()[0].attributions);
        assert_eq!(nix.semantic_payload(), lua.semantic_payload());
    }

    #[test]
    fn equivalent_frontends_preserve_all_source_attribution() {
        let authority = Authority::new([cap("workspace.read")]);
        let nix = contribution(
            "phenix-config-nix",
            "flake:acme",
            "nix-rev",
            10,
            authority.clone(),
        );
        let lua = contribution(
            "phenix-config-lua",
            "file:phenix.lua",
            "lua-rev",
            10,
            authority.clone(),
        );

        let first = resolve([nix.clone(), lua.clone()], &authority);
        let second = resolve([lua, nix], &authority);

        assert_eq!(first, second);
        let attributions = &first.entries()[0].attributions;
        assert_eq!(attributions.len(), 2);
        assert_eq!(
            attributions[0].source.frontend.as_str(),
            "phenix-config-lua"
        );
        assert_eq!(attributions[0].source.source_revision, "lua-rev");
        assert_eq!(
            attributions[1].source.frontend.as_str(),
            "phenix-config-nix"
        );
        assert_eq!(attributions[1].source.source_revision, "nix-rev");
    }

    #[test]
    fn frontend_authority_requests_are_always_attenuated_by_resolver_policy() {
        let read = cap("workspace.read");
        let write = cap("workspace.write");
        let resolved = resolve(
            [contribution(
                "phenix-config-ipc",
                "socket:control",
                "42",
                10,
                Authority::new([read.clone(), write.clone()]),
            )],
            &Authority::new([read.clone()]),
        );
        let entry = &resolved.entries()[0];

        assert!(entry.attributions[0].requested_authority.permits(&write));
        assert!(entry.granted_authority.permits(&read));
        assert!(!entry.granted_authority.permits(&write));
    }

    #[test]
    fn higher_precedence_wins_independent_of_contribution_order() {
        let authority = Authority::default();
        let low = contribution(
            "phenix-config-file",
            "file:low.toml",
            "a",
            1,
            Authority::default(),
        );
        let high = contribution(
            "phenix-config-project",
            "project:root",
            "b",
            20,
            Authority::default(),
        );
        let first = resolve([low.clone(), high.clone()], &authority);
        let second = resolve([high, low], &authority);

        assert_eq!(first, second);
        assert_eq!(first.entries().len(), 1);
        assert_eq!(
            first.entries()[0].attributions[0].source.frontend,
            ConfigurationFrontendId::parse("phenix-config-project").unwrap()
        );
    }

    #[test]
    fn equal_precedence_semantic_conflicts_are_rejected() {
        let mut left = contribution(
            "phenix-config-nix",
            "flake:one",
            "a",
            10,
            Authority::default(),
        );
        let mut right = contribution(
            "phenix-config-lua",
            "file:two.lua",
            "b",
            10,
            Authority::default(),
        );
        left.value = serde_json::json!({"mode":"strict"}).into();
        right.value = serde_json::json!({"mode":"relaxed"}).into();

        assert_eq!(
            ResolvedConfigContributions::try_resolve([left, right], &Authority::default(),)
                .unwrap_err(),
            ConfigMergeError::ConflictingContributions {
                namespace: ConfigNamespace::parse("acme.engineering@1").unwrap(),
                contract_version: 1,
                precedence: 10,
            }
        );
    }

    #[test]
    fn zero_contract_versions_are_rejected_at_frontend_and_resolver_boundaries() {
        let metadata = frontend_metadata(Authority::default());
        let mut frontend = frontend_contribution(ConfigSourceClass::Materialized);
        frontend.contract_version = 0;
        assert_eq!(
            frontend
                .lower(&metadata, &Authority::default())
                .unwrap_err(),
            FrontendConfigError::InvalidContractVersion
        );

        let mut direct = contribution(
            "phenix-config-nix",
            "flake:fixture",
            "revision",
            10,
            Authority::default(),
        );
        direct.contract_version = 0;
        assert_eq!(
            ResolvedConfigContributions::try_resolve([direct], &Authority::default()).unwrap_err(),
            ConfigMergeError::InvalidContractVersion {
                namespace: ConfigNamespace::parse("acme.engineering@1").unwrap(),
            }
        );
    }

    #[test]
    fn canonical_contributions_require_source_identity_and_revision() {
        let mut missing_identity = contribution(
            "phenix-config-nix",
            "",
            "revision",
            10,
            Authority::default(),
        );
        assert_eq!(
            ResolvedConfigContributions::try_resolve(
                [missing_identity.clone()],
                &Authority::default(),
            )
            .unwrap_err(),
            ConfigMergeError::MissingSourceIdentity {
                namespace: ConfigNamespace::parse("acme.engineering@1").unwrap(),
            }
        );

        missing_identity.source.source_identity = "flake:fixture".into();
        missing_identity.source.source_revision.clear();
        assert_eq!(
            ResolvedConfigContributions::try_resolve([missing_identity], &Authority::default())
                .unwrap_err(),
            ConfigMergeError::MissingSourceRevision {
                namespace: ConfigNamespace::parse("acme.engineering@1").unwrap(),
            }
        );
    }

    #[test]
    fn frontend_and_direct_sources_reject_whitespace_only_identity_and_revision() {
        let metadata = frontend_metadata(Authority::default());
        let mut frontend = frontend_contribution(ConfigSourceClass::Materialized);
        frontend.source_identity = " \t ".into();
        assert_eq!(
            frontend
                .clone()
                .lower(&metadata, &Authority::default())
                .unwrap_err(),
            FrontendConfigError::MissingSourceIdentity
        );
        frontend.source_identity = "file:phenix.lua".into();
        frontend.source_revision = "\n ".into();
        assert_eq!(
            frontend
                .lower(&metadata, &Authority::default())
                .unwrap_err(),
            FrontendConfigError::MissingSourceRevision
        );

        let whitespace_identity = contribution(
            "phenix-config-nix",
            "   ",
            "revision",
            10,
            Authority::default(),
        );
        assert_eq!(
            ResolvedConfigContributions::try_resolve([whitespace_identity], &Authority::default(),)
                .unwrap_err(),
            ConfigMergeError::MissingSourceIdentity {
                namespace: ConfigNamespace::parse("acme.engineering@1").unwrap(),
            }
        );

        let whitespace_revision = contribution(
            "phenix-config-nix",
            "flake:fixture",
            " \t ",
            10,
            Authority::default(),
        );
        assert_eq!(
            ResolvedConfigContributions::try_resolve([whitespace_revision], &Authority::default(),)
                .unwrap_err(),
            ConfigMergeError::MissingSourceRevision {
                namespace: ConfigNamespace::parse("acme.engineering@1").unwrap(),
            }
        );
    }

    #[test]
    fn frontend_metadata_is_inspectable_before_activation_and_controls_sources() {
        let read = cap("config.read");
        let metadata = frontend_metadata(Authority::new([read.clone()]));
        assert_eq!(metadata.id.as_str(), "phenix-config-lua");
        assert!(metadata.accepted_source_kinds.contains("lua"));
        assert!(metadata.watch);

        let denied = frontend_contribution(ConfigSourceClass::Materialized)
            .lower(&metadata, &Authority::default())
            .unwrap_err();
        assert_eq!(denied, FrontendConfigError::SourceAuthorityDenied);

        let lowered = frontend_contribution(ConfigSourceClass::Materialized)
            .lower(&metadata, &Authority::new([read]))
            .unwrap();
        assert_eq!(lowered.source.frontend, metadata.id);
        assert_eq!(lowered.source.source_revision, "sha256:fixture");
    }

    #[test]
    fn environment_binding_cannot_change_semantic_configuration() {
        let metadata = frontend_metadata(Authority::default());
        let error = frontend_contribution(ConfigSourceClass::EnvironmentBinding)
            .lower(&metadata, &Authority::default())
            .unwrap_err();
        assert_eq!(
            error,
            FrontendConfigError::EnvironmentBindingChangesSemantics
        );
    }

    #[test]
    fn frontend_cannot_emit_an_undeclared_namespace_or_source_kind() {
        let metadata = frontend_metadata(Authority::default());
        let mut wrong_kind = frontend_contribution(ConfigSourceClass::Materialized);
        wrong_kind.source_kind = "toml".into();
        assert_eq!(
            wrong_kind
                .lower(&metadata, &Authority::default())
                .unwrap_err(),
            FrontendConfigError::UnsupportedSourceKind("toml".into())
        );

        let mut wrong_namespace = frontend_contribution(ConfigSourceClass::Materialized);
        wrong_namespace.namespace = ConfigNamespace::parse("other.policy@1").unwrap();
        assert_eq!(
            wrong_namespace
                .lower(&metadata, &Authority::default())
                .unwrap_err(),
            FrontendConfigError::NamespaceNotExposed(
                ConfigNamespace::parse("other.policy@1").unwrap()
            )
        );
    }
}
