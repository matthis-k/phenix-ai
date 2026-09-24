use crate::{
    Authority, ComponentId, ComponentRuntimeMetadata, CompositionMetadataError, ConfigContribution,
    LayerPolicy, PluginId, PluginPackageMetadata, ResolvedHarness, ResolvedHarnessError, ServiceId,
    SkillResourceMetadata,
};
use std::{
    collections::BTreeMap,
    error::Error,
    fmt::{self, Display, Formatter},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MetadataResolutionError {
    Package {
        plugin: PluginId,
        error: CompositionMetadataError,
    },
    Component {
        component: ComponentId,
        error: CompositionMetadataError,
    },
    Resource {
        resource: String,
        error: CompositionMetadataError,
    },
    DuplicatePackage(PluginId),
    DuplicateComponent(ComponentId),
    DuplicateResource(String),
    UnknownComponentOwner {
        component: ComponentId,
        plugin: PluginId,
    },
    ComponentNotPackaged {
        component: ComponentId,
        plugin: PluginId,
    },
    MissingPackagedComponent {
        component: ComponentId,
        plugin: PluginId,
    },
    ResourceNotPackaged {
        resource: String,
    },
    MissingPackagedResource {
        resource: String,
        plugin: PluginId,
    },
    ResourceOwnedByMultiplePackages {
        resource: String,
        first: PluginId,
        second: PluginId,
    },
    AmbiguousPackagedResourceKind {
        resource: String,
        plugin: PluginId,
    },
    Resolver(ResolvedHarnessError),
}

impl Display for MetadataResolutionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Package { plugin, error } => {
                write!(f, "plugin package {plugin} metadata is invalid: {error:?}")
            }
            Self::Component { component, error } => {
                write!(f, "component {component} metadata is invalid: {error:?}")
            }
            Self::Resource { resource, error } => {
                write!(
                    f,
                    "skill/resource {resource} metadata is invalid: {error:?}"
                )
            }
            Self::DuplicatePackage(plugin) => {
                write!(f, "duplicate plugin package metadata: {plugin}")
            }
            Self::DuplicateComponent(component) => {
                write!(f, "duplicate component metadata: {component}")
            }
            Self::DuplicateResource(resource) => {
                write!(f, "duplicate skill/resource metadata: {resource}")
            }
            Self::UnknownComponentOwner { component, plugin } => {
                write!(
                    f,
                    "component {component} belongs to unknown package {plugin}"
                )
            }
            Self::ComponentNotPackaged { component, plugin } => {
                write!(
                    f,
                    "component {component} is not declared by package {plugin}"
                )
            }
            Self::MissingPackagedComponent { component, plugin } => {
                write!(f, "package {plugin} declares missing component {component}")
            }
            Self::ResourceNotPackaged { resource } => {
                write!(f, "skill/resource {resource} has no owning plugin package")
            }
            Self::MissingPackagedResource { resource, plugin } => {
                write!(
                    f,
                    "package {plugin} declares missing skill/resource {resource}"
                )
            }
            Self::ResourceOwnedByMultiplePackages {
                resource,
                first,
                second,
            } => write!(
                f,
                "skill/resource {resource} is owned by both {first} and {second}"
            ),
            Self::AmbiguousPackagedResourceKind { resource, plugin } => write!(
                f,
                "package {plugin} declares {resource} as both a skill and a resource"
            ),
            Self::Resolver(error) => Display::fmt(error, f),
        }
    }
}

impl Error for MetadataResolutionError {}

impl From<ResolvedHarnessError> for MetadataResolutionError {
    fn from(error: ResolvedHarnessError) -> Self {
        Self::Resolver(error)
    }
}

#[derive(Clone, Debug, Default)]
pub struct CompositionMetadataInput {
    pub packages: Vec<PluginPackageMetadata>,
    pub components: Vec<ComponentRuntimeMetadata>,
    pub resources: Vec<SkillResourceMetadata>,
    pub configuration: Vec<ConfigContribution>,
}

impl CompositionMetadataInput {
    pub fn resolve(
        self,
        authority_ceiling: &Authority,
    ) -> Result<ResolvedHarness, MetadataResolutionError> {
        self.resolve_with_layer_policies(BTreeMap::new(), authority_ceiling)
    }

    pub fn resolve_with_layer_policies(
        self,
        layer_policies: BTreeMap<ServiceId, Vec<LayerPolicy>>,
        authority_ceiling: &Authority,
    ) -> Result<ResolvedHarness, MetadataResolutionError> {
        let mut packages = BTreeMap::new();
        for package in self.packages {
            package.validate_pre_activation().map_err(|error| {
                MetadataResolutionError::Package {
                    plugin: package.manifest.id.clone(),
                    error,
                }
            })?;
            let plugin = package.manifest.id.clone();
            if packages.insert(plugin.clone(), package).is_some() {
                return Err(MetadataResolutionError::DuplicatePackage(plugin));
            }
        }

        let mut components = BTreeMap::new();
        for component in self.components {
            component.validate_pre_activation().map_err(|error| {
                MetadataResolutionError::Component {
                    component: component.manifest.id.clone(),
                    error,
                }
            })?;
            let owner = packages.get(&component.manifest.owner).ok_or_else(|| {
                MetadataResolutionError::UnknownComponentOwner {
                    component: component.manifest.id.clone(),
                    plugin: component.manifest.owner.clone(),
                }
            })?;
            if !owner.packaged_components.contains(&component.manifest.id) {
                return Err(MetadataResolutionError::ComponentNotPackaged {
                    component: component.manifest.id.clone(),
                    plugin: component.manifest.owner.clone(),
                });
            }
            let id = component.manifest.id.clone();
            if components.insert(id.clone(), component).is_some() {
                return Err(MetadataResolutionError::DuplicateComponent(id));
            }
        }

        for package in packages.values() {
            for component in &package.packaged_components {
                if !components.contains_key(component) {
                    return Err(MetadataResolutionError::MissingPackagedComponent {
                        component: component.clone(),
                        plugin: package.manifest.id.clone(),
                    });
                }
            }
        }

        let mut resource_owners = BTreeMap::<String, PluginId>::new();
        for package in packages.values() {
            if let Some(skill) = package
                .packaged_skills
                .iter()
                .find(|skill| package.packaged_resources.contains(skill.as_str()))
            {
                return Err(MetadataResolutionError::AmbiguousPackagedResourceKind {
                    resource: skill.to_string(),
                    plugin: package.manifest.id.clone(),
                });
            }

            let mut register_owner = |resource: String| {
                if let Some(first) =
                    resource_owners.insert(resource.clone(), package.manifest.id.clone())
                {
                    return Err(MetadataResolutionError::ResourceOwnedByMultiplePackages {
                        resource,
                        first,
                        second: package.manifest.id.clone(),
                    });
                }
                Ok(())
            };
            for resource in &package.packaged_resources {
                register_owner(resource.clone())?;
            }
            for skill in &package.packaged_skills {
                register_owner(skill.to_string())?;
            }
        }

        let mut resources = BTreeMap::new();
        for resource in self.resources {
            let identity = resource.identity.clone();
            resource.validate_pre_activation().map_err(|error| {
                MetadataResolutionError::Resource {
                    resource: identity.clone(),
                    error,
                }
            })?;
            if resources.insert(identity.clone(), resource).is_some() {
                return Err(MetadataResolutionError::DuplicateResource(identity));
            }
        }
        for resource in resources.keys() {
            if !resource_owners.contains_key(resource) {
                return Err(MetadataResolutionError::ResourceNotPackaged {
                    resource: resource.clone(),
                });
            }
        }
        for (resource, plugin) in &resource_owners {
            if !resources.contains_key(resource) {
                return Err(MetadataResolutionError::MissingPackagedResource {
                    resource: resource.clone(),
                    plugin: plugin.clone(),
                });
            }
        }

        let package_metadata: Vec<_> = packages.into_values().collect();
        let component_metadata: Vec<_> = components.into_values().collect();
        let mut resolved = ResolvedHarness::resolve_with_resources_and_layer_policies(
            package_metadata
                .iter()
                .map(|package| package.manifest.clone()),
            component_metadata
                .iter()
                .map(|component| component.manifest.clone()),
            resources.into_values(),
            self.configuration,
            layer_policies,
            authority_ceiling,
        )?;
        resolved.incorporate_semantic_metadata(&(&package_metadata, &component_metadata));
        Ok(resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CompatibilityMetadata, ComponentHostKind, ComponentManifest, ComponentStateClass,
        PluginExecution, PluginManifest, ReloadPolicy, SkillId,
    };
    use std::collections::BTreeSet;

    fn plugin() -> PluginId {
        PluginId::parse("fixture.plugin").unwrap()
    }

    fn component() -> ComponentId {
        ComponentId::parse("fixture.component").unwrap()
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
            configuration_frontends: BTreeSet::new(),
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

    fn resource(identity: &str) -> SkillResourceMetadata {
        SkillResourceMetadata {
            identity: identity.into(),
            version: 1,
            content_identity: "sha256:fixture".into(),
            dependencies: BTreeSet::new(),
            conflicts: BTreeSet::new(),
            triggers: BTreeSet::new(),
            scope: "execution".into(),
            priority: 0,
            required_tools: BTreeSet::new(),
            required_interfaces: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
            compatibility: CompatibilityMetadata {
                minimum_kernel_version: 1,
                maximum_kernel_version: None,
            },
            invalidation_targets: BTreeSet::new(),
            reload_policy: ReloadPolicy::Restart,
        }
    }

    #[test]
    fn rich_metadata_lowers_into_the_same_canonical_resolver() {
        let package = package();
        let component_metadata_value = component_metadata();
        let resolved = CompositionMetadataInput {
            packages: vec![package.clone()],
            components: vec![component_metadata_value.clone()],
            resources: Vec::new(),
            configuration: Vec::new(),
        }
        .resolve(&Authority::default())
        .unwrap();

        assert_eq!(resolved.plugins(), [package.manifest]);
        assert_eq!(resolved.components(), [component_metadata_value.manifest]);
        assert_eq!(resolved.components()[0].id, component());
    }

    #[test]
    fn rich_metadata_and_layer_policy_use_one_canonical_resolver_generation() {
        let service = crate::ServiceId::parse("fixture.service@1").unwrap();
        let mut package = package();
        package.manifest.services.push(crate::ServiceContribution {
            role: crate::ServiceRole::Layer,
            service: service.clone(),
            priority: 10,
            required_authority: Authority::default(),
        });
        let policy = crate::LayerPolicy {
            plugin: package.manifest.id.clone(),
            priority: 10,
            required: false,
            enabled: true,
        };
        let resolved = CompositionMetadataInput {
            packages: vec![package],
            components: vec![component_metadata()],
            resources: Vec::new(),
            configuration: Vec::new(),
        }
        .resolve_with_layer_policies(
            BTreeMap::from([(service.clone(), vec![policy.clone()])]),
            &Authority::default(),
        )
        .unwrap();

        assert_eq!(resolved.layer_policies().get(&service), Some(&vec![policy]));
    }

    #[test]
    fn lifecycle_metadata_changes_semantic_generation_identity() {
        let package = package();
        let component = component_metadata();
        let baseline = CompositionMetadataInput {
            packages: vec![package.clone()],
            components: vec![component.clone()],
            resources: Vec::new(),
            configuration: Vec::new(),
        }
        .resolve(&Authority::default())
        .unwrap();

        let mut changed = component;
        changed.state_class = ComponentStateClass::Durable;
        changed.reload_policy = ReloadPolicy::MigrationRequired;
        let changed = CompositionMetadataInput {
            packages: vec![package],
            components: vec![changed],
            resources: Vec::new(),
            configuration: Vec::new(),
        }
        .resolve(&Authority::default())
        .unwrap();

        assert_ne!(baseline.generation(), changed.generation());
    }

    #[test]
    fn rich_metadata_rejects_component_not_owned_by_its_package_before_activation() {
        let mut package = package();
        package.packaged_components.clear();

        assert_eq!(
            CompositionMetadataInput {
                packages: vec![package],
                components: vec![component_metadata()],
                resources: Vec::new(),
                configuration: Vec::new(),
            }
            .resolve(&Authority::default())
            .unwrap_err(),
            MetadataResolutionError::ComponentNotPackaged {
                component: component(),
                plugin: plugin(),
            }
        );
    }

    #[test]
    fn package_metadata_is_enforced_before_canonical_resolution() {
        let mut package = package();
        package.manifest.execution = PluginExecution::ResourceOnly;

        assert!(matches!(
            CompositionMetadataInput {
                packages: vec![package],
                components: vec![component_metadata()],
                resources: Vec::new(),
                configuration: Vec::new(),
            }
            .resolve(&Authority::default()),
            Err(MetadataResolutionError::Package {
                error: CompositionMetadataError::ResourceOnlyPackageHasComponents,
                ..
            })
        ));
    }

    #[test]
    fn resource_metadata_is_enforced_before_canonical_resolution() {
        let mut owner = package();
        owner.packaged_resources.insert("fixture.resource".into());
        let mut invalid = resource("fixture.resource");
        invalid.content_identity.clear();

        assert_eq!(
            CompositionMetadataInput {
                packages: vec![owner],
                components: vec![component_metadata()],
                resources: vec![invalid],
                configuration: Vec::new(),
            }
            .resolve(&Authority::default())
            .unwrap_err(),
            MetadataResolutionError::Resource {
                resource: "fixture.resource".into(),
                error: CompositionMetadataError::MissingContentIdentity,
            }
        );
    }

    #[test]
    fn duplicate_package_component_and_resource_metadata_is_rejected() {
        assert_eq!(
            CompositionMetadataInput {
                packages: vec![package(), package()],
                components: Vec::new(),
                resources: Vec::new(),
                configuration: Vec::new(),
            }
            .resolve(&Authority::default())
            .unwrap_err(),
            MetadataResolutionError::DuplicatePackage(plugin())
        );

        assert_eq!(
            CompositionMetadataInput {
                packages: vec![package()],
                components: vec![component_metadata(), component_metadata()],
                resources: Vec::new(),
                configuration: Vec::new(),
            }
            .resolve(&Authority::default())
            .unwrap_err(),
            MetadataResolutionError::DuplicateComponent(component())
        );

        let mut owner = package();
        owner.packaged_resources.insert("fixture.resource".into());
        assert_eq!(
            CompositionMetadataInput {
                packages: vec![owner],
                components: vec![component_metadata()],
                resources: vec![resource("fixture.resource"), resource("fixture.resource")],
                configuration: Vec::new(),
            }
            .resolve(&Authority::default())
            .unwrap_err(),
            MetadataResolutionError::DuplicateResource("fixture.resource".into())
        );
    }

    #[test]
    fn resource_metadata_requires_exactly_one_package_owner_before_activation() {
        let mut owner = package();
        owner.packaged_resources.insert("fixture.resource".into());
        let mut second = package();
        second.manifest.id = PluginId::parse("fixture.second").unwrap();
        second.packaged_components.clear();
        second.packaged_resources.insert("fixture.resource".into());

        assert_eq!(
            CompositionMetadataInput {
                packages: vec![owner, second],
                components: vec![component_metadata()],
                resources: vec![resource("fixture.resource")],
                configuration: Vec::new(),
            }
            .resolve(&Authority::default())
            .unwrap_err(),
            MetadataResolutionError::ResourceOwnedByMultiplePackages {
                resource: "fixture.resource".into(),
                first: plugin(),
                second: PluginId::parse("fixture.second").unwrap(),
            }
        );
    }

    #[test]
    fn package_resource_declarations_and_metadata_must_match_before_activation() {
        let unowned = CompositionMetadataInput {
            packages: vec![package()],
            components: vec![component_metadata()],
            resources: vec![resource("fixture.resource")],
            configuration: Vec::new(),
        }
        .resolve(&Authority::default())
        .unwrap_err();
        assert_eq!(
            unowned,
            MetadataResolutionError::ResourceNotPackaged {
                resource: "fixture.resource".into(),
            }
        );

        let mut owner = package();
        owner
            .packaged_skills
            .insert(SkillId::parse("fixture.skill").unwrap());
        let missing = CompositionMetadataInput {
            packages: vec![owner],
            components: vec![component_metadata()],
            resources: Vec::new(),
            configuration: Vec::new(),
        }
        .resolve(&Authority::default())
        .unwrap_err();
        assert_eq!(
            missing,
            MetadataResolutionError::MissingPackagedResource {
                resource: "fixture.skill".into(),
                plugin: plugin(),
            }
        );
    }

    #[test]
    fn package_cannot_classify_one_identity_as_both_skill_and_resource() {
        let mut owner = package();
        owner.packaged_resources.insert("fixture.shared".into());
        owner
            .packaged_skills
            .insert(SkillId::parse("fixture.shared").unwrap());

        assert_eq!(
            CompositionMetadataInput {
                packages: vec![owner],
                components: vec![component_metadata()],
                resources: vec![resource("fixture.shared")],
                configuration: Vec::new(),
            }
            .resolve(&Authority::default())
            .unwrap_err(),
            MetadataResolutionError::AmbiguousPackagedResourceKind {
                resource: "fixture.shared".into(),
                plugin: plugin(),
            }
        );
    }
}
