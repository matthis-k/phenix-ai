use crate::{
    CallableId, CapabilityId, ComponentId, ComponentManifest, ConfigNamespace,
    ConfigurationFrontendId, EventTypeId, InterfaceId, PluginExecution, PluginId, PluginManifest,
    ResourceNamespace, SkillId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const KERNEL_COMPATIBILITY_VERSION: u64 = 1;

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentHostKind {
    EmbeddedRust,
    EmbeddedLua,
    ExternalIpc,
    Wasm,
    Remote,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentStateClass {
    Stateless,
    Ephemeral,
    Durable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReloadPolicy {
    Retain,
    Restart,
    DrainAndRestart,
    MigrationRequired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CompatibilityMetadata {
    pub minimum_kernel_version: u64,
    pub maximum_kernel_version: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DurableMigrationMetadata {
    pub namespace: ResourceNamespace,
    pub from_version: u64,
    pub to_version: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PluginPackageMetadata {
    #[serde(flatten)]
    pub manifest: PluginManifest,
    pub packaged_components: BTreeSet<ComponentId>,
    pub packaged_resources: BTreeSet<String>,
    pub packaged_skills: BTreeSet<SkillId>,
    pub compatibility: CompatibilityMetadata,
    pub durable_namespaces: BTreeSet<ResourceNamespace>,
    pub migrations: Vec<DurableMigrationMetadata>,
    pub configuration_frontends: BTreeSet<ConfigurationFrontendId>,
    pub component_hosts: BTreeSet<ComponentHostKind>,
    pub reload_policy: ReloadPolicy,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComponentRuntimeMetadata {
    #[serde(flatten)]
    pub manifest: ComponentManifest,
    pub version: u64,
    pub configuration_contracts: BTreeSet<ConfigNamespace>,
    pub requested_capabilities: BTreeSet<CapabilityId>,
    pub state_class: ComponentStateClass,
    pub reload_policy: ReloadPolicy,
    pub interposition_interfaces: BTreeSet<InterfaceId>,
    pub event_contributions: BTreeSet<EventTypeId>,
    pub controller_contributions: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillResourceMetadata {
    pub identity: String,
    pub version: u64,
    pub content_identity: String,
    pub dependencies: BTreeSet<String>,
    pub conflicts: BTreeSet<String>,
    pub triggers: BTreeSet<String>,
    pub scope: String,
    pub priority: i32,
    pub required_tools: BTreeSet<CallableId>,
    pub required_interfaces: BTreeSet<InterfaceId>,
    pub required_capabilities: BTreeSet<CapabilityId>,
    pub compatibility: CompatibilityMetadata,
    pub invalidation_targets: BTreeSet<String>,
    pub reload_policy: ReloadPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompositionMetadataError {
    ZeroVersion,
    InvalidCompatibilityRange,
    IncompatibleKernelVersion {
        current: u64,
        minimum: u64,
        maximum: Option<u64>,
    },
    DurableNamespaceOutsideRuntimeManifest(ResourceNamespace),
    MigrationOutsideDeclaredNamespace(ResourceNamespace),
    InvalidMigrationRange(ResourceNamespace),
    MissingComponentHost,
    ResourceOnlyPackageHasComponents,
    EmptyPackagedResource,
    RequestedCapabilityExceedsMaximum(CapabilityId),
    EmptyControllerContribution,
    MissingIdentity,
    MissingContentIdentity,
    MissingScope,
    EmptyDependency,
    EmptyConflict,
    EmptyTrigger,
    EmptyInvalidationTarget,
    ResourceIdentityMismatch {
        current: String,
        next: String,
    },
    DependencyConflict(String),
    DuplicateComponentImport(InterfaceId),
    DuplicateComponentExport(InterfaceId),
}

impl CompatibilityMetadata {
    fn validate(&self) -> Result<(), CompositionMetadataError> {
        if self.minimum_kernel_version == 0 {
            return Err(CompositionMetadataError::ZeroVersion);
        }
        if let Some(maximum) = self.maximum_kernel_version {
            if maximum < self.minimum_kernel_version {
                return Err(CompositionMetadataError::InvalidCompatibilityRange);
            }
        }
        if KERNEL_COMPATIBILITY_VERSION < self.minimum_kernel_version
            || self
                .maximum_kernel_version
                .is_some_and(|maximum| KERNEL_COMPATIBILITY_VERSION > maximum)
        {
            return Err(CompositionMetadataError::IncompatibleKernelVersion {
                current: KERNEL_COMPATIBILITY_VERSION,
                minimum: self.minimum_kernel_version,
                maximum: self.maximum_kernel_version,
            });
        }
        Ok(())
    }
}

impl PluginPackageMetadata {
    pub fn plugin(&self) -> &PluginId {
        &self.manifest.id
    }

    pub fn validate_pre_activation(&self) -> Result<(), CompositionMetadataError> {
        if self.manifest.version == 0 {
            return Err(CompositionMetadataError::ZeroVersion);
        }
        self.compatibility.validate()?;
        if matches!(self.manifest.execution, PluginExecution::ResourceOnly)
            && !self.packaged_components.is_empty()
        {
            return Err(CompositionMetadataError::ResourceOnlyPackageHasComponents);
        }
        if !self.packaged_components.is_empty() && self.component_hosts.is_empty() {
            return Err(CompositionMetadataError::MissingComponentHost);
        }
        if self.packaged_resources.iter().any(String::is_empty) {
            return Err(CompositionMetadataError::EmptyPackagedResource);
        }
        for namespace in &self.durable_namespaces {
            if !self.manifest.resource_namespaces.contains(namespace) {
                return Err(
                    CompositionMetadataError::DurableNamespaceOutsideRuntimeManifest(
                        namespace.clone(),
                    ),
                );
            }
        }
        for migration in &self.migrations {
            if !self.durable_namespaces.contains(&migration.namespace) {
                return Err(CompositionMetadataError::MigrationOutsideDeclaredNamespace(
                    migration.namespace.clone(),
                ));
            }
            if migration.from_version >= migration.to_version {
                return Err(CompositionMetadataError::InvalidMigrationRange(
                    migration.namespace.clone(),
                ));
            }
        }
        Ok(())
    }
}

impl ComponentRuntimeMetadata {
    pub fn component(&self) -> &ComponentId {
        &self.manifest.id
    }

    pub fn validate_pre_activation(&self) -> Result<(), CompositionMetadataError> {
        if self.version == 0 {
            return Err(CompositionMetadataError::ZeroVersion);
        }
        for capability in &self.requested_capabilities {
            if !self.manifest.maximum_authority.permits(capability) {
                return Err(CompositionMetadataError::RequestedCapabilityExceedsMaximum(
                    capability.clone(),
                ));
            }
        }
        if self.controller_contributions.iter().any(String::is_empty) {
            return Err(CompositionMetadataError::EmptyControllerContribution);
        }

        let mut imports = BTreeSet::new();
        for import in &self.manifest.imports {
            if !imports.insert(import.interface.clone()) {
                return Err(CompositionMetadataError::DuplicateComponentImport(
                    import.interface.clone(),
                ));
            }
        }

        let mut exports = BTreeSet::new();
        for export in &self.manifest.exports {
            if !exports.insert(export.interface.clone()) {
                return Err(CompositionMetadataError::DuplicateComponentExport(
                    export.interface.clone(),
                ));
            }
        }
        Ok(())
    }
}

impl SkillResourceMetadata {
    pub fn validate_pre_activation(&self) -> Result<(), CompositionMetadataError> {
        if self.identity.is_empty() {
            return Err(CompositionMetadataError::MissingIdentity);
        }
        if self.version == 0 {
            return Err(CompositionMetadataError::ZeroVersion);
        }
        self.compatibility.validate()?;
        if self.content_identity.is_empty() {
            return Err(CompositionMetadataError::MissingContentIdentity);
        }
        if self.scope.is_empty() {
            return Err(CompositionMetadataError::MissingScope);
        }
        if self.dependencies.iter().any(String::is_empty) {
            return Err(CompositionMetadataError::EmptyDependency);
        }
        if self.conflicts.iter().any(String::is_empty) {
            return Err(CompositionMetadataError::EmptyConflict);
        }
        if self.triggers.iter().any(String::is_empty) {
            return Err(CompositionMetadataError::EmptyTrigger);
        }
        if self.invalidation_targets.iter().any(String::is_empty) {
            return Err(CompositionMetadataError::EmptyInvalidationTarget);
        }
        if let Some(conflict) = self
            .dependencies
            .intersection(&self.conflicts)
            .next()
            .cloned()
        {
            return Err(CompositionMetadataError::DependencyConflict(conflict));
        }
        Ok(())
    }

    pub fn invalidation_for_change(
        &self,
        next: &Self,
    ) -> Result<BTreeSet<String>, CompositionMetadataError> {
        self.validate_pre_activation()?;
        next.validate_pre_activation()?;
        if self.identity != next.identity {
            return Err(CompositionMetadataError::ResourceIdentityMismatch {
                current: self.identity.clone(),
                next: next.identity.clone(),
            });
        }
        if self.content_identity == next.content_identity {
            return Ok(BTreeSet::new());
        }

        Ok(self
            .invalidation_targets
            .union(&next.invalidation_targets)
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Authority, ComponentExport, ComponentImport};

    fn plugin_manifest(execution: PluginExecution) -> PluginManifest {
        PluginManifest {
            id: PluginId::parse("third-party").unwrap(),
            version: 1,
            execution,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }

    fn package_metadata(execution: PluginExecution) -> PluginPackageMetadata {
        PluginPackageMetadata {
            manifest: plugin_manifest(execution),
            packaged_components: BTreeSet::new(),
            packaged_resources: BTreeSet::new(),
            packaged_skills: BTreeSet::new(),
            compatibility: CompatibilityMetadata {
                minimum_kernel_version: 1,
                maximum_kernel_version: None,
            },
            durable_namespaces: BTreeSet::new(),
            migrations: Vec::new(),
            configuration_frontends: BTreeSet::new(),
            component_hosts: BTreeSet::new(),
            reload_policy: ReloadPolicy::Restart,
        }
    }

    fn component_manifest(
        imports: Vec<ComponentImport>,
        exports: Vec<ComponentExport>,
    ) -> ComponentManifest {
        ComponentManifest {
            listeners: Vec::new(),
            id: ComponentId::parse("third-party-component").unwrap(),
            owner: PluginId::parse("third-party").unwrap(),
            imports,
            exports,
            maximum_authority: Authority::default(),
        }
    }

    fn component_metadata(
        imports: Vec<ComponentImport>,
        exports: Vec<ComponentExport>,
    ) -> ComponentRuntimeMetadata {
        ComponentRuntimeMetadata {
            manifest: component_manifest(imports, exports),
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

    fn skill_metadata() -> SkillResourceMetadata {
        SkillResourceMetadata {
            identity: "third-party.skill".into(),
            version: 1,
            content_identity: "sha256:abc".into(),
            dependencies: BTreeSet::new(),
            conflicts: BTreeSet::new(),
            triggers: BTreeSet::from(["review".into()]),
            scope: "execution".into(),
            priority: 0,
            required_tools: BTreeSet::from([CallableId::parse("read").unwrap()]),
            required_interfaces: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
            compatibility: CompatibilityMetadata {
                minimum_kernel_version: 1,
                maximum_kernel_version: None,
            },
            invalidation_targets: BTreeSet::from(["skill-index".into()]),
            reload_policy: ReloadPolicy::Restart,
        }
    }

    #[test]
    fn package_and_resource_metadata_reject_incompatible_kernel_versions_before_activation() {
        let future = KERNEL_COMPATIBILITY_VERSION + 1;
        let mut package = package_metadata(PluginExecution::Embedded);
        package.compatibility.minimum_kernel_version = future;
        assert_eq!(
            package.validate_pre_activation(),
            Err(CompositionMetadataError::IncompatibleKernelVersion {
                current: KERNEL_COMPATIBILITY_VERSION,
                minimum: future,
                maximum: None,
            })
        );

        let mut resource = skill_metadata();
        resource.compatibility.minimum_kernel_version = future;
        assert_eq!(
            resource.validate_pre_activation(),
            Err(CompositionMetadataError::IncompatibleKernelVersion {
                current: KERNEL_COMPATIBILITY_VERSION,
                minimum: future,
                maximum: None,
            })
        );
    }

    #[test]
    fn plugin_metadata_rejects_durable_namespace_not_owned_by_runtime_manifest_before_activation() {
        let namespace = ResourceNamespace::parse("third.party.state").unwrap();
        let mut metadata = package_metadata(PluginExecution::Embedded);
        metadata.durable_namespaces.insert(namespace.clone());

        assert_eq!(
            metadata.validate_pre_activation(),
            Err(CompositionMetadataError::DurableNamespaceOutsideRuntimeManifest(namespace))
        );
    }

    #[test]
    fn plugin_metadata_rejects_undeclared_durable_migration_before_activation() {
        let declared = ResourceNamespace::parse("phenix.context").unwrap();
        let undeclared = ResourceNamespace::parse("third.party.state").unwrap();
        let mut metadata = package_metadata(PluginExecution::Embedded);
        metadata.manifest.resource_namespaces.push(declared.clone());
        metadata.durable_namespaces.insert(declared);
        metadata.migrations.push(DurableMigrationMetadata {
            namespace: undeclared.clone(),
            from_version: 1,
            to_version: 2,
        });

        assert_eq!(
            metadata.validate_pre_activation(),
            Err(CompositionMetadataError::MigrationOutsideDeclaredNamespace(
                undeclared
            ))
        );
    }

    #[test]
    fn plugin_package_metadata_uses_the_canonical_runtime_manifest() {
        let metadata = package_metadata(PluginExecution::Runtime {
            runtime: crate::RuntimeId::parse("vendor.runtime").unwrap(),
            artifact: crate::PluginArtifact {
                locator: "plugin.wasm".into(),
                revision: crate::ArtifactRevision::from_content(b"fixture"),
                configuration: std::collections::BTreeMap::new(),
            },
        });

        metadata.validate_pre_activation().unwrap();
        assert_eq!(metadata.plugin().as_str(), "third-party");
        let encoded = serde_json::to_value(&metadata).unwrap();
        assert_eq!(encoded["id"], "third-party");
        assert_eq!(encoded["version"], 1);
        assert_eq!(encoded["execution"]["kind"], "runtime");
        assert_eq!(encoded["execution"]["runtime"], "vendor.runtime");
        assert_eq!(
            encoded["execution"]["artifact"]["revision"],
            crate::ArtifactRevision::from_content(b"fixture").as_ref()
        );
    }

    #[test]
    fn resource_only_package_cannot_claim_executable_components() {
        let mut metadata = package_metadata(PluginExecution::ResourceOnly);
        metadata
            .packaged_components
            .insert(ComponentId::parse("third-party-component").unwrap());

        assert_eq!(
            metadata.validate_pre_activation(),
            Err(CompositionMetadataError::ResourceOnlyPackageHasComponents)
        );
    }

    #[test]
    fn plugin_package_metadata_rejects_empty_resource_identity() {
        let mut metadata = package_metadata(PluginExecution::Embedded);
        metadata.packaged_resources.insert(String::new());
        assert_eq!(
            metadata.validate_pre_activation(),
            Err(CompositionMetadataError::EmptyPackagedResource)
        );
    }

    #[test]
    fn component_metadata_exposes_the_canonical_typed_contract_before_activation() {
        let required_interface = InterfaceId::parse("third.party.input@1").unwrap();
        let exported_interface = InterfaceId::parse("third.party.output@1").unwrap();
        let metadata = component_metadata(
            vec![ComponentImport {
                interface: required_interface.clone(),
                schema: Default::default(),
                required: true,
                authority: Authority::default(),
            }],
            vec![ComponentExport {
                interface: exported_interface.clone(),
                schema: Default::default(),
                priority: 10,
                required_authority: Authority::default(),
            }],
        );

        metadata.validate_pre_activation().unwrap();
        assert_eq!(metadata.component().as_str(), "third-party-component");
        let encoded = serde_json::to_value(&metadata).unwrap();
        assert_eq!(
            encoded["imports"][0]["interface"],
            required_interface.as_str()
        );
        assert_eq!(encoded["imports"][0]["required"], true);
        assert_eq!(
            encoded["exports"][0]["interface"],
            exported_interface.as_str()
        );
    }

    #[test]
    fn component_metadata_rejects_requested_capability_above_component_ceiling() {
        let denied = CapabilityId::parse("workspace.write").unwrap();
        let mut metadata = component_metadata(Vec::new(), Vec::new());
        metadata.requested_capabilities.insert(denied.clone());

        assert_eq!(
            metadata.validate_pre_activation(),
            Err(CompositionMetadataError::RequestedCapabilityExceedsMaximum(
                denied
            ))
        );
    }

    #[test]
    fn component_metadata_rejects_empty_controller_identity() {
        let mut metadata = component_metadata(Vec::new(), Vec::new());
        metadata.controller_contributions.insert(String::new());
        assert_eq!(
            metadata.validate_pre_activation(),
            Err(CompositionMetadataError::EmptyControllerContribution)
        );
    }

    #[test]
    fn component_metadata_rejects_duplicate_interface_contracts_before_activation() {
        let interface = InterfaceId::parse("third.party.shared@1").unwrap();
        let metadata = component_metadata(
            vec![
                ComponentImport {
                    interface: interface.clone(),
                    schema: Default::default(),
                    required: true,
                    authority: Authority::default(),
                },
                ComponentImport {
                    interface: interface.clone(),
                    schema: Default::default(),
                    required: false,
                    authority: Authority::default(),
                },
            ],
            Vec::new(),
        );

        assert_eq!(
            metadata.validate_pre_activation(),
            Err(CompositionMetadataError::DuplicateComponentImport(
                interface
            ))
        );
    }

    #[test]
    fn skill_resource_metadata_exposes_tools_and_invalidation_before_activation() {
        let metadata = skill_metadata();

        metadata.validate_pre_activation().unwrap();
        let encoded = serde_json::to_value(&metadata).unwrap();
        assert_eq!(encoded["required_tools"][0], "read");
        assert_eq!(encoded["invalidation_targets"][0], "skill-index");
    }

    #[test]
    fn skill_content_change_invalidates_only_declared_derived_state() {
        let current = skill_metadata();
        let mut next = current.clone();
        next.content_identity = "sha256:def".into();
        next.invalidation_targets
            .insert("context-projection".into());

        assert_eq!(
            current.invalidation_for_change(&next).unwrap(),
            BTreeSet::from(["context-projection".into(), "skill-index".into()])
        );
        assert!(next.invalidation_for_change(&next).unwrap().is_empty());
    }

    #[test]
    fn skill_resource_metadata_rejects_dependency_conflict_before_activation() {
        let dependency = "phenix.tools.read".to_owned();
        let mut metadata = skill_metadata();
        metadata.dependencies.insert(dependency.clone());
        metadata.conflicts.insert(dependency.clone());

        assert_eq!(
            metadata.validate_pre_activation(),
            Err(CompositionMetadataError::DependencyConflict(dependency))
        );
    }

    #[test]
    fn skill_resource_metadata_rejects_empty_relationship_identities() {
        let mut metadata = skill_metadata();
        metadata.dependencies.insert(String::new());
        assert_eq!(
            metadata.validate_pre_activation(),
            Err(CompositionMetadataError::EmptyDependency)
        );

        let mut metadata = skill_metadata();
        metadata.conflicts.insert(String::new());
        assert_eq!(
            metadata.validate_pre_activation(),
            Err(CompositionMetadataError::EmptyConflict)
        );

        let mut metadata = skill_metadata();
        metadata.triggers.insert(String::new());
        assert_eq!(
            metadata.validate_pre_activation(),
            Err(CompositionMetadataError::EmptyTrigger)
        );
    }
}
