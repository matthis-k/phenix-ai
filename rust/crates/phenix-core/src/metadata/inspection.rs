use crate::{
    Authority, ComponentHostKind, ComponentId, ComponentRuntimeMetadata, ComponentStateClass,
    CompositionMetadataInput, ConfigurationFrontendId, ConfigurationFrontendMetadata,
    FrontendConfigContribution, FrontendMetadataResolutionError, GraphGenerationId,
    MetadataResolutionError, PluginPackageMetadata, ReloadPolicy, ResolvedHarness,
    ResolvedHarnessInspection, SkillResourceMetadata,
};
use std::collections::BTreeSet;

/// Validated rich composition metadata retained alongside a resolved generation.
///
/// This snapshot is produced only after the canonical metadata resolver accepts the
/// complete input. It exposes package, component, frontend, and resource metadata for
/// diagnostics without activating plugin behavior.
#[derive(Clone, Debug)]
pub struct ResolvedCompositionMetadata {
    generation: GraphGenerationId,
    packages: Vec<PluginPackageMetadata>,
    components: Vec<ComponentRuntimeMetadata>,
    frontends: Vec<ConfigurationFrontendMetadata>,
    resources: Vec<SkillResourceMetadata>,
}

impl ResolvedCompositionMetadata {
    pub fn generation(&self) -> &GraphGenerationId {
        &self.generation
    }

    pub fn packages(&self) -> &[PluginPackageMetadata] {
        &self.packages
    }

    pub fn components(&self) -> &[ComponentRuntimeMetadata] {
        &self.components
    }

    pub fn frontends(&self) -> &[ConfigurationFrontendMetadata] {
        &self.frontends
    }

    pub fn resources(&self) -> &[SkillResourceMetadata] {
        &self.resources
    }

    pub fn component_package(&self, component: &ComponentId) -> Option<&PluginPackageMetadata> {
        let owner = &self
            .components
            .iter()
            .find(|metadata| &metadata.manifest.id == component)?
            .manifest
            .owner;
        self.packages
            .iter()
            .find(|package| &package.manifest.id == owner)
    }

    pub fn component_hosts(&self, component: &ComponentId) -> Option<&BTreeSet<ComponentHostKind>> {
        self.component_package(component)
            .map(|package| &package.component_hosts)
    }

    pub fn component_state_class(&self, component: &ComponentId) -> Option<ComponentStateClass> {
        self.components
            .iter()
            .find(|metadata| &metadata.manifest.id == component)
            .map(|metadata| metadata.state_class)
    }

    pub fn component_reload_policy(&self, component: &ComponentId) -> Option<ReloadPolicy> {
        self.components
            .iter()
            .find(|metadata| &metadata.manifest.id == component)
            .map(|metadata| metadata.reload_policy)
    }
}

impl ResolvedHarnessInspection {
    pub fn component_runtime_metadata(
        &self,
        component: &ComponentId,
    ) -> Option<&ComponentRuntimeMetadata> {
        self.component_metadata()
            .iter()
            .find(|metadata| &metadata.manifest.id == component)
    }

    pub fn component_package_metadata(
        &self,
        component: &ComponentId,
    ) -> Option<&PluginPackageMetadata> {
        let owner = &self.component_runtime_metadata(component)?.manifest.owner;
        self.package_metadata()
            .iter()
            .find(|package| &package.manifest.id == owner)
    }

    pub fn component_hosts(&self, component: &ComponentId) -> Option<&BTreeSet<ComponentHostKind>> {
        self.component_package_metadata(component)
            .map(|package| &package.component_hosts)
    }

    pub fn component_state_class(&self, component: &ComponentId) -> Option<ComponentStateClass> {
        self.component_runtime_metadata(component)
            .map(|metadata| metadata.state_class)
    }

    pub fn component_reload_policy(&self, component: &ComponentId) -> Option<ReloadPolicy> {
        self.component_runtime_metadata(component)
            .map(|metadata| metadata.reload_policy)
    }
}

impl CompositionMetadataInput {
    /// Resolve through the canonical metadata path and retain the validated rich
    /// metadata needed for pre-activation inspection.
    pub fn resolve_inspectable(
        self,
        authority_ceiling: &Authority,
    ) -> Result<(ResolvedHarness, ResolvedCompositionMetadata), MetadataResolutionError> {
        let mut packages = self.packages.clone();
        packages.sort_by(|left, right| left.manifest.id.cmp(&right.manifest.id));
        let mut components = self.components.clone();
        components.sort_by(|left, right| left.manifest.id.cmp(&right.manifest.id));
        let mut resources = self.resources.clone();
        resources.sort_by(|left, right| left.identity.cmp(&right.identity));

        let resolved = self.resolve(authority_ceiling)?;
        let metadata = ResolvedCompositionMetadata {
            generation: resolved.generation().clone(),
            packages,
            components,
            frontends: Vec::new(),
            resources,
        };
        Ok((resolved, metadata))
    }

    /// Resolve configuration frontends through the same canonical path while retaining
    /// the validated frontend metadata for pre-activation inspection.
    pub fn resolve_frontends_inspectable(
        self,
        frontend_metadata: impl IntoIterator<Item = ConfigurationFrontendMetadata>,
        frontend_contributions: impl IntoIterator<
            Item = (ConfigurationFrontendId, FrontendConfigContribution),
        >,
        authority_ceiling: &Authority,
    ) -> Result<(ResolvedHarness, ResolvedCompositionMetadata), FrontendMetadataResolutionError>
    {
        let mut packages = self.packages.clone();
        packages.sort_by(|left, right| left.manifest.id.cmp(&right.manifest.id));
        let mut components = self.components.clone();
        components.sort_by(|left, right| left.manifest.id.cmp(&right.manifest.id));
        let mut resources = self.resources.clone();
        resources.sort_by(|left, right| left.identity.cmp(&right.identity));
        let mut frontends: Vec<_> = frontend_metadata.into_iter().collect();
        frontends.sort_by(|left, right| left.id.cmp(&right.id));

        let resolved =
            self.resolve_frontends(frontends.clone(), frontend_contributions, authority_ceiling)?;
        let metadata = ResolvedCompositionMetadata {
            generation: resolved.generation().clone(),
            packages,
            components,
            frontends,
            resources,
        };
        Ok((resolved, metadata))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CompatibilityMetadata, ComponentHostKind, ComponentId, ComponentManifest,
        ComponentStateClass, ConfigNamespace, PluginExecution, PluginId, PluginManifest,
        ReloadPolicy,
    };
    use std::collections::BTreeSet;

    fn fixture() -> (
        PluginId,
        ComponentId,
        PluginPackageMetadata,
        ComponentRuntimeMetadata,
    ) {
        let plugin = PluginId::parse("fixture.metadata").unwrap();
        let component = ComponentId::parse("fixture.metadata.component").unwrap();
        let package = PluginPackageMetadata {
            manifest: PluginManifest {
                id: plugin.clone(),
                version: 3,
                execution: PluginExecution::Embedded,
                dependencies: Vec::new(),
                services: Vec::new(),
                resource_namespaces: Vec::new(),
                maximum_authority: Authority::default(),
            },
            packaged_components: BTreeSet::from([component.clone()]),
            packaged_resources: BTreeSet::new(),
            packaged_skills: BTreeSet::new(),
            compatibility: CompatibilityMetadata {
                minimum_kernel_version: 1,
                maximum_kernel_version: Some(4),
            },
            durable_namespaces: BTreeSet::new(),
            migrations: Vec::new(),
            configuration_frontends: BTreeSet::new(),
            component_hosts: BTreeSet::from([ComponentHostKind::EmbeddedRust]),
            reload_policy: ReloadPolicy::DrainAndRestart,
        };
        let component_metadata = ComponentRuntimeMetadata {
            manifest: ComponentManifest {
                listeners: Vec::new(),
                id: component.clone(),
                owner: plugin.clone(),
                imports: Vec::new(),
                exports: Vec::new(),
                maximum_authority: Authority::default(),
            },
            version: 7,
            configuration_contracts: BTreeSet::new(),
            requested_capabilities: BTreeSet::new(),
            state_class: ComponentStateClass::Durable,
            reload_policy: ReloadPolicy::MigrationRequired,
            interposition_interfaces: BTreeSet::new(),
            event_contributions: BTreeSet::new(),
            controller_contributions: BTreeSet::new(),
        };
        (plugin, component, package, component_metadata)
    }

    #[test]
    fn canonical_resolution_retains_rich_metadata_for_pre_activation_inspection() {
        let (_, component, package, component_metadata) = fixture();

        let (resolved, metadata) = CompositionMetadataInput {
            packages: vec![package],
            components: vec![component_metadata],
            resources: Vec::new(),
            configuration: Vec::new(),
        }
        .resolve_inspectable(&Authority::default())
        .unwrap();

        assert_eq!(metadata.generation(), resolved.generation());
        assert_eq!(metadata.packages().len(), 1);
        assert_eq!(metadata.packages()[0].manifest.version, 3);
        assert_eq!(
            metadata.packages()[0].reload_policy,
            ReloadPolicy::DrainAndRestart
        );
        assert!(metadata.packages()[0]
            .component_hosts
            .contains(&ComponentHostKind::EmbeddedRust));
        assert_eq!(metadata.components().len(), 1);
        assert_eq!(metadata.components()[0].manifest.id, component);
        assert_eq!(metadata.components()[0].version, 7);
        assert_eq!(
            metadata.components()[0].state_class,
            ComponentStateClass::Durable
        );
        assert_eq!(
            metadata.components()[0].reload_policy,
            ReloadPolicy::MigrationRequired
        );
        assert_eq!(
            metadata
                .component_package(&component)
                .unwrap()
                .manifest
                .version,
            3
        );
        assert_eq!(
            metadata.component_hosts(&component).unwrap(),
            &BTreeSet::from([ComponentHostKind::EmbeddedRust])
        );
        assert_eq!(
            metadata.component_state_class(&component),
            Some(ComponentStateClass::Durable)
        );
        assert_eq!(
            metadata.component_reload_policy(&component),
            Some(ReloadPolicy::MigrationRequired)
        );

        let inspection =
            ResolvedHarnessInspection::from_resolved_with_metadata(&resolved, &metadata).unwrap();
        assert_eq!(
            inspection
                .component_runtime_metadata(&component)
                .unwrap()
                .version,
            7
        );
        assert_eq!(
            inspection
                .component_package_metadata(&component)
                .unwrap()
                .manifest
                .version,
            3
        );
        assert_eq!(
            inspection.component_hosts(&component).unwrap(),
            &BTreeSet::from([ComponentHostKind::EmbeddedRust])
        );
        assert_eq!(
            inspection.component_state_class(&component),
            Some(ComponentStateClass::Durable)
        );
        assert_eq!(
            inspection.component_reload_policy(&component),
            Some(ReloadPolicy::MigrationRequired)
        );
        assert!(metadata.frontends().is_empty());
    }

    #[test]
    fn plain_resolved_inspection_does_not_invent_rich_component_metadata() {
        let (_, component, package, component_metadata) = fixture();
        let resolved = CompositionMetadataInput {
            packages: vec![package],
            components: vec![component_metadata],
            resources: Vec::new(),
            configuration: Vec::new(),
        }
        .resolve(&Authority::default())
        .unwrap();
        let inspection = ResolvedHarnessInspection::from_resolved(&resolved);

        assert!(inspection.component_runtime_metadata(&component).is_none());
        assert!(inspection.component_package_metadata(&component).is_none());
        assert!(inspection.component_hosts(&component).is_none());
        assert_eq!(inspection.component_state_class(&component), None);
        assert_eq!(inspection.component_reload_policy(&component), None);
    }

    #[test]
    fn canonical_frontend_resolution_retains_frontend_metadata_for_inspection() {
        let (_, _, mut package, component_metadata) = fixture();
        let frontend = ConfigurationFrontendId::parse("fixture.config").unwrap();
        let namespace = ConfigNamespace::parse("fixture.policy@1").unwrap();
        package.configuration_frontends.insert(frontend.clone());
        let frontend_metadata = ConfigurationFrontendMetadata {
            id: frontend,
            version: 2,
            accepted_source_kinds: BTreeSet::from(["fixture".into()]),
            exposed_namespaces: BTreeSet::from([namespace]),
            watch: true,
            required_authority: Authority::default(),
        };

        let (resolved, metadata) = CompositionMetadataInput {
            packages: vec![package],
            components: vec![component_metadata],
            resources: Vec::new(),
            configuration: Vec::new(),
        }
        .resolve_frontends_inspectable([frontend_metadata.clone()], [], &Authority::default())
        .unwrap();

        assert_eq!(metadata.generation(), resolved.generation());
        assert_eq!(metadata.frontends(), &[frontend_metadata]);
    }
}
