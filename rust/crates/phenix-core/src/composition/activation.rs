use crate::{
    GenerationId, Kernel, KernelError, LayerPolicy, PluginId, PluginManifest,
    ResolvedComponentGraph, ResolvedGeneration, ResolvedListenerInspection, ServiceId,
    SkillResourceMetadata,
};
use std::collections::BTreeSet;

/// Installs one resolved generation as a single runtime activation unit.
///
/// Callers should activate a `ResolvedGeneration` rather than setting its generation
/// identity and component graph independently. Stable activation is immutable:
/// replacing an active generation belongs to development-mode reconciliation.
pub trait ResolvedGenerationActivation {
    fn activate_resolved_generation(
        &mut self,
        resolved: &ResolvedGeneration,
    ) -> Result<(), ResolvedGenerationActivationError>;
}

#[derive(Clone, Debug, PartialEq)]
pub enum ResolvedGenerationActivationError {
    KernelConfigurationMismatch {
        kernel_plugins: Vec<PluginId>,
        resolved_plugins: Vec<PluginId>,
    },
    KernelPluginManifestMismatch {
        plugin: PluginId,
        kernel_manifest: Box<PluginManifest>,
        resolved_manifest: Box<PluginManifest>,
    },
    KernelLayerPolicyMismatch {
        service: ServiceId,
        kernel_layers: Vec<LayerPolicy>,
        resolved_layers: Vec<LayerPolicy>,
    },
    DifferentGenerationAlreadyActive {
        active: GenerationId,
        requested: GenerationId,
    },
    DurableSchemaPreparation(KernelError),
    AuthorityCeiling(KernelError),
}

pub(crate) fn validate_resolved_generation_configuration(
    kernel: &Kernel,
    resolved: &ResolvedGeneration,
) -> Result<(), ResolvedGenerationActivationError> {
    let kernel_manifests: Vec<_> = kernel.config().manifests().cloned().collect();
    let kernel_plugins: Vec<_> = kernel_manifests
        .iter()
        .map(|manifest| manifest.id.clone())
        .collect();
    let resolved_plugins: Vec<_> = resolved
        .plugins()
        .iter()
        .map(|manifest| manifest.id.clone())
        .collect();
    if kernel_plugins != resolved_plugins {
        return Err(
            ResolvedGenerationActivationError::KernelConfigurationMismatch {
                kernel_plugins,
                resolved_plugins,
            },
        );
    }
    for (kernel_manifest, resolved_manifest) in
        kernel_manifests.iter().zip(resolved.plugins().iter())
    {
        if kernel_manifest != resolved_manifest {
            return Err(
                ResolvedGenerationActivationError::KernelPluginManifestMismatch {
                    plugin: kernel_manifest.id.clone(),
                    kernel_manifest: Box::new(kernel_manifest.clone()),
                    resolved_manifest: Box::new(resolved_manifest.clone()),
                },
            );
        }
    }

    let services: BTreeSet<_> = kernel
        .config()
        .manifests()
        .flat_map(|manifest| {
            manifest
                .services
                .iter()
                .map(|contribution| contribution.service.clone())
        })
        .chain(resolved.layer_policies().keys().cloned())
        .collect();
    for service in services {
        let kernel_layers = kernel.config().layer_policy(&service);
        let resolved_layers = resolved
            .layer_policies()
            .get(&service)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if kernel_layers != resolved_layers {
            return Err(
                ResolvedGenerationActivationError::KernelLayerPolicyMismatch {
                    service,
                    kernel_layers: kernel_layers.to_vec(),
                    resolved_layers: resolved_layers.to_vec(),
                },
            );
        }
    }
    Ok(())
}

impl ResolvedGenerationActivation for Kernel {
    fn activate_resolved_generation(
        &mut self,
        resolved: &ResolvedGeneration,
    ) -> Result<(), ResolvedGenerationActivationError> {
        validate_resolved_generation_configuration(self, resolved)?;

        if let Some(active) = self.graph_generation() {
            if active != resolved.generation() {
                return Err(
                    ResolvedGenerationActivationError::DifferentGenerationAlreadyActive {
                        active: active.clone(),
                        requested: resolved.generation().clone(),
                    },
                );
            }
        }

        self.prepare_durable_schemas(resolved.durable_schemas())
            .map_err(ResolvedGenerationActivationError::DurableSchemaPreparation)?;
        self.validate_generation_authority(resolved)
            .map_err(ResolvedGenerationActivationError::AuthorityCeiling)?;
        self.install_generation_topology(
            resolved.generation_topology().clone(),
            resolved.durable_schemas().to_vec(),
            resolved.authority_ceiling().clone(),
        );
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActiveGenerationGraph {
    pub generation: GenerationId,
    pub component_graph: ResolvedComponentGraph,
    pub resources: Vec<SkillResourceMetadata>,
}

impl ActiveGenerationGraph {
    pub fn from_resolved(resolved: &ResolvedGeneration) -> Self {
        Self {
            generation: resolved.generation().clone(),
            component_graph: resolved.component_graph().clone(),
            resources: resolved.resources().to_vec(),
        }
    }

    pub fn listeners(&self) -> impl Iterator<Item = ResolvedListenerInspection<'_>> {
        self.component_graph
            .listeners()
            .map(|listener| ResolvedListenerInspection {
                generation: &self.generation,
                listener,
            })
    }
}

impl Kernel {
    /// Returns one coherent snapshot of the active resolved generation.
    ///
    /// The snapshot is absent before activation. After activation, generation,
    /// component bindings, and selected resources come from the same resolved
    /// harness revision.
    pub fn active_generation_graph(&self) -> Option<ActiveGenerationGraph> {
        Some(ActiveGenerationGraph {
            generation: self.graph_generation()?.clone(),
            component_graph: self.component_graph().clone(),
            resources: self.active_resources().to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Authority, CompatibilityMetadata, ComponentId, ComponentManifest, KernelConfig,
        PluginExecution, ReloadPolicy, ServiceContribution, ServiceRole,
    };
    use std::collections::{BTreeMap, BTreeSet};

    fn resource() -> SkillResourceMetadata {
        SkillResourceMetadata {
            identity: "fixture.skill".into(),
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
            invalidation_targets: BTreeSet::from(["skill-index".into()]),
            reload_policy: ReloadPolicy::Restart,
        }
    }

    fn service() -> ServiceId {
        ServiceId::parse("fixture.service@1").unwrap()
    }

    fn layered_plugin() -> PluginManifest {
        PluginManifest {
            id: PluginId::parse("fixture.layer").unwrap(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: vec![ServiceContribution {
                role: ServiceRole::Layer,
                service: service(),
                priority: 10,
                required_authority: Authority::default(),
            }],
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }

    #[test]
    fn activation_installs_generation_and_graph_from_the_same_resolved_generation() {
        let resolved = ResolvedGeneration::resolve_with_resources(
            [],
            [],
            [resource()],
            [],
            &Authority::default(),
        )
        .unwrap();
        let expected = ActiveGenerationGraph::from_resolved(&resolved);
        let mut kernel = Kernel::kernel_only();

        kernel.activate_resolved_generation(&resolved).unwrap();

        assert_eq!(kernel.graph_generation(), Some(&expected.generation));
        assert_eq!(kernel.component_graph(), &expected.component_graph);
        assert_eq!(kernel.active_resources(), expected.resources.as_slice());
        assert_eq!(kernel.active_resources()[0].identity, "fixture.skill");
        assert_eq!(
            kernel.generation_topology().generation(),
            Some(&expected.generation)
        );
        assert_eq!(
            kernel.generation_topology().component_graph(),
            &expected.component_graph
        );
        assert_eq!(
            kernel.generation_topology().resources(),
            expected.resources.as_slice()
        );
        assert_eq!(kernel.active_generation_graph(), Some(expected));
    }

    #[test]
    fn activation_rejects_a_resolved_graph_for_a_different_kernel_configuration() {
        let plugin = PluginId::parse("fixture.plugin").unwrap();
        let resolved = ResolvedGeneration::resolve(
            [PluginManifest::resource_only(plugin.clone())],
            [],
            [],
            &Authority::default(),
        )
        .unwrap();
        let mut kernel = Kernel::kernel_only();

        let error = kernel.activate_resolved_generation(&resolved).unwrap_err();

        assert_eq!(
            error,
            ResolvedGenerationActivationError::KernelConfigurationMismatch {
                kernel_plugins: Vec::new(),
                resolved_plugins: vec![plugin],
            }
        );
        assert_eq!(kernel.graph_generation(), None);
        assert_eq!(kernel.component_graph(), &ResolvedComponentGraph::empty());
        assert!(kernel.active_resources().is_empty());
        assert_eq!(kernel.active_generation_graph(), None);
    }

    #[test]
    fn activation_rejects_same_plugin_identity_with_different_manifest_semantics() {
        let plugin = PluginId::parse("fixture.plugin").unwrap();
        let resolved_manifest = PluginManifest::resource_only(plugin.clone());
        let resolved =
            ResolvedGeneration::resolve([resolved_manifest.clone()], [], [], &Authority::default())
                .unwrap();
        let mut kernel_manifest = resolved_manifest.clone();
        kernel_manifest.version += 1;
        let mut kernel = Kernel::new(KernelConfig::new([kernel_manifest.clone()]).unwrap());

        let error = kernel.activate_resolved_generation(&resolved).unwrap_err();

        assert_eq!(
            error,
            ResolvedGenerationActivationError::KernelPluginManifestMismatch {
                plugin,
                kernel_manifest: Box::new(kernel_manifest),
                resolved_manifest: Box::new(resolved_manifest),
            }
        );
        assert_eq!(kernel.graph_generation(), None);
        assert_eq!(kernel.component_graph(), &ResolvedComponentGraph::empty());
    }

    #[test]
    fn activation_rejects_runtime_layer_policy_that_differs_from_the_resolved_generation() {
        let plugin = layered_plugin();
        let layer = LayerPolicy {
            plugin: plugin.id.clone(),
            priority: 10,
            required: true,
            enabled: true,
        };
        let resolved = ResolvedGeneration::resolve_with_layer_policies(
            [plugin.clone()],
            [],
            [],
            BTreeMap::from([(service(), vec![layer.clone()])]),
            &Authority::default(),
        )
        .unwrap();
        let mut kernel = Kernel::new(KernelConfig::new([plugin]).unwrap());

        let error = kernel.activate_resolved_generation(&resolved).unwrap_err();

        assert_eq!(
            error,
            ResolvedGenerationActivationError::KernelLayerPolicyMismatch {
                service: service(),
                kernel_layers: Vec::new(),
                resolved_layers: vec![layer],
            }
        );
        assert_eq!(kernel.graph_generation(), None);
        assert_eq!(kernel.component_graph(), &ResolvedComponentGraph::empty());
    }

    #[test]
    fn stable_activation_rejects_replacing_an_active_generation() {
        let plugin = PluginManifest {
            id: PluginId::parse("fixture.plugin").unwrap(),
            version: 1,
            execution: crate::PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        };
        let initial =
            ResolvedGeneration::resolve([plugin.clone()], [], [], &Authority::default()).unwrap();
        let replacement = ResolvedGeneration::resolve(
            [plugin.clone()],
            [ComponentManifest {
                listeners: Vec::new(),
                id: ComponentId::parse("fixture.component").unwrap(),
                owner: plugin.id.clone(),
                imports: Vec::new(),
                exports: Vec::new(),
                maximum_authority: Authority::default(),
            }],
            [],
            &Authority::default(),
        )
        .unwrap();
        assert_ne!(initial.generation(), replacement.generation());
        let expected = ActiveGenerationGraph::from_resolved(&initial);
        let mut kernel = Kernel::new(crate::KernelConfig::new([plugin]).unwrap());
        kernel.activate_resolved_generation(&initial).unwrap();

        let error = kernel
            .activate_resolved_generation(&replacement)
            .unwrap_err();

        assert_eq!(
            error,
            ResolvedGenerationActivationError::DifferentGenerationAlreadyActive {
                active: initial.generation().clone(),
                requested: replacement.generation().clone(),
            }
        );
        assert_eq!(kernel.graph_generation(), Some(&expected.generation));
        assert_eq!(kernel.component_graph(), &expected.component_graph);
    }
}
