use crate::{
    Authority, CompositionMetadataInput, ConfigurationFrontendId, ConfigurationFrontendMetadata,
    FrontendConfigContribution, FrontendConfigError, MetadataResolutionError, PluginId,
    ResolvedHarness,
};
use std::{
    collections::BTreeMap,
    error::Error,
    fmt::{self, Display, Formatter},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrontendMetadataResolutionError {
    DuplicateOwner {
        frontend: ConfigurationFrontendId,
        first: PluginId,
        second: PluginId,
    },
    DuplicateMetadata(ConfigurationFrontendId),
    Undeclared(ConfigurationFrontendId),
    MissingMetadata {
        frontend: ConfigurationFrontendId,
        plugin: PluginId,
    },
    InvalidMetadataVersion(ConfigurationFrontendId),
    MissingAcceptedSourceKinds(ConfigurationFrontendId),
    EmptyAcceptedSourceKind(ConfigurationFrontendId),
    MissingExposedNamespaces(ConfigurationFrontendId),
    Contribution {
        frontend: ConfigurationFrontendId,
        error: FrontendConfigError,
    },
    Metadata(MetadataResolutionError),
}

impl Display for FrontendMetadataResolutionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateOwner {
                frontend,
                first,
                second,
            } => write!(
                f,
                "configuration frontend {frontend} is declared by both {first} and {second}"
            ),
            Self::DuplicateMetadata(frontend) => {
                write!(f, "duplicate configuration frontend metadata: {frontend}")
            }
            Self::Undeclared(frontend) => {
                write!(
                    f,
                    "configuration frontend {frontend} has no owning plugin package"
                )
            }
            Self::MissingMetadata { frontend, plugin } => {
                write!(
                    f,
                    "plugin package {plugin} declares configuration frontend {frontend} without metadata"
                )
            }
            Self::InvalidMetadataVersion(frontend) => {
                write!(f, "configuration frontend {frontend} has version zero")
            }
            Self::MissingAcceptedSourceKinds(frontend) => {
                write!(
                    f,
                    "configuration frontend {frontend} accepts no source kinds"
                )
            }
            Self::EmptyAcceptedSourceKind(frontend) => {
                write!(
                    f,
                    "configuration frontend {frontend} declares an empty source kind"
                )
            }
            Self::MissingExposedNamespaces(frontend) => {
                write!(
                    f,
                    "configuration frontend {frontend} exposes no configuration namespaces"
                )
            }
            Self::Contribution { frontend, error } => {
                write!(
                    f,
                    "configuration frontend {frontend} rejected contribution: {error:?}"
                )
            }
            Self::Metadata(error) => Display::fmt(error, f),
        }
    }
}

impl Error for FrontendMetadataResolutionError {}

impl From<MetadataResolutionError> for FrontendMetadataResolutionError {
    fn from(error: MetadataResolutionError) -> Self {
        Self::Metadata(error)
    }
}

fn validate_frontend_metadata(
    metadata: &ConfigurationFrontendMetadata,
) -> Result<(), FrontendMetadataResolutionError> {
    if metadata.version == 0 {
        return Err(FrontendMetadataResolutionError::InvalidMetadataVersion(
            metadata.id.clone(),
        ));
    }
    if metadata.accepted_source_kinds.is_empty() {
        return Err(FrontendMetadataResolutionError::MissingAcceptedSourceKinds(
            metadata.id.clone(),
        ));
    }
    if metadata.accepted_source_kinds.iter().any(String::is_empty) {
        return Err(FrontendMetadataResolutionError::EmptyAcceptedSourceKind(
            metadata.id.clone(),
        ));
    }
    if metadata.exposed_namespaces.is_empty() {
        return Err(FrontendMetadataResolutionError::MissingExposedNamespaces(
            metadata.id.clone(),
        ));
    }
    Ok(())
}

impl CompositionMetadataInput {
    pub fn resolve_frontends(
        mut self,
        frontend_metadata: impl IntoIterator<Item = ConfigurationFrontendMetadata>,
        frontend_contributions: impl IntoIterator<
            Item = (ConfigurationFrontendId, FrontendConfigContribution),
        >,
        authority_ceiling: &Authority,
    ) -> Result<ResolvedHarness, FrontendMetadataResolutionError> {
        let mut owners = BTreeMap::new();
        for package in &self.packages {
            for frontend in &package.configuration_frontends {
                if let Some(first) = owners.insert(frontend.clone(), package.manifest.id.clone()) {
                    return Err(FrontendMetadataResolutionError::DuplicateOwner {
                        frontend: frontend.clone(),
                        first,
                        second: package.manifest.id.clone(),
                    });
                }
            }
        }

        let mut metadata_by_id = BTreeMap::new();
        for metadata in frontend_metadata {
            let frontend = metadata.id.clone();
            if !owners.contains_key(&frontend) {
                return Err(FrontendMetadataResolutionError::Undeclared(frontend));
            }
            validate_frontend_metadata(&metadata)?;
            if metadata_by_id.insert(frontend.clone(), metadata).is_some() {
                return Err(FrontendMetadataResolutionError::DuplicateMetadata(frontend));
            }
        }

        for (frontend, plugin) in &owners {
            if !metadata_by_id.contains_key(frontend) {
                return Err(FrontendMetadataResolutionError::MissingMetadata {
                    frontend: frontend.clone(),
                    plugin: plugin.clone(),
                });
            }
        }

        for (frontend, contribution) in frontend_contributions {
            let metadata = metadata_by_id
                .get(&frontend)
                .ok_or_else(|| FrontendMetadataResolutionError::Undeclared(frontend.clone()))?;
            let contribution =
                contribution
                    .lower(metadata, authority_ceiling)
                    .map_err(|error| FrontendMetadataResolutionError::Contribution {
                        frontend: frontend.clone(),
                        error,
                    })?;
            self.configuration.push(contribution);
        }

        let frontend_metadata: Vec<_> = metadata_by_id.into_values().collect();
        let mut resolved = self.resolve(authority_ceiling)?;
        resolved.incorporate_semantic_metadata(&frontend_metadata);
        Ok(resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CapabilityId, CompatibilityMetadata, ComponentHostKind, ComponentId, ComponentManifest,
        ComponentRuntimeMetadata, ComponentStateClass, ConfigContribution,
        ConfigContributionSource, ConfigNamespace, ConfigSourceClass, PluginExecution,
        PluginManifest, PluginPackageMetadata, ReloadPolicy,
    };
    use std::collections::BTreeSet;

    fn plugin() -> PluginId {
        PluginId::parse("fixture.plugin").unwrap()
    }

    fn component() -> ComponentId {
        ComponentId::parse("fixture.component").unwrap()
    }

    fn frontend() -> ConfigurationFrontendId {
        ConfigurationFrontendId::parse("fixture.config").unwrap()
    }

    fn capability(value: &str) -> CapabilityId {
        CapabilityId::parse(value).unwrap()
    }

    fn package() -> PluginPackageMetadata {
        PluginPackageMetadata {
            manifest: PluginManifest {
                id: plugin(),
                version: 1,
                execution: PluginExecution::Embedded,
                dependencies: Vec::new(),
                services: Vec::new(),
                resource_namespaces: Vec::new(),
                maximum_authority: Authority::default(),
            },
            packaged_components: BTreeSet::from([component()]),
            packaged_resources: BTreeSet::new(),
            packaged_skills: BTreeSet::new(),
            compatibility: CompatibilityMetadata {
                minimum_kernel_version: 1,
                maximum_kernel_version: None,
            },
            durable_namespaces: BTreeSet::new(),
            migrations: Vec::new(),
            configuration_frontends: BTreeSet::from([frontend()]),
            component_hosts: BTreeSet::from([ComponentHostKind::EmbeddedRust]),
            reload_policy: ReloadPolicy::Restart,
        }
    }

    fn component_metadata() -> ComponentRuntimeMetadata {
        ComponentRuntimeMetadata {
            manifest: ComponentManifest {
                listeners: Vec::new(),
                id: component(),
                owner: plugin(),
                imports: Vec::new(),
                exports: Vec::new(),
                maximum_authority: Authority::default(),
            },
            version: 1,
            configuration_contracts: BTreeSet::new(),
            requested_capabilities: BTreeSet::new(),
            state_class: ComponentStateClass::Stateless,
            reload_policy: ReloadPolicy::Restart,
            interposition_interfaces: BTreeSet::new(),
            event_contributions: BTreeSet::new(),
            controller_contributions: BTreeSet::new(),
        }
    }

    fn metadata_for(
        id: ConfigurationFrontendId,
        source_kind: &str,
    ) -> ConfigurationFrontendMetadata {
        ConfigurationFrontendMetadata {
            id,
            version: 1,
            accepted_source_kinds: BTreeSet::from([source_kind.into()]),
            exposed_namespaces: BTreeSet::from([
                ConfigNamespace::parse("fixture.policy@1").unwrap()
            ]),
            watch: true,
            required_authority: Authority::default(),
        }
    }

    fn metadata() -> ConfigurationFrontendMetadata {
        metadata_for(frontend(), "fixture")
    }

    fn contribution_for(source_kind: &str, source_identity: &str) -> FrontendConfigContribution {
        FrontendConfigContribution {
            source_kind: source_kind.into(),
            source_identity: source_identity.into(),
            source_revision: "sha256:fixture".into(),
            source_class: ConfigSourceClass::Materialized,
            namespace: ConfigNamespace::parse("fixture.policy@1").unwrap(),
            contract_version: 1,
            precedence: 10,
            value: serde_json::json!({"mode":"strict"}).into(),
            requested_authority: Authority::default(),
        }
    }

    fn frontend_contribution() -> FrontendConfigContribution {
        contribution_for("fixture", "fixture:config")
    }

    #[test]
    fn package_declared_frontend_lowers_through_the_canonical_resolver() {
        let package = package();
        let component = component_metadata();
        let expected = ResolvedHarness::resolve(
            [package.manifest.clone()],
            [component.manifest.clone()],
            [ConfigContribution {
                source: ConfigContributionSource {
                    frontend: frontend(),
                    source_identity: "fixture:config".into(),
                    source_revision: "sha256:fixture".into(),
                },
                namespace: ConfigNamespace::parse("fixture.policy@1").unwrap(),
                contract_version: 1,
                precedence: 10,
                value: serde_json::json!({"mode":"strict"}).into(),
                requested_authority: Authority::default(),
            }],
            &Authority::default(),
        )
        .unwrap();

        let resolved = CompositionMetadataInput {
            packages: vec![package],
            components: vec![component],
            resources: Vec::new(),
            configuration: Vec::new(),
        }
        .resolve_frontends(
            [metadata()],
            [(frontend(), frontend_contribution())],
            &Authority::default(),
        )
        .unwrap();

        assert_ne!(
            resolved.generation(),
            expected.generation(),
            "retained frontend metadata is part of composition identity",
        );
        assert_eq!(resolved.configuration(), expected.configuration());
    }

    #[test]
    fn frontend_metadata_change_creates_a_new_graph_generation() {
        let baseline = CompositionMetadataInput {
            packages: vec![package()],
            components: vec![component_metadata()],
            resources: Vec::new(),
            configuration: Vec::new(),
        }
        .resolve_frontends([metadata()], [], &Authority::default())
        .unwrap();

        let mut changed_metadata = metadata();
        changed_metadata.watch = false;
        let changed = CompositionMetadataInput {
            packages: vec![package()],
            components: vec![component_metadata()],
            resources: Vec::new(),
            configuration: Vec::new(),
        }
        .resolve_frontends([changed_metadata], [], &Authority::default())
        .unwrap();

        assert_ne!(baseline.generation(), changed.generation());
    }

    #[test]
    fn equivalent_frontends_produce_the_same_graph_generation() {
        let nix = ConfigurationFrontendId::parse("fixture.nix").unwrap();
        let lua = ConfigurationFrontendId::parse("fixture.lua").unwrap();
        let mut package = package();
        package.configuration_frontends = BTreeSet::from([nix.clone(), lua.clone()]);
        let frontends = [
            metadata_for(nix.clone(), "nix"),
            metadata_for(lua.clone(), "lua"),
        ];

        let nix_resolved = CompositionMetadataInput {
            packages: vec![package.clone()],
            components: vec![component_metadata()],
            resources: Vec::new(),
            configuration: Vec::new(),
        }
        .resolve_frontends(
            frontends.clone(),
            [(nix, contribution_for("nix", "flake:fixture"))],
            &Authority::default(),
        )
        .unwrap();

        let lua_resolved = CompositionMetadataInput {
            packages: vec![package],
            components: vec![component_metadata()],
            resources: Vec::new(),
            configuration: Vec::new(),
        }
        .resolve_frontends(
            frontends,
            [(lua, contribution_for("lua", "file:fixture.lua"))],
            &Authority::default(),
        )
        .unwrap();

        assert_eq!(nix_resolved.generation(), lua_resolved.generation());
        assert_eq!(
            nix_resolved.configuration().semantic_payload(),
            lua_resolved.configuration().semantic_payload()
        );
        assert_ne!(
            nix_resolved.configuration().entries()[0].attributions,
            lua_resolved.configuration().entries()[0].attributions
        );
    }

    #[test]
    fn frontend_metadata_is_validated_before_activation_even_without_contributions() {
        let mut invalid = metadata();
        invalid.version = 0;
        assert_eq!(
            CompositionMetadataInput {
                packages: vec![package()],
                components: vec![component_metadata()],
                resources: Vec::new(),
                configuration: Vec::new(),
            }
            .resolve_frontends([invalid], [], &Authority::default())
            .unwrap_err(),
            FrontendMetadataResolutionError::InvalidMetadataVersion(frontend())
        );

        let mut invalid = metadata();
        invalid.accepted_source_kinds.clear();
        assert_eq!(
            CompositionMetadataInput {
                packages: vec![package()],
                components: vec![component_metadata()],
                resources: Vec::new(),
                configuration: Vec::new(),
            }
            .resolve_frontends([invalid], [], &Authority::default())
            .unwrap_err(),
            FrontendMetadataResolutionError::MissingAcceptedSourceKinds(frontend())
        );

        let mut invalid = metadata();
        invalid.accepted_source_kinds = BTreeSet::from([String::new()]);
        assert_eq!(
            CompositionMetadataInput {
                packages: vec![package()],
                components: vec![component_metadata()],
                resources: Vec::new(),
                configuration: Vec::new(),
            }
            .resolve_frontends([invalid], [], &Authority::default())
            .unwrap_err(),
            FrontendMetadataResolutionError::EmptyAcceptedSourceKind(frontend())
        );

        let mut invalid = metadata();
        invalid.exposed_namespaces.clear();
        assert_eq!(
            CompositionMetadataInput {
                packages: vec![package()],
                components: vec![component_metadata()],
                resources: Vec::new(),
                configuration: Vec::new(),
            }
            .resolve_frontends([invalid], [], &Authority::default())
            .unwrap_err(),
            FrontendMetadataResolutionError::MissingExposedNamespaces(frontend())
        );
    }

    #[test]
    fn frontend_requested_authority_is_attenuated_by_resolver_policy() {
        let read = capability("workspace.read");
        let write = capability("workspace.write");
        let ceiling = Authority::new([read.clone()]);
        let mut contribution = frontend_contribution();
        contribution.requested_authority = Authority::new([read.clone(), write.clone()]);

        let resolved = CompositionMetadataInput {
            packages: vec![package()],
            components: vec![component_metadata()],
            resources: Vec::new(),
            configuration: Vec::new(),
        }
        .resolve_frontends([metadata()], [(frontend(), contribution)], &ceiling)
        .unwrap();

        let entry = &resolved.configuration().entries()[0];
        assert!(entry.attributions[0].requested_authority.permits(&read));
        assert!(entry.attributions[0].requested_authority.permits(&write));
        assert!(entry.granted_authority.permits(&read));
        assert!(!entry.granted_authority.permits(&write));
    }

    #[test]
    fn undeclared_frontend_is_rejected_before_activation() {
        let mut package = package();
        package.configuration_frontends.clear();

        assert_eq!(
            CompositionMetadataInput {
                packages: vec![package],
                components: vec![component_metadata()],
                resources: Vec::new(),
                configuration: Vec::new(),
            }
            .resolve_frontends(
                [metadata()],
                [(frontend(), frontend_contribution())],
                &Authority::default(),
            )
            .unwrap_err(),
            FrontendMetadataResolutionError::Undeclared(frontend())
        );
    }

    #[test]
    fn declared_frontend_requires_metadata_before_activation() {
        assert_eq!(
            CompositionMetadataInput {
                packages: vec![package()],
                components: vec![component_metadata()],
                resources: Vec::new(),
                configuration: Vec::new(),
            }
            .resolve_frontends([], [], &Authority::default())
            .unwrap_err(),
            FrontendMetadataResolutionError::MissingMetadata {
                frontend: frontend(),
                plugin: plugin(),
            }
        );
    }
}
